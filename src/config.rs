use crate::error::Error;

#[derive(Debug, Clone, PartialEq)]
pub enum SslMode {
    Disable,
    Prefer,
    Require,
    VerifyCa,
    VerifyFull,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    pub dbname: String,
    pub application_name: Option<String>,
    pub connect_timeout: Option<std::time::Duration>,
    pub ssl_mode: SslMode,
    pub ssl_root_cert: Option<String>,
    pub ssl_cert: Option<String>,
    pub ssl_key: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            host: "127.0.0.1".to_string(),
            port: 5432,
            user: String::new(),
            password: String::new(),
            dbname: "postgres".to_string(),
            application_name: None,
            connect_timeout: None,
            ssl_mode: SslMode::Prefer,
            ssl_root_cert: None,
            ssl_cert: None,
            ssl_key: None,
        }
    }
}

impl Config {
    pub fn new() -> Self {
        Config::default()
    }

    /// Fill missing values from environment variables (PGHOST, PGPORT, etc.).
    pub fn apply_env(&mut self) {
        if self.host == "127.0.0.1" {
            if let Ok(v) = std::env::var("PGHOST") { self.host = v; }
        }
        if self.port == 5432 {
            if let Ok(v) = std::env::var("PGPORT") {
                if let Ok(p) = v.parse() { self.port = p; }
            }
        }
        if self.user.is_empty() {
            if let Ok(v) = std::env::var("PGUSER") { self.user = v; }
        }
        if self.password.is_empty() {
            if let Ok(v) = std::env::var("PGPASSWORD") { self.password = v; }
        }
        if self.dbname == "postgres" {
            if let Ok(v) = std::env::var("PGDATABASE") { self.dbname = v; }
        }
        if self.application_name.is_none() {
            if let Ok(v) = std::env::var("PGAPPNAME") { self.application_name = Some(v); }
        }
        if self.connect_timeout.is_none() {
            if let Ok(v) = std::env::var("PGCONNECT_TIMEOUT") {
                if let Ok(s) = v.parse() {
                    self.connect_timeout = Some(std::time::Duration::from_secs(s));
                }
            }
        }
        if self.ssl_mode == SslMode::Prefer {
            if let Ok(v) = std::env::var("PGSSLMODE") {
                if let Ok(m) = parse_sslmode(&v) { self.ssl_mode = m; }
            }
        }
        if self.ssl_root_cert.is_none() {
            if let Ok(v) = std::env::var("PGSSLROOTCERT") { self.ssl_root_cert = Some(v); }
        }
        if self.ssl_cert.is_none() {
            if let Ok(v) = std::env::var("PGSSLCERT") { self.ssl_cert = Some(v); }
        }
        if self.ssl_key.is_none() {
            if let Ok(v) = std::env::var("PGSSLKEY") { self.ssl_key = Some(v); }
        }
    }

    pub fn parse(s: &str) -> Result<Self, Error> {
        if s.starts_with("postgresql://") || s.starts_with("postgres://") {
            Self::parse_uri(s)
        } else {
            Self::parse_key_value(s)
        }
    }

    fn parse_uri(uri: &str) -> Result<Self, Error> {
        let mut config = Config::default();

        let without_scheme = if let Some(rest) = uri.strip_prefix("postgresql://") {
            rest
        } else if let Some(rest) = uri.strip_prefix("postgres://") {
            rest
        } else {
            return Err(Error::Config("invalid URI scheme".into()));
        };

        // Split authority + path from query
        let (authority_path, query) = match without_scheme.split_once('?') {
            Some((ap, q)) => (ap, q),
            None => (without_scheme, ""),
        };

        // Split authority from path
        let (authority, path) = match authority_path.split_once('/') {
            Some((auth, p)) => (auth, p.trim_start_matches('/')),
            None => (authority_path, ""),
        };

        // Parse user:password@host:port
        let userinfo_hostport = if let Some(rest) = authority.strip_prefix('@') {
            // No userinfo: just host:port
            rest
        } else if let Some((userinfo, rest)) = authority.split_once('@') {
            // Has userinfo
            if let Some((user, pass)) = userinfo.split_once(':') {
                config.user = percent_decode(user)?;
                config.password = percent_decode(pass)?;
            } else {
                config.user = percent_decode(userinfo)?;
            }
            rest
        } else {
            authority
        };

        // Parse host:port
        if let Some((host, port_str)) = userinfo_hostport.split_once(':') {
            config.host = host.to_string();
            config.port = port_str
                .parse()
                .map_err(|_| Error::Config("invalid port in URI".into()))?;
        } else if !userinfo_hostport.is_empty() {
            config.host = userinfo_hostport.to_string();
        }

        // Path component is the database name
        if !path.is_empty() {
            config.dbname = path.to_string();
        }

        // Query parameters
        if !query.is_empty() {
            for pair in query.split('&') {
                if let Some((k, v)) = pair.split_once('=') {
                    match k {
                        "application_name" => config.application_name = Some(percent_decode(v)?),
                        "connect_timeout" => {
                            let secs: u64 = v
                                .parse()
                                .map_err(|_| Error::Config("invalid timeout in URI".into()))?;
                            config.connect_timeout = Some(std::time::Duration::from_secs(secs));
                        }
                        "sslmode" => {
                            config.ssl_mode = parse_sslmode(v)?;
                        }
                        "sslrootcert" => config.ssl_root_cert = Some(percent_decode(v)?),
                        "sslcert" => config.ssl_cert = Some(percent_decode(v)?),
                        "sslkey" => config.ssl_key = Some(percent_decode(v)?),
                        _ => {}
                    }
                }
            }
        }

        if config.user.is_empty() {
            return Err(Error::Config("user is required".into()));
        }
        if config.dbname.is_empty() {
            config.dbname = "postgres".to_string();
        }

        Ok(config)
    }

    fn parse_key_value(s: &str) -> Result<Self, Error> {
        let mut config = Config::default();
        let mut user = String::new();
        let mut dbname = String::new();

        let parts = split_key_value_parts(s);
        for part in &parts {
            if let Some((key, value)) = part.split_once('=') {
                match key.trim() {
                    "host" => config.host = value.to_string(),
                    "port" => {
                        config.port = value
                            .parse()
                            .map_err(|_| Error::Config("invalid port".into()))?
                    }
                    "user" => user = value.to_string(),
                    "password" => config.password = value.to_string(),
                    "dbname" => dbname = value.to_string(),
                    "application_name" => config.application_name = Some(value.to_string()),
                    "connect_timeout" => {
                        let secs: u64 = value
                            .parse()
                            .map_err(|_| Error::Config("invalid timeout".into()))?;
                        config.connect_timeout = Some(std::time::Duration::from_secs(secs));
                    }
                    "sslmode" => {
                        config.ssl_mode = parse_sslmode(value)?;
                    }
                    "sslrootcert" => config.ssl_root_cert = Some(value.to_string()),
                    "sslcert" => config.ssl_cert = Some(value.to_string()),
                    "sslkey" => config.ssl_key = Some(value.to_string()),
                    _ => {}
                }
            }
        }

        if user.is_empty() {
            return Err(Error::Config("user is required".into()));
        }
        config.user = user;

        if dbname.is_empty() {
            dbname = "postgres".to_string();
        }
        config.dbname = dbname;

        Ok(config)
    }
}

/// Split a key=value DSN string into parts, respecting single and double quotes.
fn split_key_value_parts(s: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let ch = chars[i];
        match ch {
            '\'' if !in_double => {
                in_single = !in_single;
                current.push(ch);
            }
            '"' if !in_single => {
                in_double = !in_double;
                current.push(ch);
            }
            ' ' if !in_single && !in_double => {
                if !current.is_empty() {
                    parts.push(current.clone());
                    current.clear();
                }
            }
            _ => {
                current.push(ch);
            }
        }
        i += 1;
    }

    if !current.is_empty() {
        parts.push(current);
    }

    // Strip surrounding quotes from values
    parts
        .iter()
        .map(|p| {
            if let Some((key, value)) = p.split_once('=') {
                let stripped = value
                    .strip_prefix('\'')
                    .and_then(|v| v.strip_suffix('\''))
                    .or_else(|| {
                        value
                            .strip_prefix('"')
                            .and_then(|v| v.strip_suffix('"'))
                    })
                    .unwrap_or(value);
                format!("{}={}", key, stripped)
            } else {
                p.clone()
            }
        })
        .collect()
}

fn parse_sslmode(s: &str) -> Result<SslMode, Error> {
    match s {
        "disable" => Ok(SslMode::Disable),
        "prefer" => Ok(SslMode::Prefer),
        "require" => Ok(SslMode::Require),
        "verify-ca" => Ok(SslMode::VerifyCa),
        "verify-full" => Ok(SslMode::VerifyFull),
        _ => Err(Error::Config(format!("invalid sslmode: {}", s))),
    }
}

fn percent_decode(s: &str) -> Result<String, Error> {
    let mut result = String::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3])
                .map_err(|_| Error::Config("invalid percent encoding".into()))?;
            let byte = u8::from_str_radix(hex, 16)
                .map_err(|_| Error::Config("invalid percent encoding".into()))?;
            result.push(byte as char);
            i += 3;
        } else if bytes[i] == b'+' {
            result.push(' ');
            i += 1;
        } else {
            result.push(bytes[i] as char);
            i += 1;
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_dbname_to_postgres_when_omitted() {
        let config = Config::parse("host=127.0.0.1 user=root password=secret").unwrap();
        assert_eq!(config.dbname, "postgres");
    }

    #[test]
    fn defaults_dbname_to_postgres_when_empty() {
        let config = Config::parse("host=127.0.0.1 user=root password=secret dbname=").unwrap();
        assert_eq!(config.dbname, "postgres");
    }

    #[test]
    fn keeps_explicit_dbname() {
        let config =
            Config::parse("host=127.0.0.1 user=root password=secret dbname=app").unwrap();
        assert_eq!(config.dbname, "app");
    }

    #[test]
    fn parse_uri_format() {
        let config =
            Config::parse("postgresql://user:pass@localhost:5432/mydb?application_name=myapp")
                .unwrap();
        assert_eq!(config.host, "localhost");
        assert_eq!(config.port, 5432);
        assert_eq!(config.user, "user");
        assert_eq!(config.password, "pass");
        assert_eq!(config.dbname, "mydb");
        assert_eq!(config.application_name.as_deref(), Some("myapp"));
    }

    #[test]
    fn parse_quoted_password() {
        let config =
            Config::parse("host=127.0.0.1 user=root password='has spaces'").unwrap();
        assert_eq!(config.password, "has spaces");
    }

    #[test]
    fn parse_double_quoted_password() {
        let config =
            Config::parse("host=127.0.0.1 user=root password=\"has spaces\"").unwrap();
        assert_eq!(config.password, "has spaces");
    }
}
