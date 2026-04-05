use std::process;

#[tokio::main]
async fn main() {
    if let Err(error) = atai::app::run().await {
        eprintln!("Error: {error:#}");
        process::exit(1);
    }
}
