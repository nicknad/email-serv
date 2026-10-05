#[derive(clap::Parser)]
pub struct Config {
    #[clap(long, env)]
    pub db_conn: String,

    #[clap(long, env)]
    pub port: u16,

    #[clap(long, env)]
    pub blake3_key: String,

    #[clap(long, env, required = false)]
    pub log_dir: String,

    #[clap(long, env)]
    pub smtp_host: String,

    #[clap(long, env)]
    pub smtp_port: u16,

    #[clap(long, env)]
    pub smtp_user: String,

    #[clap(long, env)]
    pub smtp_pass: String,

    #[clap(long, env)]
    pub email_from: String,

    #[clap(long, env)]
    pub admin_api_key: String,

    #[clap(long, env, default_value = "http://localhost:3000")]
    pub site_url: String,
}
