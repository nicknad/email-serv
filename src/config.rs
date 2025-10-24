#[derive(clap::Parser)]

pub struct Config {
    #[clap(long, env)]
    pub db_conn: String,

    #[clap(long, env)]
    pub port: u16,
}
