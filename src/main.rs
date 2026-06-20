#[tokio::main]
async fn main() {
    if let Err(err) = ozr::cli::run().await {
        eprintln!("error: {}", err);
        std::process::exit(1);
    }
}
