use envconfig::Envconfig;

#[derive(Envconfig, serde::Deserialize, Debug, Clone)]
pub struct Config {
    #[envconfig(from = "REDIS_ADDR", default = "redis://127.0.0.1")]
    pub redis_addr: String,

    #[envconfig(from = "PARTITIONNER_ADDR", default = "http://localhost")]
    pub partitionner_addr: String,

    #[envconfig(from = "PARTITIONNER_PORT", default = "3535")]
    pub partitionner_port: u16,

    #[envconfig(from = "RUNNER_EXE", default = "steadyum-runner.exe")]
    pub runner_exe: String,

    #[envconfig(from = "PRIV_NET_INT", default = "ens4")]
    pub priv_net_int: String,

    #[envconfig(from = "ZENOH_ROUTER", default = "tcp/162.19.70.139:7447")]
    pub zenoh_router: String,
}

pub fn get_config() -> Config {
    dotenv::dotenv().ok();
    match Config::init_from_env() {
        Ok(config) => config,
        Err(error) => panic!("Configuration Error: {error:#?}"),
    }
}

lazy_static::lazy_static! {
    pub static ref CONFIG: Config = get_config();
}
