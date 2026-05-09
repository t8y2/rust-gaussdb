use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

type HmacSha1 = Hmac<sha1::Sha1>;
type HmacSha256 = Hmac<Sha256>;

fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC key");
    mac.update(data);
    mac.finalize().into_bytes().into()
}

fn pbkdf2_sha1_manual(password: &[u8], salt: &[u8], iterations: u32, output: &mut [u8]) {
    let mut block_num = 1u32;
    let mut offset = 0;
    while offset < output.len() {
        let mut mac = HmacSha1::new_from_slice(password).expect("HMAC key");
        mac.update(salt);
        mac.update(&block_num.to_be_bytes());
        let u1 = mac.finalize().into_bytes();
        let mut prev = u1;
        let mut block = [0u8; 20];
        block.copy_from_slice(&prev);
        for _ in 1..iterations {
            let mut mac = HmacSha1::new_from_slice(password).expect("HMAC key");
            mac.update(&prev);
            prev = mac.finalize().into_bytes();
            for (a, b) in block.iter_mut().zip(prev.iter()) {
                *a ^= b;
            }
        }
        let copy_len = std::cmp::min(20, output.len() - offset);
        output[offset..offset + copy_len].copy_from_slice(&block[..copy_len]);
        offset += copy_len;
        block_num += 1;
    }
}

#[test]
fn test_pbkdf2_rfc6070() {
    let mut out = [0u8; 20];
    pbkdf2::pbkdf2_hmac::<sha1::Sha1>(b"password", b"salt", 1, &mut out);
    let expected = hex::decode("0c60c80f961f0e71f3a9b524af6012062fe037a6").unwrap();
    println!("crate:    {}", hex::encode(&out));
    println!("expected: {}", hex::encode(&expected));
    assert_eq!(&out[..], &expected[..], "RFC6070 test vector failed");
}

#[test]
fn test_crate_vs_manual() {
    let password = b"QB!vdjmLRueEZ6";
    let salt =
        hex::decode("ee08c1c27a10fa328b74ed6831368b0ea3f0911ce52ac4ac3d1b5b85f9394077").unwrap();

    let mut crate_k = [0u8; 32];
    pbkdf2::pbkdf2_hmac::<sha1::Sha1>(password, &salt, 2048, &mut crate_k);

    let mut manual_k = [0u8; 32];
    pbkdf2_sha1_manual(password, &salt, 2048, &mut manual_k);

    println!("crate  K: {}", hex::encode(&crate_k));
    println!("manual K: {}", hex::encode(&manual_k));
    assert_eq!(crate_k, manual_k);

    let token_bytes = hex::decode("24ca4686").unwrap();
    let expected_sig = "33a1a6bedf3c8fa2fcab344e1c8f0feaeb4bfd894faf5dac749c9787367f54ce";

    for label in [b"Sever Key" as &[u8], b"Server Key"] {
        let sk = hmac_sha256(&crate_k, label);
        let sig = hex::encode(hmac_sha256(&sk, &token_bytes));
        let name = std::str::from_utf8(label).unwrap();
        println!("iter=2048 {name}: sig={sig} match={}", sig == expected_sig);
    }

    for iter in [2048u32, 10000, 1, 4096] {
        let mut k = [0u8; 32];
        pbkdf2::pbkdf2_hmac::<sha1::Sha1>(password, &salt, iter, &mut k);
        let sk = hmac_sha256(&k, b"Sever Key");
        let sig = hex::encode(hmac_sha256(&sk, &token_bytes));
        println!("iter={iter} Sever: {sig} match={}", sig == expected_sig);
    }
}
