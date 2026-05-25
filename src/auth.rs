//! GaussDB SHA256 SASL authentication implementation.

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use hmac::{Hmac, Mac};
use rand::Rng;
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

const NONCE_LENGTH: usize = 24;

pub const GAUSSDB_SHA256: &str = "SHA256";
pub const GAUSSDB_MD5_SHA256: &str = "MD5_SHA256";

fn generate_nonce() -> String {
    let mut rng = rand::rng();
    (0..NONCE_LENGTH)
        .map(|_| {
            let mut v = rng.random_range(0x21u8..0x7e);
            if v == 0x2c {
                v = 0x7e;
            }
            v as char
        })
        .collect()
}

fn hi(password: &[u8], salt: &[u8], iterations: u32) -> [u8; 32] {
    let mut mac = HmacSha256::new_from_slice(password).expect("HMAC key");
    mac.update(salt);
    mac.update(&1u32.to_be_bytes());
    let mut prev = mac.finalize().into_bytes();
    let mut result = prev;

    for _ in 1..iterations {
        let mut mac = HmacSha256::new_from_slice(password).expect("HMAC key");
        mac.update(&prev);
        prev = mac.finalize().into_bytes();
        for (a, b) in result.iter_mut().zip(prev.iter()) {
            *a ^= b;
        }
    }

    result.into()
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC key");
    mac.update(data);
    mac.finalize().into_bytes().into()
}

fn sha256(data: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().into()
}

fn md5_sha256_password(user: &str, password: &str) -> String {
    use md5::Digest as Md5Digest;
    let mut hasher = md5::Md5::new();
    hasher.update(password.as_bytes());
    hasher.update(user.as_bytes());
    let md5_result = hasher.finalize();
    let md5_hex = format!("md5{:x}", md5_result);

    let sha256_result = sha256(md5_hex.as_bytes());
    hex::encode(sha256_result)
}

pub enum GaussAuthState {
    WaitingForServerFirst {
        password: Vec<u8>,
        user: String,
        mechanism: String,
        client_nonce: String,
        client_first_bare: String,
    },
    WaitingForServerFinal {
        server_signature: Vec<u8>,
    },
    Done,
}

impl GaussAuthState {
    pub fn new(user: &str, password: &[u8], mechanism: &str) -> (Self, Vec<u8>) {
        let client_nonce = generate_nonce();
        let client_first_bare = format!("n=,r={}", client_nonce);
        let gs2_header = "n,,";
        let client_first_message = format!("{}{}", gs2_header, client_first_bare);

        let state = GaussAuthState::WaitingForServerFirst {
            password: password.to_vec(),
            user: user.to_string(),
            mechanism: mechanism.to_string(),
            client_nonce: client_nonce.clone(),
            client_first_bare,
        };

        (state, client_first_message.into_bytes())
    }

    pub fn process_server_first(&mut self, server_first: &[u8]) -> Result<Vec<u8>, String> {
        let state = std::mem::replace(self, GaussAuthState::Done);

        match state {
            GaussAuthState::WaitingForServerFirst {
                password,
                user,
                mechanism,
                client_nonce,
                client_first_bare,
            } => {
                let server_first_str =
                    std::str::from_utf8(server_first).map_err(|e| format!("Invalid UTF-8: {e}"))?;

                let mut server_nonce = None;
                let mut salt_b64 = None;
                let mut iterations = None;

                for part in server_first_str.split(',') {
                    if let Some(val) = part.strip_prefix("r=") {
                        server_nonce = Some(val.to_string());
                    } else if let Some(val) = part.strip_prefix("s=") {
                        salt_b64 = Some(val.to_string());
                    } else if let Some(val) = part.strip_prefix("i=") {
                        iterations = Some(
                            val.parse::<u32>()
                                .map_err(|e| format!("Bad iteration count: {e}"))?,
                        );
                    }
                }

                let server_nonce = server_nonce.ok_or("Missing server nonce")?;
                let salt_b64 = salt_b64.ok_or("Missing salt")?;
                let iterations = iterations.ok_or("Missing iterations")?;

                if !server_nonce.starts_with(&client_nonce) {
                    return Err("Server nonce doesn't start with client nonce".to_string());
                }

                let salt = STANDARD
                    .decode(&salt_b64)
                    .map_err(|e| format!("Bad salt base64: {e}"))?;

                let salted_password = if mechanism == GAUSSDB_MD5_SHA256 {
                    let pwd_str = std::str::from_utf8(&password).unwrap_or("");
                    let md5_sha = md5_sha256_password(&user, pwd_str);
                    hi(md5_sha.as_bytes(), &salt, iterations)
                } else if mechanism == "SCRAM-SHA-256" {
                    // Standard SCRAM: use raw password bytes (with SASLprep)
                    hi(&password, &salt, iterations)
                } else {
                    // GaussDB SHA256: pre-hash password with SHA256, then hex-encode
                    let pwd_sha = sha256(&password);
                    let pwd_hex = hex::encode(pwd_sha);
                    hi(pwd_hex.as_bytes(), &salt, iterations)
                };

                let client_key = hmac_sha256(&salted_password, b"Client Key");
                let stored_key = sha256(&client_key);

                let channel_binding = "c=biws";
                let client_final_without_proof = format!("{},r={}", channel_binding, server_nonce);
                let auth_message = format!(
                    "{},{},{}",
                    client_first_bare, server_first_str, client_final_without_proof
                );

                let client_signature = hmac_sha256(&stored_key, auth_message.as_bytes());
                let mut proof = client_key;
                for (a, b) in proof.iter_mut().zip(client_signature.iter()) {
                    *a ^= b;
                }
                let proof_b64 = STANDARD.encode(proof);

                let server_key = hmac_sha256(&salted_password, b"Server Key");
                let server_signature = hmac_sha256(&server_key, auth_message.as_bytes());

                let client_final = format!("{},p={}", client_final_without_proof, proof_b64);

                *self = GaussAuthState::WaitingForServerFinal {
                    server_signature: server_signature.to_vec(),
                };

                Ok(client_final.into_bytes())
            }
            _ => Err("Unexpected state".to_string()),
        }
    }

    pub fn process_server_final(&mut self, server_final: &[u8]) -> Result<(), String> {
        let state = std::mem::replace(self, GaussAuthState::Done);

        match state {
            GaussAuthState::WaitingForServerFinal { server_signature } => {
                let server_final_str =
                    std::str::from_utf8(server_final).map_err(|e| format!("Invalid UTF-8: {e}"))?;

                if let Some(verifier) = server_final_str.strip_prefix("v=") {
                    let received = STANDARD
                        .decode(verifier)
                        .map_err(|e| format!("Bad base64: {e}"))?;
                    if received != server_signature {
                        return Err("Server signature mismatch".to_string());
                    }
                    Ok(())
                } else if let Some(err) = server_final_str.strip_prefix("e=") {
                    Err(format!("Server error: {err}"))
                } else {
                    Ok(())
                }
            }
            _ => Err("Unexpected state".to_string()),
        }
    }
}

/// openGauss RFC5802 challenge-response authentication.
/// Single-round: server sends random+token, client computes proof and sends back.
pub fn rfc5802_sha256(
    password: &str,
    random64code: &str,
    token: &str,
    server_signature_hex: Option<&str>,
    server_iteration: u32,
) -> Result<String, String> {
    let salt = hex::decode(random64code).map_err(|e| format!("Bad random64code hex: {e}"))?;
    let token_bytes = hex::decode(token).map_err(|e| format!("Bad token hex: {e}"))?;

    let iteration = if let Some(expected_sig) = server_signature_hex {
        detect_iteration(
            password.as_bytes(),
            &salt,
            &token_bytes,
            expected_sig,
            server_iteration,
        )?
    } else {
        server_iteration
    };

    let mut k = [0u8; 32];
    pbkdf2::pbkdf2_hmac::<sha1::Sha1>(password.as_bytes(), &salt, iteration, &mut k);

    // "Sever Key" is intentional (openGauss typo preserved for compatibility)
    let client_key = hmac_sha256(&k, b"Client Key");
    let stored_key = sha256(&client_key);

    let hmac_result = hmac_sha256(&stored_key, &token_bytes);
    let mut h = hmac_result;
    for (a, b) in h.iter_mut().zip(client_key.iter()) {
        *a ^= b;
    }

    Ok(hex::encode(h))
}

const CANDIDATE_ITERATIONS: &[u32] = &[10000, 2048];

fn detect_iteration(
    password: &[u8],
    salt: &[u8],
    token_bytes: &[u8],
    expected_sig: &str,
    default: u32,
) -> Result<u32, String> {
    for &iter in CANDIDATE_ITERATIONS {
        let mut k = [0u8; 32];
        pbkdf2::pbkdf2_hmac::<sha1::Sha1>(password, salt, iter, &mut k);
        let server_key = hmac_sha256(&k, b"Sever Key");
        let sig = hex::encode(hmac_sha256(&server_key, token_bytes));
        if sig == expected_sig {
            return Ok(iter);
        }
    }
    // No candidate matched, fall back to default
    Ok(default)
}
