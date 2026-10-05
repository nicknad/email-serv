#[tokio::main]
async fn main() -> anyhow::Result<()> {
    email_serv::run().await
}
