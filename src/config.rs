use serde::Deserialize;

#[derive(Deserialize)]
pub struct Config {
    pub global: Global,
}

#[derive(Deserialize)]
pub struct Global {
    pub address: String,
    pub db_url: String,
    pub pool_size: usize,
}