use tracing_subscriber::{EnvFilter, fmt};

fn main() {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("crabclaw=info"));

    let json_mode = std::env::var("LOG_FORMAT")
        .map(|v| v.eq_ignore_ascii_case("json"))
        .unwrap_or(false);

    if json_mode {
        fmt()
            .json()
            .with_env_filter(filter)
            .with_target(true)
            .with_current_span(true)
            .init();
    } else {
        fmt()
            .compact()
            .with_env_filter(filter)
            .with_target(true)
            .init();
    }

    if let Err(err) = crabclaw::channels::cli::run() {
        eprintln!("{err}");
        std::process::exit(1);
    }
}
