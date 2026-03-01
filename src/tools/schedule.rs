use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tracing::{debug, error, info, warn};

/// Whether a schedule job sends a static reminder or runs the agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobMode {
    /// Just deliver the message text (cheap, no LLM call).
    Reminder,
    /// Run the full agent pipeline with the message as prompt.
    /// The agent can call tools (web.fetch, etc.) and the result
    /// is delivered back to the user.
    Agent,
}

impl std::fmt::Display for JobMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JobMode::Reminder => write!(f, "reminder"),
            JobMode::Agent => write!(f, "agent"),
        }
    }
}

/// A scheduled job with its metadata.
#[derive(Debug, Clone)]
struct ScheduledJob {
    id: String,
    message: String,
    mode: JobMode,
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

/// Notification callback type — each job captures its own notifier.
pub type Notifier = Arc<dyn Fn(String) + Send + Sync>;

/// Async agent runner callback — runs the full agent pipeline with a prompt.
///
/// Captures config, workspace, session_id, and delivery mechanism.
/// When invoked, it calls the agent loop and sends the result to the user.
pub type AgentRunner =
    Arc<dyn Fn(String) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;

/// In-memory scheduler that manages timed jobs.
///
/// Jobs are executed by spawning tokio tasks. The scheduler is process-wide
/// and stored as a global singleton so `execute_tool` (which has a fixed
/// sync signature) can access it without signature changes.
#[derive(Clone, Default)]
pub struct SchedulerService {
    jobs: Arc<Mutex<HashMap<String, ScheduledJob>>>,
    /// Handles for spawned tokio tasks, keyed by job ID.
    handles: Arc<Mutex<HashMap<String, tokio::task::JoinHandle<()>>>>,
}

impl std::fmt::Debug for SchedulerService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SchedulerService")
            .field("jobs", &self.jobs)
            .finish()
    }
}

impl SchedulerService {
    pub fn new() -> Self {
        Self {
            jobs: Arc::new(Mutex::new(HashMap::new())),
            handles: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Add a scheduled job with optional notification and agent runner callbacks.
    ///
    /// - **Reminder mode**: fires `notifier(message)` — cheap, no LLM call.
    /// - **Agent mode**: fires `agent_runner(message).await` — runs full agent
    ///   pipeline (LLM + tools) and delivers the result.
    ///
    /// Each job captures its own callbacks — context-bound closures.
    pub fn add_job(
        &self,
        message: &str,
        after_seconds: Option<u64>,
        interval_seconds: Option<u64>,
        mode: JobMode,
        notifier: Option<Notifier>,
        agent_runner: Option<AgentRunner>,
    ) -> String {
        if after_seconds.is_none() && interval_seconds.is_none() {
            return "Error: must specify either 'after_seconds' or 'interval_seconds'".to_string();
        }
        if mode == JobMode::Agent && agent_runner.is_none() {
            return "Error: agent mode requires an agent runner (not available in this channel)"
                .to_string();
        }

        let id = generate_job_id();
        let after = after_seconds.map(Duration::from_secs);
        let interval = interval_seconds.map(Duration::from_secs);

        let job = ScheduledJob {
            id: id.clone(),
            message: message.to_string(),
            mode,
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
                    debug!(job_id = %job_id, "schedule: firing one-shot");
                    fire_job(&notifier, &agent_runner, &job_id, &msg).await;
                    let mut jobs = jobs_ref.lock().unwrap();
                    jobs.remove(&job_id);
                }
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
                    debug!(job_id = %job_id, "schedule: firing interval");
                    fire_job(&notifier, &agent_runner, &job_id, &msg).await;
                }
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
                    "{} mode={} schedule={} msg={}",
                    j.id,
                    j.mode,
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

/// Fire a job — either runs the agent pipeline or sends a simple notification.
async fn fire_job(
    notifier: &Option<Notifier>,
    agent_runner: &Option<AgentRunner>,
    job_id: &str,
    message: &str,
) {
    // Agent mode: run the full agent pipeline with the message as prompt
    if let Some(runner) = agent_runner {
        info!(job_id = %job_id, "schedule: running agent-mode job");
        let fut = runner(message.to_string());
        match tokio::task::spawn(fut).await {
            Ok(()) => {
                info!(job_id = %job_id, "schedule: agent-mode job completed");
            }
            Err(e) => {
                error!(job_id = %job_id, error = %e, "schedule: agent-mode job panicked");
                // Fall back to sending the error via notifier so the user knows
                if let Some(notify_fn) = notifier {
                    notify_fn(format!(
                        "\u{26a0} [Schedule {job_id}] Agent job failed: {e}"
                    ));
                }
            }
        }
        return;
    }

    // Reminder mode: just send the message text
    let text = format!("\u{23f0} [Reminder: {job_id}] {message}");
    if let Some(notify_fn) = notifier {
        notify_fn(text);
    } else {
        warn!(job_id = %job_id, "schedule: no notifier available, printing to stderr");
        eprintln!("[schedule:{job_id}] {message}");
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
        let result = svc.add_job(
            "test reminder",
            Some(60),
            None,
            JobMode::Reminder,
            None,
            None,
        );
        assert!(result.starts_with("scheduled:"), "got: {result}");
        assert!(result.contains("once in"), "got: {result}");
    }

    #[tokio::test]
    async fn add_interval_returns_scheduled() {
        let svc = fresh_service();
        let result = svc.add_job("repeating", None, Some(300), JobMode::Reminder, None, None);
        assert!(result.starts_with("scheduled:"), "got: {result}");
        assert!(result.contains("every 300s"), "got: {result}");
    }

    #[test]
    fn add_requires_timing_parameter() {
        let svc = fresh_service();
        let result = svc.add_job("no timing", None, None, JobMode::Reminder, None, None);
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
        svc.add_job("reminder 1", Some(60), None, JobMode::Reminder, None, None);
        svc.add_job("reminder 2", None, Some(120), JobMode::Reminder, None, None);
        let result = svc.list_jobs();
        assert!(result.contains("reminder 1"), "got: {result}");
        assert!(result.contains("reminder 2"), "got: {result}");
    }

    #[tokio::test]
    async fn remove_existing_job() {
        let svc = fresh_service();
        let add_result = svc.add_job("to remove", Some(60), None, JobMode::Reminder, None, None);
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
        svc.add_job("one", Some(60), None, JobMode::Reminder, None, None);
        assert_eq!(svc.active_count(), 1);
        svc.add_job("two", Some(120), None, JobMode::Reminder, None, None);
        assert_eq!(svc.active_count(), 2);
    }

    #[tokio::test]
    async fn per_job_notifier_is_called() {
        let svc = fresh_service();
        let received = Arc::new(Mutex::new(Vec::new()));
        let recv_clone = received.clone();
        let notifier: Notifier = Arc::new(move |msg| {
            recv_clone.lock().unwrap().push(msg);
        });

        // One-shot job with 0-second delay
        svc.add_job(
            "drink water",
            Some(0),
            None,
            JobMode::Reminder,
            Some(notifier),
            None,
        );

        // Wait for it to fire
        tokio::time::sleep(Duration::from_millis(100)).await;

        let msgs = received.lock().unwrap();
        assert_eq!(msgs.len(), 1, "expected 1 notification, got: {msgs:?}");
        assert!(msgs[0].contains("drink water"), "got: {}", msgs[0]);
    }
}
