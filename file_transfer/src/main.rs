use clap::Parser;
use tracing_subscriber::EnvFilter;

mod request_handlers;
mod client;
mod network;
mod common;

#[derive(Parser)]
struct Opts {
    #[arg(long, default_value_t = 4)]
    seed: u64,
}

#[tokio::main]
async fn main() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();
    let opts = Opts::parse();

}