use crate::error::Error;

#[derive(Debug, Clone)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    pub dbname: String,
    pub application_name: Option<String>,
    pub connect_timeout: Option<std::time::Duration>,
}

impl Config {
    pub fn parse(s: &str) -> Result<Self, Error> {
        let mut host = "127.0.0.1".to_string();
        let mut port = 5432u16;
        let mut user = String::new();
        let mut password = String::new();
        let mut dbname = String::new();
        let mut application_name = None;
        let mut connect_timeout = None;

        for part in s.split_whitespace() {
            if let Some((key, value)) = part.split_once('=') {
                match key {
                    "host" => host = value.to_string(),
                    "port" => port = value.parse().map_err(|_| Error::Config("invalid port".into()))?,
                    "user" => user = value.to_string(),
                    "password" => password = value.to_string(),
                    "dbname" => dbname = value.to_string(),
                    "application_name" => application_name = Some(value.to_string()),
                    "connect_timeout" => {
                        let secs: u64 = value.parse().map_err(|_| Error::Config("invalid timeout".into()))?;
                        connect_timeout = Some(std::time::Duration::from_secs(secs));
                    }
                    _ => {}
                }
            }
        }

        if user.is_empty() {
            return Err(Error::Config("user is required".into()));
        }

        Ok(Config {
            host,
            port,
            user,
            password,
            dbname,
            application_name,
            connect_timeout,
        })
    }
}
