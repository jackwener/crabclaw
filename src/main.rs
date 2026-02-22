use tracing_subscriber::EnvFilter;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    if let Err(err) = crabclaw::cli::run() {
        eprintln!("{err}");
        std::process::exit(1);
    }
}
