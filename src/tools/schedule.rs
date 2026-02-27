use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tracing::debug;

/// A scheduled job with its metadata.
#[derive(Debug, Clone)]
struct ScheduledJob {
    id: String,
    message: String,
    created_at: Instant,
    /// For one-shot: fires after this duration from creation
    after: Option<Duration>,
    /// For repeating: fires at this interval
    interval: Option<Duration>,
    /// Whether the job has been cancelled
    cancelled: bool,
}

impl ScheduledJob {
    /// Human-readable description of when the job fires.
    fn schedule_description(&self) -> String {
        if let Some(after) = self.after {
            let fires_in = after
                .checked_sub(self.created_at.elapsed())
                .unwrap_or(Duration::ZERO);
            format!("once in {}s", fires_in.as_secs())
        } else if let Some(interval) = self.interval {
            format!("every {}s", interval.as_secs())
        } else {
            "unknown".to_string()
        }
    }
}

/// In-memory scheduler that manages timed jobs.
///
/// Jobs are executed by spawning tokio tasks. The scheduler is process-wide
/// and stored as a global singleton so `execute_tool` (which has a fixed
/// sync signature) can access it without signature changes.
#[derive(Debug, Clone)]
pub struct SchedulerService {
    jobs: Arc<Mutex<HashMap<String, ScheduledJob>>>,
    /// Handles for spawned tokio tasks, keyed by job ID.
    /// Used to cancel tasks when removing jobs.
    handles: Arc<Mutex<HashMap<String, tokio::task::JoinHandle<()>>>>,
}

impl SchedulerService {
    pub fn new() -> Self {
        Self {
            jobs: Arc::new(Mutex::new(HashMap::new())),
            handles: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Add a scheduled job.
    ///
    /// - `after_seconds`: fire once after this many seconds
    /// - `interval_seconds`: fire repeatedly at this interval
    /// - `message`: the reminder text to deliver
    ///
    /// Returns a status string like "scheduled: abc123 fires=once in 60s"
    pub fn add_job(
        &self,
        message: &str,
        after_seconds: Option<u64>,
        interval_seconds: Option<u64>,
    ) -> String {
        if after_seconds.is_none() && interval_seconds.is_none() {
            return "Error: must specify either 'after_seconds' or 'interval_seconds'".to_string();
        }

        let id = generate_job_id();
        let after = after_seconds.map(Duration::from_secs);
        let interval = interval_seconds.map(Duration::from_secs);

        let job = ScheduledJob {
            id: id.clone(),
            message: message.to_string(),
            created_at: Instant::now(),
            after,
            interval,
            cancelled: false,
        };

        let description = job.schedule_description();

        // Store the job
        {
            let mut jobs = self.jobs.lock().unwrap();
            jobs.insert(id.clone(), job);
        }

        // Spawn the timer task
        let jobs_ref = self.jobs.clone();
        let handles_ref = self.handles.clone();
        let job_id = id.clone();
        let msg = message.to_string();

        let handle = match tokio::runtime::Handle::try_current() {
            Ok(h) => h,
            Err(_) => {
                // No runtime — remove the job we just inserted and return error
                let mut jobs = self.jobs.lock().unwrap();
                jobs.remove(&id);
                return "Error: no async runtime available to schedule jobs".to_string();
            }
        };

        let task_handle = handle.spawn(async move {
            if let Some(delay) = after {
                // One-shot timer
                tokio::time::sleep(delay).await;
                let cancelled = {
                    let jobs = jobs_ref.lock().unwrap();
                    jobs.get(&job_id).map(|j| j.cancelled).unwrap_or(true)
                };
                if !cancelled {
                    debug!(job_id = %job_id, "schedule: firing one-shot reminder");
                    eprintln!("[schedule:{}] {}", job_id, msg);
                    // Clean up after firing
                    let mut jobs = jobs_ref.lock().unwrap();
                    jobs.remove(&job_id);
                }
                // Clean up handle
                let mut handles = handles_ref.lock().unwrap();
                handles.remove(&job_id);
            } else if let Some(interval_dur) = interval {
                // Repeating timer
                let mut ticker = tokio::time::interval(interval_dur);
                ticker.tick().await; // first tick fires immediately, skip it
                loop {
                    ticker.tick().await;
                    let cancelled = {
                        let jobs = jobs_ref.lock().unwrap();
                        jobs.get(&job_id).map(|j| j.cancelled).unwrap_or(true)
                    };
                    if cancelled {
                        break;
                    }
                    debug!(job_id = %job_id, "schedule: firing interval reminder");
                    eprintln!("[schedule:{}] {}", job_id, msg);
                }
                // Clean up handle
                let mut handles = handles_ref.lock().unwrap();
                handles.remove(&job_id);
            }
        });

        let mut handles = self.handles.lock().unwrap();
        handles.insert(id.clone(), task_handle);

        format!("scheduled: {id} fires={description}")
    }

    /// List all active jobs.
    pub fn list_jobs(&self) -> String {
        let jobs = self.jobs.lock().unwrap();
        if jobs.is_empty() {
            return "(no scheduled jobs)".to_string();
        }

        let mut rows: Vec<String> = jobs
            .values()
            .filter(|j| !j.cancelled)
            .map(|j| {
                format!(
                    "{} schedule={} msg={}",
                    j.id,
                    j.schedule_description(),
                    j.message
                )
            })
            .collect();
        rows.sort();
        rows.join("\n")
    }

    /// Remove a job by ID.
    pub fn remove_job(&self, job_id: &str) -> String {
        // Mark as cancelled
        {
            let mut jobs = self.jobs.lock().unwrap();
            match jobs.get_mut(job_id) {
                Some(job) => {
                    job.cancelled = true;
                    jobs.remove(job_id);
                }
                None => return format!("Error: job not found: {job_id}"),
            }
        }

        // Abort the tokio task
        {
            let mut handles = self.handles.lock().unwrap();
            if let Some(handle) = handles.remove(job_id) {
                handle.abort();
            }
        }

        format!("removed: {job_id}")
    }

    /// Number of active (non-cancelled) jobs.
    pub fn active_count(&self) -> usize {
        let jobs = self.jobs.lock().unwrap();
        jobs.values().filter(|j| !j.cancelled).count()
    }
}

impl Default for SchedulerService {
    fn default() -> Self {
        Self::new()
    }
}

/// Generate a short random job ID (8 hex chars).
fn generate_job_id() -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::time::SystemTime;

    let mut hasher = DefaultHasher::new();
    SystemTime::now().hash(&mut hasher);
    std::thread::current().id().hash(&mut hasher);
    format!("{:08x}", hasher.finish() as u32)
}

/// Global scheduler singleton.
///
/// This allows `execute_tool` (which has a fixed sync signature) to access
/// the scheduler without changing its function signature.
static GLOBAL_SCHEDULER: std::sync::OnceLock<SchedulerService> = std::sync::OnceLock::new();

/// Get or initialize the global scheduler.
pub fn global_scheduler() -> &'static SchedulerService {
    GLOBAL_SCHEDULER.get_or_init(SchedulerService::new)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_service() -> SchedulerService {
        SchedulerService::new()
    }

    #[tokio::test]
    async fn add_after_seconds_returns_scheduled() {
        let svc = fresh_service();
        let result = svc.add_job("test reminder", Some(60), None);
        assert!(result.starts_with("scheduled:"), "got: {result}");
        assert!(result.contains("once in"), "got: {result}");
    }

    #[tokio::test]
    async fn add_interval_returns_scheduled() {
        let svc = fresh_service();
        let result = svc.add_job("repeating", None, Some(300));
        assert!(result.starts_with("scheduled:"), "got: {result}");
        assert!(result.contains("every 300s"), "got: {result}");
    }

    #[test]
    fn add_requires_timing_parameter() {
        let svc = fresh_service();
        let result = svc.add_job("no timing", None, None);
        assert!(result.starts_with("Error:"), "got: {result}");
    }

    #[test]
    fn list_empty() {
        let svc = fresh_service();
        let result = svc.list_jobs();
        assert_eq!(result, "(no scheduled jobs)");
    }

    #[tokio::test]
    async fn list_with_jobs() {
        let svc = fresh_service();
        svc.add_job("reminder 1", Some(60), None);
        svc.add_job("reminder 2", None, Some(120));
        let result = svc.list_jobs();
        assert!(result.contains("reminder 1"), "got: {result}");
        assert!(result.contains("reminder 2"), "got: {result}");
    }

    #[tokio::test]
    async fn remove_existing_job() {
        let svc = fresh_service();
        let add_result = svc.add_job("to remove", Some(60), None);
        // Extract job ID from "scheduled: abc123 fires=..."
        let job_id = add_result
            .strip_prefix("scheduled: ")
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap();

        let result = svc.remove_job(job_id);
        assert!(result.starts_with("removed:"), "got: {result}");
        assert_eq!(svc.active_count(), 0);
    }

    #[test]
    fn remove_nonexistent_returns_error() {
        let svc = fresh_service();
        let result = svc.remove_job("fake_id");
        assert!(result.starts_with("Error:"), "got: {result}");
    }

    #[tokio::test]
    async fn active_count_tracks_correctly() {
        let svc = fresh_service();
        assert_eq!(svc.active_count(), 0);
        svc.add_job("one", Some(60), None);
        assert_eq!(svc.active_count(), 1);
        svc.add_job("two", Some(120), None);
        assert_eq!(svc.active_count(), 2);
    }
}
