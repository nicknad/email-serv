use clap::Parser;

use email_serv::config::Config;
use email_serv::http;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // setup configuration
    dotenv::dotenv().ok();
    env_logger::init();
    let config = Config::parse();

    // start the server
    http::serve(config).await?;

    Ok(())
}
