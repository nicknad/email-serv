use clap::Parser;

use email_serv::config::Config;
use email_serv::http;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();
    env_logger::init();
    let config = Config::parse();

    println!("{:?}", config.db_conn);
    Ok(())
}
