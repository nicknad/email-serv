use clap::Parser;
use email_serv::config::Config;
use email_serv::http;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // setup configuration
    dotenv::dotenv().ok();

    // construct a subscriber that prints formatted traces to stdout
    let subscriber = tracing_subscriber::FmtSubscriber::new();
    // use that subscriber to process traces emitted after this point
    tracing::subscriber::set_global_default(subscriber).unwrap();

    let config = Config::parse();

    // start the server
    http::serve(config).await?;

    Ok(())
}
