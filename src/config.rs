use std::env;

pub struct Config {
    pub host: String,
    pub port: u16,
    pub mongodb_uri: String,
    pub mongodb_database: String,
}

impl Config {
    pub fn from_env() -> Result<Self, Box<dyn std::error::Error>> {
        let host = env::var("APP_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());

        let port: u16 = env::var("APP_PORT")
            .unwrap_or_else(|_| "8000".to_string())
            .parse()?;

        let mongodb_uri =
            env::var("MONGODB_URI").unwrap_or_else(|_| "mongodb://127.0.0.1:27017".to_string());

        let mongodb_database =
            env::var("MONGODB_DATABASE").unwrap_or_else(|_| "orderrecon".to_string());

        Ok(Self {
            host,
            port,
            mongodb_uri,
            mongodb_database,
        })
    }
}
