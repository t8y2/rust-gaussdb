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
            Error::Protocol(format!(
                "column index {} out of range ({})",
                idx,
                self.values.len()
            ))
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

    pub fn values(&self) -> &[Option<Vec<u8>>] {
        &self.values
    }
}

pub trait FromSql: Sized {
    fn from_sql(raw: Option<&[u8]>) -> Result<Self, Error>;
}

pub trait ToSql {
    fn to_sql(&self) -> Vec<u8>;
    fn oid(&self) -> u32;
}

// ---- FromSql implementations ----

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
                s.parse()
                    .map_err(|e| Error::Protocol(format!("parse i32: {e}")))
            }
            None => Err(Error::Protocol("unexpected NULL".into())),
        }
    }
}

impl FromSql for Option<i32> {
    fn from_sql(raw: Option<&[u8]>) -> Result<Self, Error> {
        match raw {
            Some(_) => i32::from_sql(raw).map(Some),
            None => Ok(None),
        }
    }
}

impl FromSql for i64 {
    fn from_sql(raw: Option<&[u8]>) -> Result<Self, Error> {
        match raw {
            Some(bytes) => {
                let s = std::str::from_utf8(bytes).map_err(|e| Error::Protocol(e.to_string()))?;
                s.parse()
                    .map_err(|e| Error::Protocol(format!("parse i64: {e}")))
            }
            None => Err(Error::Protocol("unexpected NULL".into())),
        }
    }
}

impl FromSql for Option<i64> {
    fn from_sql(raw: Option<&[u8]>) -> Result<Self, Error> {
        match raw {
            Some(_) => i64::from_sql(raw).map(Some),
            None => Ok(None),
        }
    }
}

impl FromSql for f64 {
    fn from_sql(raw: Option<&[u8]>) -> Result<Self, Error> {
        match raw {
            Some(bytes) => {
                let s = std::str::from_utf8(bytes).map_err(|e| Error::Protocol(e.to_string()))?;
                s.parse()
                    .map_err(|e| Error::Protocol(format!("parse f64: {e}")))
            }
            None => Err(Error::Protocol("unexpected NULL".into())),
        }
    }
}

impl FromSql for Option<f64> {
    fn from_sql(raw: Option<&[u8]>) -> Result<Self, Error> {
        match raw {
            Some(_) => f64::from_sql(raw).map(Some),
            None => Ok(None),
        }
    }
}

impl FromSql for f32 {
    fn from_sql(raw: Option<&[u8]>) -> Result<Self, Error> {
        match raw {
            Some(bytes) => {
                let s = std::str::from_utf8(bytes).map_err(|e| Error::Protocol(e.to_string()))?;
                s.parse()
                    .map_err(|e| Error::Protocol(format!("parse f32: {e}")))
            }
            None => Err(Error::Protocol("unexpected NULL".into())),
        }
    }
}

impl FromSql for Option<f32> {
    fn from_sql(raw: Option<&[u8]>) -> Result<Self, Error> {
        match raw {
            Some(_) => f32::from_sql(raw).map(Some),
            None => Ok(None),
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

impl FromSql for Option<bool> {
    fn from_sql(raw: Option<&[u8]>) -> Result<Self, Error> {
        match raw {
            Some(_) => bool::from_sql(raw).map(Some),
            None => Ok(None),
        }
    }
}

impl FromSql for Vec<u8> {
    fn from_sql(raw: Option<&[u8]>) -> Result<Self, Error> {
        match raw {
            Some(bytes) => Ok(bytes.to_vec()),
            None => Err(Error::Protocol("unexpected NULL".into())),
        }
    }
}

// ---- ToSql implementations ----

impl ToSql for String {
    fn to_sql(&self) -> Vec<u8> {
        self.as_bytes().to_vec()
    }
    fn oid(&self) -> u32 {
        25 // TEXTOID
    }
}

impl ToSql for &str {
    fn to_sql(&self) -> Vec<u8> {
        self.as_bytes().to_vec()
    }
    fn oid(&self) -> u32 {
        25 // TEXTOID
    }
}

impl ToSql for i32 {
    fn to_sql(&self) -> Vec<u8> {
        self.to_string().into_bytes()
    }
    fn oid(&self) -> u32 {
        23 // INT4OID
    }
}

impl ToSql for i64 {
    fn to_sql(&self) -> Vec<u8> {
        self.to_string().into_bytes()
    }
    fn oid(&self) -> u32 {
        20 // INT8OID
    }
}

impl ToSql for f64 {
    fn to_sql(&self) -> Vec<u8> {
        self.to_string().into_bytes()
    }
    fn oid(&self) -> u32 {
        701 // FLOAT8OID
    }
}

impl ToSql for f32 {
    fn to_sql(&self) -> Vec<u8> {
        self.to_string().into_bytes()
    }
    fn oid(&self) -> u32 {
        700 // FLOAT4OID
    }
}

impl ToSql for bool {
    fn to_sql(&self) -> Vec<u8> {
        (if *self { "t" } else { "f" }).as_bytes().to_vec()
    }
    fn oid(&self) -> u32 {
        16 // BOOLOID
    }
}

impl<T: ToSql> ToSql for Option<T> {
    fn to_sql(&self) -> Vec<u8> {
        match self {
            Some(v) => v.to_sql(),
            None => Vec::new(),
        }
    }
    fn oid(&self) -> u32 {
        match self {
            Some(v) => v.oid(),
            None => 0,
        }
    }
}

impl ToSql for Vec<u8> {
    fn to_sql(&self) -> Vec<u8> {
        self.clone()
    }
    fn oid(&self) -> u32 {
        17 // BYTEAOID
    }
}

// ---- Type OID constants ----

pub struct PgType;

#[allow(dead_code)]
impl PgType {
    pub const BOOL: u32 = 16;
    pub const BYTEA: u32 = 17;
    pub const INT8: u32 = 20;
    pub const INT2: u32 = 21;
    pub const INT4: u32 = 23;
    pub const TEXT: u32 = 25;
    pub const OID: u32 = 26;
    pub const FLOAT4: u32 = 700;
    pub const FLOAT8: u32 = 701;
    pub const NUMERIC: u32 = 1700;
    pub const VARCHAR: u32 = 1043;
    pub const DATE: u32 = 1082;
    pub const TIME: u32 = 1083;
    pub const TIMESTAMP: u32 = 1114;
    pub const TIMESTAMPTZ: u32 = 1184;
}
