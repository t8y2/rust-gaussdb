use crate::codec::ColumnDescription;
use crate::error::Error;

#[derive(Debug)]
pub struct Row {
    columns: Vec<ColumnDescription>,
    values: Vec<Option<Vec<u8>>>,
}

impl Row {
    pub(crate) fn new(columns: Vec<ColumnDescription>, values: Vec<Option<Vec<u8>>>) -> Self {
        Row { columns, values }
    }

    pub fn columns(&self) -> &[ColumnDescription] {
        &self.columns
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub fn get<T: FromSql>(&self, idx: usize) -> Result<T, Error> {
        let raw = self.values.get(idx).ok_or_else(|| {
            Error::Protocol(format!("column index {} out of range ({})", idx, self.values.len()))
        })?;
        T::from_sql(raw.as_deref())
    }

    pub fn get_by_name<T: FromSql>(&self, name: &str) -> Result<T, Error> {
        let idx = self
            .columns
            .iter()
            .position(|c| c.name == name)
            .ok_or_else(|| Error::Protocol(format!("column '{}' not found", name)))?;
        self.get(idx)
    }

    pub fn try_get<T: FromSql>(&self, idx: usize) -> Option<T> {
        self.get(idx).ok()
    }
}

pub trait FromSql: Sized {
    fn from_sql(raw: Option<&[u8]>) -> Result<Self, Error>;
}

impl FromSql for String {
    fn from_sql(raw: Option<&[u8]>) -> Result<Self, Error> {
        match raw {
            Some(bytes) => String::from_utf8(bytes.to_vec())
                .map_err(|e| Error::Protocol(format!("invalid UTF-8: {e}"))),
            None => Err(Error::Protocol("unexpected NULL".into())),
        }
    }
}

impl FromSql for Option<String> {
    fn from_sql(raw: Option<&[u8]>) -> Result<Self, Error> {
        match raw {
            Some(bytes) => Ok(Some(
                String::from_utf8(bytes.to_vec())
                    .map_err(|e| Error::Protocol(format!("invalid UTF-8: {e}")))?,
            )),
            None => Ok(None),
        }
    }
}

impl FromSql for i32 {
    fn from_sql(raw: Option<&[u8]>) -> Result<Self, Error> {
        match raw {
            Some(bytes) => {
                let s = std::str::from_utf8(bytes).map_err(|e| Error::Protocol(e.to_string()))?;
                s.parse().map_err(|e| Error::Protocol(format!("parse i32: {e}")))
            }
            None => Err(Error::Protocol("unexpected NULL".into())),
        }
    }
}

impl FromSql for i64 {
    fn from_sql(raw: Option<&[u8]>) -> Result<Self, Error> {
        match raw {
            Some(bytes) => {
                let s = std::str::from_utf8(bytes).map_err(|e| Error::Protocol(e.to_string()))?;
                s.parse().map_err(|e| Error::Protocol(format!("parse i64: {e}")))
            }
            None => Err(Error::Protocol("unexpected NULL".into())),
        }
    }
}

impl FromSql for f64 {
    fn from_sql(raw: Option<&[u8]>) -> Result<Self, Error> {
        match raw {
            Some(bytes) => {
                let s = std::str::from_utf8(bytes).map_err(|e| Error::Protocol(e.to_string()))?;
                s.parse().map_err(|e| Error::Protocol(format!("parse f64: {e}")))
            }
            None => Err(Error::Protocol("unexpected NULL".into())),
        }
    }
}

impl FromSql for bool {
    fn from_sql(raw: Option<&[u8]>) -> Result<Self, Error> {
        match raw {
            Some(bytes) => match bytes {
                b"t" | b"true" | b"1" => Ok(true),
                b"f" | b"false" | b"0" => Ok(false),
                _ => Err(Error::Protocol("invalid bool".into())),
            },
            None => Err(Error::Protocol("unexpected NULL".into())),
        }
    }
}
