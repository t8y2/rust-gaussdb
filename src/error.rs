use std::fmt;

#[derive(Debug, Clone, Default)]
pub struct DbError {
    pub severity: String,
    pub code: String,
    pub message: String,
    pub detail: Option<String>,
    pub hint: Option<String>,
    pub position: Option<String>,
    pub internal_position: Option<String>,
    pub internal_query: Option<String>,
    pub where_: Option<String>,
    pub schema_name: Option<String>,
    pub table_name: Option<String>,
    pub column_name: Option<String>,
    pub data_type_name: Option<String>,
    pub constraint_name: Option<String>,
    pub file: Option<String>,
    pub line: Option<String>,
    pub routine: Option<String>,
}

impl DbError {
    pub fn from_fields(fields: &[(u8, String)]) -> Self {
        let mut err = DbError::default();
        for &(field_type, ref value) in fields {
            match field_type {
                b'S' => err.severity = value.clone(),
                b'C' => err.code = value.clone(),
                b'M' => err.message = value.clone(),
                b'D' => err.detail = Some(value.clone()),
                b'H' => err.hint = Some(value.clone()),
                b'P' => err.position = Some(value.clone()),
                b'p' => err.internal_position = Some(value.clone()),
                b'q' => err.internal_query = Some(value.clone()),
                b'W' => err.where_ = Some(value.clone()),
                b's' => err.schema_name = Some(value.clone()),
                b't' => err.table_name = Some(value.clone()),
                b'c' => err.column_name = Some(value.clone()),
                b'd' => err.data_type_name = Some(value.clone()),
                b'n' => err.constraint_name = Some(value.clone()),
                b'F' => err.file = Some(value.clone()),
                b'L' => err.line = Some(value.clone()),
                b'R' => err.routine = Some(value.clone()),
                _ => {}
            }
        }
        err
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
        if let Some(ref table) = self.table_name {
            if let Some(ref col) = self.column_name {
                write!(f, "\n  Column: {}.{}", table, col)?;
            } else {
                write!(f, "\n  Table: {}", table)?;
            }
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
    Tls(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "IO error: {e}"),
            Error::Protocol(msg) => write!(f, "Protocol error: {msg}"),
            Error::Authentication(msg) => write!(f, "Authentication failed: {msg}"),
            Error::Database(e) => write!(f, "Database error: {e}"),
            Error::Config(msg) => write!(f, "Config error: {msg}"),
            Error::Tls(msg) => write!(f, "TLS error: {msg}"),
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
