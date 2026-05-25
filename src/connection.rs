use std::pin::Pin;

use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio_util::codec::Framed;

use crate::auth::{self, GaussAuthState, GAUSSDB_MD5_SHA256, GAUSSDB_SHA256};
use crate::codec::*;
use crate::config::{Config, SslMode};
use crate::error::Error;

#[derive(Debug, Clone)]
pub struct Notification {
    pub pid: i32,
    pub channel: String,
    pub payload: String,
}

pub(crate) trait AsyncReadWrite: AsyncRead + AsyncWrite + Send + Unpin {}
impl<T: AsyncRead + AsyncWrite + Send + Unpin> AsyncReadWrite for T {}

type BoxedStream = Pin<Box<dyn AsyncReadWrite>>;

pub struct Connection {
    pub(crate) framed: Framed<BoxedStream, PgCodec>,
    #[allow(dead_code)]
    pub(crate) process_id: i32,
    #[allow(dead_code)]
    pub(crate) secret_key: i32,
    pub(crate) parameters: Vec<(String, String)>,
    pub(crate) notification_tx: tokio::sync::broadcast::Sender<Notification>,
}

impl Connection {
    pub fn subscribe_notifications(&self) -> tokio::sync::broadcast::Receiver<Notification> {
        self.notification_tx.subscribe()
    }
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

        let stream: BoxedStream = if config.ssl_mode == SslMode::Disable {
            Box::pin(stream)
        } else {
            tls_handshake(stream, config).await?
        };

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
                BackendMessage::ErrorResponse { error } => {
                    return Err(Error::Database(error));
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
            notification_tx: tokio::sync::broadcast::channel(16).0,
        })
    }
}

async fn tls_handshake(stream: TcpStream, config: &Config) -> Result<BoxedStream, Error> {
    // Send SSLRequest (int32: 8, int32: 80877103)
    let mut stream = stream;
    let ssl_request = {
        let mut buf = Vec::with_capacity(8);
        buf.extend_from_slice(&8i32.to_be_bytes());
        buf.extend_from_slice(&80877103i32.to_be_bytes());
        buf
    };
    tokio::io::AsyncWriteExt::write_all(&mut stream, &ssl_request).await?;

    // Read server response: 'S' = yes, 'N' = no
    let mut response = [0u8; 1];
    tokio::io::AsyncReadExt::read_exact(&mut stream, &mut response).await?;

    match response[0] {
        b'S' => {
            #[cfg(feature = "tls")]
            {
                let mut builder = native_tls::TlsConnector::builder();

                let verify = matches!(config.ssl_mode, SslMode::VerifyCa | SslMode::VerifyFull);
                if !verify {
                    builder.danger_accept_invalid_certs(true);
                }

                // Load custom CA certificate if provided
                if let Some(ref root_cert_path) = config.ssl_root_cert {
                    let cert_data = std::fs::read(root_cert_path)
                        .map_err(|e| Error::Tls(format!("failed to read sslrootcert: {}", e)))?;
                    let cert = native_tls::Certificate::from_pem(&cert_data)
                        .or_else(|_| native_tls::Certificate::from_der(&cert_data))
                        .map_err(|e| Error::Tls(format!("failed to parse sslrootcert: {}", e)))?;
                    builder.add_root_certificate(cert);
                }

                // Load client certificate if provided
                if let Some(ref ssl_cert_path) = config.ssl_cert {
                    let cert_data = std::fs::read(ssl_cert_path)
                        .map_err(|e| Error::Tls(format!("failed to read sslcert: {}", e)))?;
                    let identity = if let Some(ref ssl_key_path) = config.ssl_key {
                        let key_data = std::fs::read(ssl_key_path)
                            .map_err(|e| Error::Tls(format!("failed to read sslkey: {}", e)))?;
                        native_tls::Identity::from_pkcs8(&cert_data, &key_data)
                            .or_else(|_| native_tls::Identity::from_pkcs12(&cert_data, ""))
                            .map_err(|e| Error::Tls(format!("failed to load client cert: {}", e)))?
                    } else {
                        native_tls::Identity::from_pkcs12(&cert_data, "")
                            .or_else(|_| native_tls::Identity::from_pkcs8(&cert_data, b""))
                            .map_err(|e| Error::Tls(format!("failed to load client cert: {}", e)))?
                    };
                    builder.identity(identity);
                }

                let tls_connector = builder.build().map_err(|e| Error::Tls(e.to_string()))?;
                let tls_connector: tokio_native_tls::TlsConnector = tls_connector.into();
                let domain = config.host.clone();
                let tls_stream = tls_connector
                    .connect(&domain, stream)
                    .await
                    .map_err(|e| Error::Tls(e.to_string()))?;
                Ok(Box::pin(tls_stream))
            }
            #[cfg(not(feature = "tls"))]
            {
                Err(Error::Config(
                    "TLS required but gaussdb-rs was built without TLS support (enable 'tls' feature)"
                        .into(),
                ))
            }
        }
        b'N' => {
            if config.ssl_mode == SslMode::Require {
                Err(Error::Config("TLS required by sslmode=require but server refused".into()))
            } else {
                Ok(Box::pin(stream))
            }
        }
        c => Err(Error::Protocol(format!(
            "unexpected SSL response: {}",
            c
        ))),
    }
}

async fn authenticate_sasl(
    framed: &mut Framed<BoxedStream, PgCodec>,
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
    framed: &mut Framed<BoxedStream, PgCodec>,
    config: &Config,
) -> Result<(), Error> {
    let (mut state, initial_data) =
        GaussAuthState::new(&config.user, config.password.as_bytes(), "SCRAM-SHA-256");
    let msg = build_sasl_initial_response("SCRAM-SHA-256", &initial_data);
    framed.send(FrontendMessage(msg)).await?;

    let server_first = match framed.next().await {
        Some(Ok(BackendMessage::AuthenticationSaslContinue { data })) => data,
        Some(Ok(BackendMessage::ErrorResponse { error })) => {
            return Err(Error::Authentication(error.message));
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
        Some(Ok(BackendMessage::ErrorResponse { error })) => {
            return Err(Error::Authentication(error.message));
        }
        _ => return Err(Error::Protocol("expected SaslFinal".into())),
    }

    Ok(())
}

async fn authenticate_gaussdb_sasl(
    framed: &mut Framed<BoxedStream, PgCodec>,
    config: &Config,
    mechanism: &str,
) -> Result<(), Error> {
    let (mut state, initial_data) =
        GaussAuthState::new(&config.user, config.password.as_bytes(), mechanism);
    let msg = build_sasl_initial_response("SCRAM-SHA-256", &initial_data);
    framed.send(FrontendMessage(msg)).await?;

    let server_first = match framed.next().await {
        Some(Ok(BackendMessage::AuthenticationSaslContinue { data })) => data,
        Some(Ok(BackendMessage::ErrorResponse { error })) => {
            return Err(Error::Authentication(error.message));
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
        Some(Ok(BackendMessage::ErrorResponse { error })) => {
            return Err(Error::Authentication(error.message));
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
