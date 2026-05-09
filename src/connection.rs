use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio_util::codec::Framed;

use crate::auth::{self, GaussAuthState, GAUSSDB_MD5_SHA256, GAUSSDB_SHA256};
use crate::codec::*;
use crate::config::Config;
use crate::error::Error;

pub struct Connection {
    pub(crate) framed: Framed<TcpStream, PgCodec>,
    pub(crate) process_id: i32,
    pub(crate) secret_key: i32,
    pub(crate) parameters: Vec<(String, String)>,
}

impl Connection {
    pub async fn connect(config: &Config) -> Result<Self, Error> {
        let addr = format!("{}:{}", config.host, config.port);

        let stream = match &config.connect_timeout {
            Some(timeout) => tokio::time::timeout(*timeout, TcpStream::connect(&addr))
                .await
                .map_err(|_| {
                    Error::Io(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "connection timed out",
                    ))
                })??,
            None => TcpStream::connect(&addr).await?,
        };

        stream.set_nodelay(true)?;
        let mut framed = Framed::new(stream, PgCodec);

        let startup = build_startup_message(
            &config.user,
            &config.dbname,
            config.application_name.as_deref(),
        );
        framed.send(FrontendMessage(startup)).await?;

        let mut process_id = 0i32;
        let mut secret_key = 0i32;
        let mut parameters = Vec::new();

        loop {
            let msg = framed
                .next()
                .await
                .ok_or_else(|| Error::Protocol("connection closed during startup".into()))??;

            match msg {
                BackendMessage::AuthenticationOk => {}
                BackendMessage::AuthenticationCleartextPassword => {
                    let pw = build_password_message(config.password.as_bytes());
                    framed.send(FrontendMessage(pw)).await?;
                }
                BackendMessage::AuthenticationMd5Password { salt } => {
                    let hash = md5_password(&config.user, &config.password, &salt);
                    let pw = build_password_message(hash.as_bytes());
                    framed.send(FrontendMessage(pw)).await?;
                }
                BackendMessage::AuthenticationSasl { mechanisms } => {
                    authenticate_sasl(&mut framed, config, &mechanisms).await?;
                }
                BackendMessage::AuthenticationGaussdbSha256 {
                    random64code,
                    token,
                    server_signature,
                    server_iteration,
                } => {
                    let proof = auth::rfc5802_sha256(
                        &config.password,
                        &random64code,
                        &token,
                        server_signature.as_deref(),
                        server_iteration,
                    )
                    .map_err(Error::Authentication)?;
                    let msg = build_password_message(proof.as_bytes());
                    framed.send(FrontendMessage(msg)).await?;
                }
                BackendMessage::ParameterStatus { name, value } => {
                    parameters.push((name, value));
                }
                BackendMessage::BackendKeyData {
                    process_id: pid,
                    secret_key: sk,
                } => {
                    process_id = pid;
                    secret_key = sk;
                }
                BackendMessage::ReadyForQuery => break,
                BackendMessage::ErrorResponse { fields } => {
                    let msg = fields
                        .iter()
                        .find(|(t, _)| *t == b'M')
                        .map(|(_, v)| v.clone())
                        .unwrap_or_else(|| "unknown error".to_string());
                    return Err(Error::Database(msg));
                }
                BackendMessage::NoticeResponse => {}
                _ => {}
            }
        }

        Ok(Connection {
            framed,
            process_id,
            secret_key,
            parameters,
        })
    }
}

async fn authenticate_sasl(
    framed: &mut Framed<TcpStream, PgCodec>,
    config: &Config,
    mechanisms: &[String],
) -> Result<(), Error> {
    let mechanism = if mechanisms.iter().any(|m| m == "SCRAM-SHA-256") {
        "SCRAM-SHA-256"
    } else if mechanisms.iter().any(|m| m == GAUSSDB_SHA256) {
        GAUSSDB_SHA256
    } else if mechanisms.iter().any(|m| m == GAUSSDB_MD5_SHA256) {
        GAUSSDB_MD5_SHA256
    } else {
        return Err(Error::Authentication(format!(
            "unsupported SASL mechanisms: {}",
            mechanisms.join(", ")
        )));
    };

    log::debug!("Using SASL mechanism: {mechanism}");

    if mechanism == "SCRAM-SHA-256" {
        authenticate_scram_sha256(framed, config).await
    } else {
        authenticate_gaussdb_sasl(framed, config, mechanism).await
    }
}

async fn authenticate_scram_sha256(
    framed: &mut Framed<TcpStream, PgCodec>,
    config: &Config,
) -> Result<(), Error> {
    let (mut state, initial_data) =
        GaussAuthState::new(&config.user, config.password.as_bytes(), "SCRAM-SHA-256");
    let msg = build_sasl_initial_response("SCRAM-SHA-256", &initial_data);
    framed.send(FrontendMessage(msg)).await?;

    let server_first = match framed.next().await {
        Some(Ok(BackendMessage::AuthenticationSaslContinue { data })) => data,
        Some(Ok(BackendMessage::ErrorResponse { fields })) => {
            let msg = extract_error(&fields);
            return Err(Error::Authentication(msg));
        }
        _ => return Err(Error::Protocol("expected SaslContinue".into())),
    };

    let response = state
        .process_server_first(&server_first)
        .map_err(Error::Authentication)?;
    let msg = build_sasl_response(&response);
    framed.send(FrontendMessage(msg)).await?;

    match framed.next().await {
        Some(Ok(BackendMessage::AuthenticationSaslFinal { data })) => {
            state
                .process_server_final(&data)
                .map_err(Error::Authentication)?;
        }
        Some(Ok(BackendMessage::ErrorResponse { fields })) => {
            let msg = extract_error(&fields);
            return Err(Error::Authentication(msg));
        }
        _ => return Err(Error::Protocol("expected SaslFinal".into())),
    }

    Ok(())
}

async fn authenticate_gaussdb_sasl(
    framed: &mut Framed<TcpStream, PgCodec>,
    config: &Config,
    mechanism: &str,
) -> Result<(), Error> {
    let (mut state, initial_data) =
        GaussAuthState::new(&config.user, config.password.as_bytes(), mechanism);
    let msg = build_sasl_initial_response("SCRAM-SHA-256", &initial_data);
    framed.send(FrontendMessage(msg)).await?;

    let server_first = match framed.next().await {
        Some(Ok(BackendMessage::AuthenticationSaslContinue { data })) => data,
        Some(Ok(BackendMessage::ErrorResponse { fields })) => {
            let msg = extract_error(&fields);
            return Err(Error::Authentication(msg));
        }
        _ => return Err(Error::Protocol("expected SaslContinue".into())),
    };

    let response = state
        .process_server_first(&server_first)
        .map_err(Error::Authentication)?;
    let msg = build_sasl_response(&response);
    framed.send(FrontendMessage(msg)).await?;

    match framed.next().await {
        Some(Ok(BackendMessage::AuthenticationSaslFinal { data })) => {
            state
                .process_server_final(&data)
                .map_err(Error::Authentication)?;
        }
        Some(Ok(BackendMessage::AuthenticationOk)) => {}
        Some(Ok(BackendMessage::ErrorResponse { fields })) => {
            let msg = extract_error(&fields);
            return Err(Error::Authentication(msg));
        }
        _ => {}
    }

    Ok(())
}

fn md5_password(user: &str, password: &str, salt: &[u8; 4]) -> String {
    use md5::{Digest, Md5};
    let mut hasher = Md5::new();
    hasher.update(password.as_bytes());
    hasher.update(user.as_bytes());
    let first = format!("{:x}", hasher.finalize());

    let mut hasher = Md5::new();
    hasher.update(first.as_bytes());
    hasher.update(salt);
    format!("md5{:x}", hasher.finalize())
}

fn extract_error(fields: &[(u8, String)]) -> String {
    fields
        .iter()
        .find(|(t, _)| *t == b'M')
        .map(|(_, v)| v.clone())
        .unwrap_or_else(|| "unknown error".to_string())
}
