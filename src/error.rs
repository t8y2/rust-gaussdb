use std::fmt;

#[derive(Debug, Clone)]
pub struct DbError {
    pub severity: String,
    pub code: String,
    pub message: String,
    pub detail: Option<String>,
    pub hint: Option<String>,
    pub position: Option<String>,
}

impl DbError {
    pub fn from_fields(fields: &[(u8, String)]) -> Self {
        let mut severity = String::new();
        let mut code = String::new();
        let mut message = String::new();
        let mut detail = None;
        let mut hint = None;
        let mut position = None;

        for &(field_type, ref value) in fields {
            match field_type {
                b'S' => severity = value.clone(),
                b'C' => code = value.clone(),
                b'M' => message = value.clone(),
                b'D' => detail = Some(value.clone()),
                b'H' => hint = Some(value.clone()),
                b'P' => position = Some(value.clone()),
                _ => {}
            }
        }

        DbError {
            severity,
            code,
            message,
            detail,
            hint,
            position,
        }
    }
}

impl fmt::Display for DbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)?;
        if let Some(ref detail) = self.detail {
            write!(f, "\n  Detail: {}", detail)?;
        }
        if let Some(ref hint) = self.hint {
            write!(f, "\n  Hint: {}", hint)?;
        }
        Ok(())
    }
}

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Protocol(String),
    Authentication(String),
    Database(DbError),
    Config(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "IO error: {e}"),
            Error::Protocol(msg) => write!(f, "Protocol error: {msg}"),
            Error::Authentication(msg) => write!(f, "Authentication failed: {msg}"),
            Error::Database(e) => write!(f, "Database error: {e}"),
            Error::Config(msg) => write!(f, "Config error: {msg}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}
