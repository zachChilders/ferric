//! Auto-extracted metadata for the `crypto` stdlib module.
//!
//! Names + param names mirror `crates/ferric_stdlib/src/crypto.rs`.
//! Type signatures are `Signature::Unknown` unless filled in by hand.

use crate::{FunctionMeta, Signature};

pub const CRYPTO_FNS: &[FunctionMeta] = &[
    FunctionMeta { name: "crypto_sha256", params: &["data"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "crypto_sha384", params: &["data"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "crypto_sha512", params: &["data"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "crypto_blake3", params: &["data"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "crypto_sha256_str", params: &["s"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "crypto_sha512_str", params: &["s"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "crypto_blake3_str", params: &["s"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "crypto_hmac_sha256", params: &["key", "data"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "crypto_hmac_sha512", params: &["key", "data"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "crypto_eq_constant", params: &["a", "b"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "crypto_chacha_encrypt", params: &["key", "nonce", "plaintext"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "crypto_chacha_decrypt", params: &["key", "nonce", "ciphertext"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "crypto_chacha_key", params: &[], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "crypto_chacha_nonce", params: &[], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "crypto_aes_gcm_encrypt", params: &["key", "nonce", "plaintext"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "crypto_aes_gcm_decrypt", params: &["key", "nonce", "ciphertext"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "crypto_hash_password", params: &["password"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "crypto_verify_password", params: &["password", "hash"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "crypto_ed25519_keypair", params: &[], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "crypto_ed25519_sign", params: &["private_key", "message"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "crypto_ed25519_verify", params: &["public_key", "message", "sig"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "crypto_random_bytes", params: &["n"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "crypto_random_int", params: &["low", "high"], signature: Signature::Unknown, display: "fn(...)" },
];
