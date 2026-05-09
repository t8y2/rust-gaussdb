use bytes::{Buf, BufMut, BytesMut};
use std::io;
use tokio_util::codec::{Decoder, Encoder};

pub struct PgCodec;

pub enum BackendMessage {
    AuthenticationOk,
    AuthenticationCleartextPassword,
    AuthenticationMd5Password {
        salt: [u8; 4],
    },
    AuthenticationSasl {
        mechanisms: Vec<String>,
    },
    AuthenticationSaslContinue {
        data: Vec<u8>,
    },
    AuthenticationSaslFinal {
        data: Vec<u8>,
    },
    AuthenticationGaussdbSha256 {
        random64code: String,
        token: String,
        server_signature: Option<String>,
        server_iteration: u32,
    },
    ParameterStatus {
        name: String,
        value: String,
    },
    BackendKeyData {
        process_id: i32,
        secret_key: i32,
    },
    ReadyForQuery,
    RowDescription {
        columns: Vec<ColumnDescription>,
    },
    DataRow {
        values: Vec<Option<Vec<u8>>>,
    },
    CommandComplete {
        tag: String,
    },
    ErrorResponse {
        fields: Vec<(u8, String)>,
    },
    NoticeResponse,
    EmptyQueryResponse,
    ParseComplete,
    BindComplete,
    CloseComplete,
    NoData,
}

#[derive(Debug, Clone)]
pub struct ColumnDescription {
    pub name: String,
    pub type_oid: u32,
    pub type_size: i16,
    pub type_modifier: i32,
    pub format: i16,
}

impl Decoder for PgCodec {
    type Item = BackendMessage;
    type Error = io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if src.len() < 5 {
            return Ok(None);
        }

        let msg_type = src[0];
        let len = u32::from_be_bytes([src[1], src[2], src[3], src[4]]) as usize;

        if src.len() < 1 + len {
            return Ok(None);
        }

        src.advance(1);
        let mut body = src.split_to(len);
        body.advance(4);

        match msg_type {
            b'R' => decode_auth(&mut body),
            b'S' => {
                let name = read_cstr(&mut body)?;
                let value = read_cstr(&mut body)?;
                Ok(Some(BackendMessage::ParameterStatus { name, value }))
            }
            b'K' => {
                let process_id = body.get_i32();
                let secret_key = body.get_i32();
                Ok(Some(BackendMessage::BackendKeyData {
                    process_id,
                    secret_key,
                }))
            }
            b'Z' => Ok(Some(BackendMessage::ReadyForQuery)),
            b'T' => decode_row_description(&mut body),
            b'D' => decode_data_row(&mut body),
            b'C' => {
                let tag = read_cstr(&mut body)?;
                Ok(Some(BackendMessage::CommandComplete { tag }))
            }
            b'E' => {
                let mut fields = Vec::new();
                loop {
                    let field_type = body.get_u8();
                    if field_type == 0 {
                        break;
                    }
                    let value = read_cstr(&mut body)?;
                    fields.push((field_type, value));
                }
                Ok(Some(BackendMessage::ErrorResponse { fields }))
            }
            b'N' => Ok(Some(BackendMessage::NoticeResponse)),
            b'I' => Ok(Some(BackendMessage::EmptyQueryResponse)),
            b'1' => Ok(Some(BackendMessage::ParseComplete)),
            b'2' => Ok(Some(BackendMessage::BindComplete)),
            b'3' => Ok(Some(BackendMessage::CloseComplete)),
            b'n' => Ok(Some(BackendMessage::NoData)),
            _ => Ok(None),
        }
    }
}

fn decode_auth(body: &mut BytesMut) -> Result<Option<BackendMessage>, io::Error> {
    let auth_type = body.get_i32();
    match auth_type {
        0 => Ok(Some(BackendMessage::AuthenticationOk)),
        3 => Ok(Some(BackendMessage::AuthenticationCleartextPassword)),
        5 => {
            let mut salt = [0u8; 4];
            salt.copy_from_slice(&body[..4]);
            Ok(Some(BackendMessage::AuthenticationMd5Password { salt }))
        }
        10 => {
            if body.len() >= 4 && body[0] == 0 {
                let password_stored_method = body.get_i32();
                let remaining = body.len();
                if (password_stored_method == 2 || password_stored_method == 0) && remaining >= 72 {
                    let random64code = String::from_utf8_lossy(&body.split_to(64)).to_string();
                    let token = String::from_utf8_lossy(&body.split_to(8)).to_string();
                    let (server_signature, server_iteration) = if body.len() >= 64
                        && body.len() < 68
                    {
                        // Old protocol: random64code(64) + token(8) + server_signature(64), iteration=10000
                        let sig = String::from_utf8_lossy(&body.split_to(body.len())).to_string();
                        (Some(sig), 10000u32)
                    } else if body.len() == 4 {
                        // New protocol (>=351): random64code(64) + token(8) + server_iteration(4)
                        (None, body.get_i32() as u32)
                    } else if body.len() >= 68 {
                        // New protocol with signature: random64code(64) + token(8) + server_signature(64) + server_iteration(4)
                        let sig = String::from_utf8_lossy(&body.split_to(64)).to_string();
                        let iter = body.get_i32() as u32;
                        (Some(sig), iter)
                    } else {
                        (None, 10000)
                    };
                    log::debug!("GaussDB SHA256 auth: iteration={server_iteration}");
                    Ok(Some(BackendMessage::AuthenticationGaussdbSha256 {
                        random64code,
                        token,
                        server_signature,
                        server_iteration,
                    }))
                } else {
                    let mechanism = match password_stored_method {
                        2 => "SHA256".to_string(),
                        3 => "MD5_SHA256".to_string(),
                        other => format!("unknown({other})"),
                    };
                    Ok(Some(BackendMessage::AuthenticationSasl {
                        mechanisms: vec![mechanism],
                    }))
                }
            } else {
                let mut mechanisms = Vec::new();
                loop {
                    let mech = read_cstr(body)?;
                    if mech.is_empty() {
                        break;
                    }
                    mechanisms.push(mech);
                }
                Ok(Some(BackendMessage::AuthenticationSasl { mechanisms }))
            }
        }
        11 => {
            let data = body.to_vec();
            Ok(Some(BackendMessage::AuthenticationSaslContinue { data }))
        }
        12 => {
            let data = body.to_vec();
            Ok(Some(BackendMessage::AuthenticationSaslFinal { data }))
        }
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unsupported auth type: {auth_type}"),
        )),
    }
}

fn decode_row_description(body: &mut BytesMut) -> Result<Option<BackendMessage>, io::Error> {
    let count = body.get_i16() as usize;
    let mut columns = Vec::with_capacity(count);
    for _ in 0..count {
        let name = read_cstr(body)?;
        let _table_oid = body.get_i32();
        let _attr_num = body.get_i16();
        let type_oid = body.get_u32();
        let type_size = body.get_i16();
        let type_modifier = body.get_i32();
        let format = body.get_i16();
        columns.push(ColumnDescription {
            name,
            type_oid,
            type_size,
            type_modifier,
            format,
        });
    }
    Ok(Some(BackendMessage::RowDescription { columns }))
}

fn decode_data_row(body: &mut BytesMut) -> Result<Option<BackendMessage>, io::Error> {
    let count = body.get_i16() as usize;
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        let len = body.get_i32();
        if len < 0 {
            values.push(None);
        } else {
            let data = body.split_to(len as usize).to_vec();
            values.push(Some(data));
        }
    }
    Ok(Some(BackendMessage::DataRow { values }))
}

fn read_cstr(buf: &mut BytesMut) -> Result<String, io::Error> {
    if let Some(pos) = buf.iter().position(|&b| b == 0) {
        let s = String::from_utf8_lossy(&buf[..pos]).to_string();
        buf.advance(pos + 1);
        Ok(s)
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "missing null terminator",
        ))
    }
}

pub struct FrontendMessage(pub BytesMut);

impl Encoder<FrontendMessage> for PgCodec {
    type Error = io::Error;

    fn encode(&mut self, item: FrontendMessage, dst: &mut BytesMut) -> Result<(), Self::Error> {
        dst.extend_from_slice(&item.0);
        Ok(())
    }
}

pub fn build_startup_message(
    user: &str,
    database: &str,
    application_name: Option<&str>,
) -> BytesMut {
    let mut params = BytesMut::new();

    params.extend_from_slice(b"user\0");
    params.extend_from_slice(user.as_bytes());
    params.put_u8(0);

    if !database.is_empty() {
        params.extend_from_slice(b"database\0");
        params.extend_from_slice(database.as_bytes());
        params.put_u8(0);
    }

    params.extend_from_slice(b"client_encoding\0UTF8\0");

    if let Some(app) = application_name {
        params.extend_from_slice(b"application_name\0");
        params.extend_from_slice(app.as_bytes());
        params.put_u8(0);
    }

    params.put_u8(0);

    let len = 4 + 4 + params.len();
    let mut buf = BytesMut::with_capacity(len);
    buf.put_i32(len as i32);
    buf.put_i32(196608); // protocol version 3.0
    buf.extend_from_slice(&params);
    buf
}

pub fn build_password_message(password: &[u8]) -> BytesMut {
    let len = 4 + password.len() + 1;
    let mut buf = BytesMut::with_capacity(1 + len);
    buf.put_u8(b'p');
    buf.put_i32(len as i32);
    buf.extend_from_slice(password);
    buf.put_u8(0);
    buf
}

pub fn build_sasl_initial_response(mechanism: &str, data: &[u8]) -> BytesMut {
    let len = 4 + mechanism.len() + 1 + 4 + data.len();
    let mut buf = BytesMut::with_capacity(1 + len);
    buf.put_u8(b'p');
    buf.put_i32(len as i32);
    buf.extend_from_slice(mechanism.as_bytes());
    buf.put_u8(0);
    buf.put_i32(data.len() as i32);
    buf.extend_from_slice(data);
    buf
}

pub fn build_sasl_response(data: &[u8]) -> BytesMut {
    let len = 4 + data.len();
    let mut buf = BytesMut::with_capacity(1 + len);
    buf.put_u8(b'p');
    buf.put_i32(len as i32);
    buf.extend_from_slice(data);
    buf
}

pub fn build_query_message(sql: &str) -> BytesMut {
    let len = 4 + sql.len() + 1;
    let mut buf = BytesMut::with_capacity(1 + len);
    buf.put_u8(b'Q');
    buf.put_i32(len as i32);
    buf.extend_from_slice(sql.as_bytes());
    buf.put_u8(0);
    buf
}

pub fn build_terminate_message() -> BytesMut {
    let mut buf = BytesMut::with_capacity(5);
    buf.put_u8(b'X');
    buf.put_i32(4);
    buf
}
