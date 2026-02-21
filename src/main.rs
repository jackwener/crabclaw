fn main() {
    if let Err(err) = crabclaw::cli::run() {
        eprintln!("{err}");
        std::process::exit(1);
    }
}
