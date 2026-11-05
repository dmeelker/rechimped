//! Cognito's variant of SRP-6a (USER_SRP_AUTH), as implemented by amazon-cognito-identity-js.

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use num_bigint::BigUint;
use num_traits::Zero;
use rand::RngCore;
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

/// RFC 3526 3072-bit MODP group prime.
const N_HEX: &str = concat!(
    "FFFFFFFFFFFFFFFFC90FDAA22168C234C4C6628B80DC1CD1",
    "29024E088A67CC74020BBEA63B139B22514A08798E3404DD",
    "EF9519B3CD3A431B302B0A6DF25F14374FE1356D6D51C245",
    "E485B576625E7EC6F44C42E9A637ED6B0BFF5CB6F406B7ED",
    "EE386BFB5A899FA5AE9F24117C4B1FE649286651ECE45B3D",
    "C2007CB8A163BF0598DA48361C55D39A69163FA8FD24CF5F",
    "83655D23DCA3AD961C62F356208552BB9ED529077096966D",
    "670C354E4ABC9804F1746C08CA18217C32905E462E36CE3B",
    "E39E772C180E86039B2783A2EC07A28FB5C55DF06F4C52C9",
    "DE2BCBF6955817183995497CEA956AE515D2261898FA0510",
    "15728E5A8AAAC42DAD33170D04507A33A85521ABDF1CBA64",
    "ECFB850458DBEF0A8AEA71575D060C7DB3970F85A6E1E4C7",
    "ABF5AE8CDB0933D71E8C94E04A25619DCEE3D2261AD2EE6B",
    "F12FFA06D98A0864D87602733EC86A64521F2B18177B200C",
    "BBE117577A615D6C770988C0BAD946E208E24FA074E5AB31",
    "43DB5BFCE0FD108E4B82D120A93AD2CAFFFFFFFFFFFFFFFF",
);

pub(crate) struct SrpClient {
    n: BigUint,
    g: BigUint,
    k: BigUint,
    a: BigUint,
    big_a: BigUint,
}

pub(crate) struct ChallengeParams<'a> {
    pub pool_name: &'a str,
    pub user_id: &'a str,
    pub password: &'a str,
    pub salt_hex: &'a str,
    pub srp_b_hex: &'a str,
    pub secret_block_b64: &'a str,
    pub timestamp: &'a str,
}

impl SrpClient {
    pub fn new() -> Self {
        loop {
            let mut bytes = [0u8; 128];
            rand::thread_rng().fill_bytes(&mut bytes);
            let client = Self::with_private_key(BigUint::from_bytes_be(&bytes));
            if !(&client.big_a % &client.n).is_zero() {
                return client;
            }
        }
    }

    pub fn with_private_key(a: BigUint) -> Self {
        let n = BigUint::parse_bytes(N_HEX.as_bytes(), 16).expect("valid N");
        let g = BigUint::from(2u32);
        let k = BigUint::from_bytes_be(&sha256(&[&pad(&n), &pad(&g)]));
        let a = a % &n;
        let big_a = g.modpow(&a, &n);
        Self { n, g, k, a, big_a }
    }

    /// The `SRP_A` auth parameter.
    pub fn public_key_hex(&self) -> String {
        self.big_a.to_str_radix(16)
    }

    /// Computes `PASSWORD_CLAIM_SIGNATURE` for the PASSWORD_VERIFIER challenge.
    pub fn password_claim_signature(&self, p: &ChallengeParams) -> Result<String, String> {
        let b = parse_hex(p.srp_b_hex, "SRP_B")?;
        if (&b % &self.n).is_zero() {
            return Err("server sent an invalid SRP_B".into());
        }
        let salt = parse_hex(p.salt_hex, "SALT")?;

        let u = BigUint::from_bytes_be(&sha256(&[&pad(&self.big_a), &pad(&b)]));
        if u.is_zero() {
            return Err("SRP scrambling parameter is zero".into());
        }

        let user_pass_hash = sha256(&[format!("{}{}:{}", p.pool_name, p.user_id, p.password).as_bytes()]);
        let x = BigUint::from_bytes_be(&sha256(&[&pad(&salt), &user_pass_hash]));

        // S = (B - k * g^x) ^ (a + u * x) mod N
        let kgx = (&self.k * self.g.modpow(&x, &self.n)) % &self.n;
        let base = (&b + &self.n - kgx) % &self.n;
        let s = base.modpow(&(&self.a + &u * &x), &self.n);

        let key = hkdf(&pad(&s), &pad(&u));
        let secret_block = BASE64
            .decode(p.secret_block_b64)
            .map_err(|e| format!("invalid SECRET_BLOCK: {e}"))?;

        let mut mac = HmacSha256::new_from_slice(&key).expect("any key length");
        mac.update(p.pool_name.as_bytes());
        mac.update(p.user_id.as_bytes());
        mac.update(&secret_block);
        mac.update(p.timestamp.as_bytes());
        Ok(BASE64.encode(mac.finalize().into_bytes()))
    }
}

/// Cognito's timestamp format, e.g. `Wed Nov 4 07:56:21 UTC 2026` (day not zero-padded).
pub(crate) fn timestamp(now: DateTime<Utc>) -> String {
    now.format("%a %b %-d %H:%M:%S UTC %Y").to_string()
}

/// Big-endian bytes, with a leading zero byte if the high bit is set (Cognito's `padHex`).
fn pad(n: &BigUint) -> Vec<u8> {
    let mut bytes = n.to_bytes_be();
    if bytes[0] & 0x80 != 0 {
        bytes.insert(0, 0);
    }
    bytes
}

fn parse_hex(s: &str, name: &str) -> Result<BigUint, String> {
    BigUint::parse_bytes(s.as_bytes(), 16).ok_or_else(|| format!("server sent an invalid {name}"))
}

fn sha256(parts: &[&[u8]]) -> Vec<u8> {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part);
    }
    hasher.finalize().to_vec()
}

fn hkdf(ikm: &[u8], salt: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(salt).expect("any key length");
    mac.update(ikm);
    let prk = mac.finalize().into_bytes();
    let mut mac = HmacSha256::new_from_slice(&prk).expect("any key length");
    mac.update(b"Caldera Derived Key\x01");
    mac.finalize().into_bytes()[..16].to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn timestamp_has_unpadded_day() {
        let t = Utc.with_ymd_and_hms(2026, 11, 4, 7, 56, 21).unwrap();
        assert_eq!(timestamp(t), "Wed Nov 4 07:56:21 UTC 2026");
    }

    /// Vector generated by pycognito (an independent implementation), see testdata/.
    #[test]
    fn matches_pycognito() {
        let v: serde_json::Value =
            serde_json::from_str(include_str!("../testdata/srp_vector_pycognito.json")).unwrap();
        let s = |k: &str| v[k].as_str().unwrap();

        let client = SrpClient::with_private_key(BigUint::parse_bytes(s("a").as_bytes(), 16).unwrap());
        assert_eq!(client.public_key_hex(), s("A"));

        let signature = client
            .password_claim_signature(&ChallengeParams {
                pool_name: "NIGD97pvM",
                user_id: s("USER_ID_FOR_SRP"),
                password: s("password"),
                salt_hex: s("SALT"),
                srp_b_hex: s("SRP_B"),
                secret_block_b64: s("SECRET_BLOCK"),
                timestamp: s("timestamp"),
            })
            .unwrap();
        assert_eq!(signature, s("signature"));
    }
}
