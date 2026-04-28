//! # Ferric Standard Library — `crypto` module
//!
//! Opinionated, safe-by-default cryptographic primitives. No raw ECB. No MD5.
//! No DIY. See `docs/stdlib.md` (`crypto` section) and
//! `docs/tasks/stdlib-12-crypto.md` for the spec.
//!
//! All functions are registered with the `crypto_*` prefix
//! (e.g. `crypto::sha256` -> native name `crypto_sha256`).
//!
//! Errors are returned as `Result<NativeValue, String>` at the native ABI
//! boundary. Internally these correspond to spec `CryptoError` variants:
//!
//! - `InvalidKeyLength`   - wrong-size symmetric key
//! - `InvalidNonceLength` - wrong-size nonce
//! - `DecryptionFailed`   - authentication tag mismatch (tamper detection)
//! - `InvalidHashFormat`  - PHC password hash could not be parsed
//! - `InternalError`      - everything else (RNG failures, etc.)
//!
//! REQUIRED NATIVE VALUE VARIANTS:
//!   Bytes(Vec<u8>)            // shared with str-bytes task
//!   Tuple2(Box<NativeValue>, Box<NativeValue>)   // for ed25519_keypair
//!
//! REQUIRED DEPS:
//!   sha2 = "0.10"
//!   blake3 = "1"
//!   hmac = "0.12"
//!   chacha20poly1305 = "0.10"
//!   aes-gcm = "0.10"
//!   argon2 = "0.5"
//!   ed25519-dalek = { version = "2", features = ["rand_core"] }
//!   rand = "0.8"
//!   subtle = "2"            // for constant-time eq

use crate::{NativeRegistry, NativeValue};
use ferric_common::Interner;

use aes_gcm::aead::{Aead, KeyInit as AesKeyInit};
use aes_gcm::{Aes256Gcm, Key as AesKey, Nonce as AesNonce};
use argon2::password_hash::{
    rand_core::OsRng as ArgonOsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
};
use argon2::Argon2;
use chacha20poly1305::{ChaCha20Poly1305, Key as ChaKey, Nonce as ChaNonce};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use hmac::{Hmac, Mac};
use rand::{rngs::OsRng, RngCore};
use sha2::{Digest, Sha256, Sha384, Sha512};
use subtle::ConstantTimeEq;

// ============================================================================
// Constants
// ============================================================================

const CHACHA_KEY_LEN: usize = 32;
const CHACHA_NONCE_LEN: usize = 12;
const AES_KEY_LEN: usize = 32; // AES-256
const AES_NONCE_LEN: usize = 12; // GCM standard
const ED25519_SK_LEN: usize = 32;
const ED25519_PK_LEN: usize = 32;
const ED25519_SIG_LEN: usize = 64;

// ============================================================================
// Helpers (private)
// ============================================================================

fn check_arg_count(args: &[NativeValue], expected: usize) -> Result<(), String> {
    if args.len() != expected {
        Err(format!(
            "expected {} argument(s), got {}",
            expected,
            args.len()
        ))
    } else {
        Ok(())
    }
}

fn expect_bytes(value: &NativeValue) -> Result<&[u8], String> {
    match value {
        NativeValue::Bytes(b) => Ok(b.as_slice()),
        other => Err(format!("expected Bytes, got {other:?}")),
    }
}

fn expect_str(value: &NativeValue) -> Result<&str, String> {
    match value {
        NativeValue::Str(s) => Ok(s.as_str()),
        other => Err(format!("expected Str, got {other:?}")),
    }
}

fn expect_int(value: &NativeValue) -> Result<i64, String> {
    match value {
        NativeValue::Int(n) => Ok(*n),
        other => Err(format!("expected Int, got {other:?}")),
    }
}

fn bytes(v: Vec<u8>) -> NativeValue {
    NativeValue::Bytes(v)
}

// ============================================================================
// Hashing
// ============================================================================

fn builtin_crypto_sha256(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let data = expect_bytes(&args[0])?;
    let digest = Sha256::digest(data);
    Ok(bytes(digest.to_vec()))
}

fn builtin_crypto_sha384(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let data = expect_bytes(&args[0])?;
    let digest = Sha384::digest(data);
    Ok(bytes(digest.to_vec()))
}

fn builtin_crypto_sha512(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let data = expect_bytes(&args[0])?;
    let digest = Sha512::digest(data);
    Ok(bytes(digest.to_vec()))
}

fn builtin_crypto_blake3(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let data = expect_bytes(&args[0])?;
    let digest = blake3::hash(data);
    Ok(bytes(digest.as_bytes().to_vec()))
}

fn builtin_crypto_sha256_str(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let s = expect_str(&args[0])?;
    let digest = Sha256::digest(s.as_bytes());
    Ok(bytes(digest.to_vec()))
}

fn builtin_crypto_sha512_str(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let s = expect_str(&args[0])?;
    let digest = Sha512::digest(s.as_bytes());
    Ok(bytes(digest.to_vec()))
}

fn builtin_crypto_blake3_str(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let s = expect_str(&args[0])?;
    let digest = blake3::hash(s.as_bytes());
    Ok(bytes(digest.as_bytes().to_vec()))
}

// ============================================================================
// HMAC
// ============================================================================

fn builtin_crypto_hmac_sha256(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let key = expect_bytes(&args[0])?;
    let data = expect_bytes(&args[1])?;
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(key)
        .map_err(|e| format!("InternalError: hmac_sha256: {e}"))?;
    mac.update(data);
    Ok(bytes(mac.finalize().into_bytes().to_vec()))
}

fn builtin_crypto_hmac_sha512(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let key = expect_bytes(&args[0])?;
    let data = expect_bytes(&args[1])?;
    let mut mac = <Hmac<Sha512> as Mac>::new_from_slice(key)
        .map_err(|e| format!("InternalError: hmac_sha512: {e}"))?;
    mac.update(data);
    Ok(bytes(mac.finalize().into_bytes().to_vec()))
}

// ============================================================================
// Constant-time equality
// ============================================================================

fn builtin_crypto_eq_constant(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let a = expect_bytes(&args[0])?;
    let b = expect_bytes(&args[1])?;
    if a.len() != b.len() {
        // Spec: unequal-length inputs return false (no panic). Returning
        // early here is fine because the *length* is not secret data; the
        // contents would have been the constant-time comparison's job.
        return Ok(NativeValue::Bool(false));
    }
    let eq = a.ct_eq(b).unwrap_u8() == 1;
    Ok(NativeValue::Bool(eq))
}

// ============================================================================
// ChaCha20-Poly1305
// ============================================================================

fn builtin_crypto_chacha_encrypt(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 3)?;
    let key = expect_bytes(&args[0])?;
    let nonce = expect_bytes(&args[1])?;
    let plaintext = expect_bytes(&args[2])?;

    if key.len() != CHACHA_KEY_LEN {
        return Err(format!(
            "InvalidKeyLength: expected {}, found {}",
            CHACHA_KEY_LEN,
            key.len()
        ));
    }
    if nonce.len() != CHACHA_NONCE_LEN {
        return Err(format!(
            "InvalidNonceLength: expected {}, found {}",
            CHACHA_NONCE_LEN,
            nonce.len()
        ));
    }

    let cipher = ChaCha20Poly1305::new(ChaKey::from_slice(key));
    let n = ChaNonce::from_slice(nonce);
    let ct = cipher
        .encrypt(n, plaintext)
        .map_err(|_| "InternalError: chacha_encrypt failed".to_string())?;
    Ok(bytes(ct))
}

fn builtin_crypto_chacha_decrypt(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 3)?;
    let key = expect_bytes(&args[0])?;
    let nonce = expect_bytes(&args[1])?;
    let ciphertext = expect_bytes(&args[2])?;

    if key.len() != CHACHA_KEY_LEN {
        return Err(format!(
            "InvalidKeyLength: expected {}, found {}",
            CHACHA_KEY_LEN,
            key.len()
        ));
    }
    if nonce.len() != CHACHA_NONCE_LEN {
        return Err(format!(
            "InvalidNonceLength: expected {}, found {}",
            CHACHA_NONCE_LEN,
            nonce.len()
        ));
    }

    let cipher = ChaCha20Poly1305::new(ChaKey::from_slice(key));
    let n = ChaNonce::from_slice(nonce);
    let pt = cipher
        .decrypt(n, ciphertext)
        .map_err(|_| "DecryptionFailed".to_string())?;
    Ok(bytes(pt))
}

fn builtin_crypto_chacha_key(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 0)?;
    let mut buf = vec![0u8; CHACHA_KEY_LEN];
    OsRng.fill_bytes(&mut buf);
    Ok(bytes(buf))
}

fn builtin_crypto_chacha_nonce(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 0)?;
    let mut buf = vec![0u8; CHACHA_NONCE_LEN];
    OsRng.fill_bytes(&mut buf);
    Ok(bytes(buf))
}

// ============================================================================
// AES-256-GCM
// ============================================================================

fn builtin_crypto_aes_gcm_encrypt(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 3)?;
    let key = expect_bytes(&args[0])?;
    let nonce = expect_bytes(&args[1])?;
    let plaintext = expect_bytes(&args[2])?;

    if key.len() != AES_KEY_LEN {
        return Err(format!(
            "InvalidKeyLength: expected {}, found {}",
            AES_KEY_LEN,
            key.len()
        ));
    }
    if nonce.len() != AES_NONCE_LEN {
        return Err(format!(
            "InvalidNonceLength: expected {}, found {}",
            AES_NONCE_LEN,
            nonce.len()
        ));
    }

    let cipher = Aes256Gcm::new(AesKey::<Aes256Gcm>::from_slice(key));
    let n = AesNonce::from_slice(nonce);
    let ct = cipher
        .encrypt(n, plaintext)
        .map_err(|_| "InternalError: aes_gcm_encrypt failed".to_string())?;
    Ok(bytes(ct))
}

fn builtin_crypto_aes_gcm_decrypt(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 3)?;
    let key = expect_bytes(&args[0])?;
    let nonce = expect_bytes(&args[1])?;
    let ciphertext = expect_bytes(&args[2])?;

    if key.len() != AES_KEY_LEN {
        return Err(format!(
            "InvalidKeyLength: expected {}, found {}",
            AES_KEY_LEN,
            key.len()
        ));
    }
    if nonce.len() != AES_NONCE_LEN {
        return Err(format!(
            "InvalidNonceLength: expected {}, found {}",
            AES_NONCE_LEN,
            nonce.len()
        ));
    }

    let cipher = Aes256Gcm::new(AesKey::<Aes256Gcm>::from_slice(key));
    let n = AesNonce::from_slice(nonce);
    let pt = cipher
        .decrypt(n, ciphertext)
        .map_err(|_| "DecryptionFailed".to_string())?;
    Ok(bytes(pt))
}

// ============================================================================
// Password hashing (Argon2id, PHC string format, crate defaults)
// ============================================================================

fn builtin_crypto_hash_password(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let password = expect_str(&args[0])?;
    let salt = SaltString::generate(&mut ArgonOsRng);
    let argon2 = Argon2::default();
    let hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| format!("InternalError: hash_password: {e}"))?
        .to_string();
    Ok(NativeValue::Str(hash))
}

fn builtin_crypto_verify_password(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let password = expect_str(&args[0])?;
    let hash_str = expect_str(&args[1])?;
    let parsed =
        PasswordHash::new(hash_str).map_err(|_| "InvalidHashFormat".to_string())?;
    match Argon2::default().verify_password(password.as_bytes(), &parsed) {
        Ok(()) => Ok(NativeValue::Bool(true)),
        Err(argon2::password_hash::Error::Password) => Ok(NativeValue::Bool(false)),
        Err(e) => Err(format!("InternalError: verify_password: {e}")),
    }
}

// ============================================================================
// Ed25519
// ============================================================================

fn builtin_crypto_ed25519_keypair(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 0)?;
    let mut sk_bytes = [0u8; ED25519_SK_LEN];
    OsRng.fill_bytes(&mut sk_bytes);
    let signing = SigningKey::from_bytes(&sk_bytes);
    let verifying = signing.verifying_key();
    let sk = signing.to_bytes().to_vec();
    let pk = verifying.to_bytes().to_vec();
    Ok(NativeValue::Tuple2(
        Box::new(bytes(sk)),
        Box::new(bytes(pk)),
    ))
}

fn builtin_crypto_ed25519_sign(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let sk_bytes = expect_bytes(&args[0])?;
    let message = expect_bytes(&args[1])?;
    if sk_bytes.len() != ED25519_SK_LEN {
        return Err(format!(
            "InvalidKeyLength: expected {}, found {}",
            ED25519_SK_LEN,
            sk_bytes.len()
        ));
    }
    let arr: [u8; ED25519_SK_LEN] = sk_bytes
        .try_into()
        .map_err(|_| "InvalidKeyLength: ed25519 sk".to_string())?;
    let signing = SigningKey::from_bytes(&arr);
    let sig: Signature = signing.sign(message);
    Ok(bytes(sig.to_bytes().to_vec()))
}

fn builtin_crypto_ed25519_verify(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 3)?;
    let pk_bytes = expect_bytes(&args[0])?;
    let message = expect_bytes(&args[1])?;
    let sig_bytes = expect_bytes(&args[2])?;
    if pk_bytes.len() != ED25519_PK_LEN {
        return Err(format!(
            "InvalidKeyLength: expected {}, found {}",
            ED25519_PK_LEN,
            pk_bytes.len()
        ));
    }
    if sig_bytes.len() != ED25519_SIG_LEN {
        return Err(format!(
            "InternalError: expected signature length {}, found {}",
            ED25519_SIG_LEN,
            sig_bytes.len()
        ));
    }
    let pk_arr: [u8; ED25519_PK_LEN] = pk_bytes
        .try_into()
        .map_err(|_| "InvalidKeyLength: ed25519 pk".to_string())?;
    let sig_arr: [u8; ED25519_SIG_LEN] = sig_bytes
        .try_into()
        .map_err(|_| "InternalError: ed25519 sig length".to_string())?;
    let verifying = VerifyingKey::from_bytes(&pk_arr)
        .map_err(|e| format!("InternalError: ed25519 pk: {e}"))?;
    let sig = Signature::from_bytes(&sig_arr);
    Ok(NativeValue::Bool(verifying.verify(message, &sig).is_ok()))
}

// ============================================================================
// Random
// ============================================================================

fn builtin_crypto_random_bytes(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let n = expect_int(&args[0])?;
    if n < 0 {
        return Err(format!("InternalError: random_bytes: negative length {n}"));
    }
    let mut buf = vec![0u8; n as usize];
    OsRng.fill_bytes(&mut buf);
    Ok(bytes(buf))
}

fn builtin_crypto_random_int(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let low = expect_int(&args[0])?;
    let high = expect_int(&args[1])?;
    if low > high {
        return Err(format!(
            "InternalError: random_int: low ({low}) > high ({high})"
        ));
    }
    // Inclusive range. Compute span as u128 to avoid i64 overflow when
    // low = i64::MIN and high = i64::MAX.
    let span = (high as i128) - (low as i128) + 1;
    debug_assert!(span >= 1);
    let span = span as u128;

    // Rejection-sample a u128 to remove modulo bias.
    let zone = u128::MAX - (u128::MAX % span);
    loop {
        let mut buf = [0u8; 16];
        OsRng.fill_bytes(&mut buf);
        let r = u128::from_le_bytes(buf);
        if r < zone {
            let offset = (r % span) as i128;
            let value = (low as i128) + offset;
            return Ok(NativeValue::Int(value as i64));
        }
    }
}

// ============================================================================
// Registration
// ============================================================================

/// Registers every `crypto::*` function with the given registry.
///
/// Native names are `crypto_<function>` (e.g. `crypto::sha256` ->
/// `crypto_sha256`).
pub fn register(registry: &mut NativeRegistry, interner: &mut Interner) {
    type Entry = (&'static str, &'static [&'static str], crate::NativeFn);
    let entries: &[Entry] = &[
        // Hashing
        ("crypto_sha256",          &["data"],                       builtin_crypto_sha256),
        ("crypto_sha384",          &["data"],                       builtin_crypto_sha384),
        ("crypto_sha512",          &["data"],                       builtin_crypto_sha512),
        ("crypto_blake3",          &["data"],                       builtin_crypto_blake3),
        ("crypto_sha256_str",      &["s"],                          builtin_crypto_sha256_str),
        ("crypto_sha512_str",      &["s"],                          builtin_crypto_sha512_str),
        ("crypto_blake3_str",      &["s"],                          builtin_crypto_blake3_str),
        // HMAC
        ("crypto_hmac_sha256",     &["key", "data"],                builtin_crypto_hmac_sha256),
        ("crypto_hmac_sha512",     &["key", "data"],                builtin_crypto_hmac_sha512),
        // Constant-time eq
        ("crypto_eq_constant",     &["a", "b"],                     builtin_crypto_eq_constant),
        // ChaCha20-Poly1305
        ("crypto_chacha_encrypt",  &["key", "nonce", "plaintext"],  builtin_crypto_chacha_encrypt),
        ("crypto_chacha_decrypt",  &["key", "nonce", "ciphertext"], builtin_crypto_chacha_decrypt),
        ("crypto_chacha_key",      &[],                             builtin_crypto_chacha_key),
        ("crypto_chacha_nonce",    &[],                             builtin_crypto_chacha_nonce),
        // AES-GCM
        ("crypto_aes_gcm_encrypt", &["key", "nonce", "plaintext"],  builtin_crypto_aes_gcm_encrypt),
        ("crypto_aes_gcm_decrypt", &["key", "nonce", "ciphertext"], builtin_crypto_aes_gcm_decrypt),
        // Password hashing
        ("crypto_hash_password",   &["password"],                   builtin_crypto_hash_password),
        ("crypto_verify_password", &["password", "hash"],           builtin_crypto_verify_password),
        // Ed25519
        ("crypto_ed25519_keypair", &[],                             builtin_crypto_ed25519_keypair),
        ("crypto_ed25519_sign",    &["private_key", "message"],     builtin_crypto_ed25519_sign),
        ("crypto_ed25519_verify",  &["public_key", "message", "sig"], builtin_crypto_ed25519_verify),
        // Random
        ("crypto_random_bytes",    &["n"],                          builtin_crypto_random_bytes),
        ("crypto_random_int",      &["low", "high"],                builtin_crypto_random_int),
    ];
    for (name, params, f) in entries {
        registry.register_named(interner, name, params, *f);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- helpers ------------------------------------------------------------

    fn b(v: &[u8]) -> NativeValue {
        NativeValue::Bytes(v.to_vec())
    }

    fn s(v: &str) -> NativeValue {
        NativeValue::Str(v.to_string())
    }

    fn i(v: i64) -> NativeValue {
        NativeValue::Int(v)
    }

    fn unwrap_bytes(v: NativeValue) -> Vec<u8> {
        match v {
            NativeValue::Bytes(b) => b,
            other => panic!("expected Bytes, got {other:?}"),
        }
    }

    fn unwrap_bool(v: NativeValue) -> bool {
        match v {
            NativeValue::Bool(b) => b,
            other => panic!("expected Bool, got {other:?}"),
        }
    }

    fn unwrap_str(v: NativeValue) -> String {
        match v {
            NativeValue::Str(s) => s,
            other => panic!("expected Str, got {other:?}"),
        }
    }

    fn unwrap_int(v: NativeValue) -> i64 {
        match v {
            NativeValue::Int(n) => n,
            other => panic!("expected Int, got {other:?}"),
        }
    }

    fn from_hex(s: &str) -> Vec<u8> {
        assert!(s.len() % 2 == 0);
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    // --- hashing: published vectors -----------------------------------------

    #[test]
    fn sha256_abc_matches_nist_vector() {
        // FIPS 180-2 vector for "abc"
        let out = unwrap_bytes(builtin_crypto_sha256(&[b(b"abc")]).unwrap());
        let expected =
            from_hex("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(out, expected);
    }

    #[test]
    fn sha256_str_matches_sha256_bytes() {
        let from_str = unwrap_bytes(builtin_crypto_sha256_str(&[s("abc")]).unwrap());
        let from_bytes = unwrap_bytes(builtin_crypto_sha256(&[b(b"abc")]).unwrap());
        assert_eq!(from_str, from_bytes);
    }

    #[test]
    fn sha512_empty_matches_known_vector() {
        // NIST vector for SHA-512 of the empty string.
        let out = unwrap_bytes(builtin_crypto_sha512(&[b(b"")]).unwrap());
        let expected = from_hex(concat!(
            "cf83e1357eefb8bdf1542850d66d8007",
            "d620e4050b5715dc83f4a921d36ce9ce",
            "47d0d13c5d85f2b0ff8318d2877eec2f",
            "63b931bd47417a81a538327af927da3e",
        ));
        assert_eq!(out, expected);
    }

    #[test]
    fn sha384_has_correct_length() {
        let out = unwrap_bytes(builtin_crypto_sha384(&[b(b"abc")]).unwrap());
        assert_eq!(out.len(), 48);
    }

    #[test]
    fn blake3_empty_matches_known_vector() {
        // From the BLAKE3 reference test vectors.
        let out = unwrap_bytes(builtin_crypto_blake3(&[b(b"")]).unwrap());
        let expected =
            from_hex("af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262");
        assert_eq!(out, expected);
    }

    #[test]
    fn blake3_str_matches_blake3_bytes() {
        let from_str = unwrap_bytes(builtin_crypto_blake3_str(&[s("hello")]).unwrap());
        let from_bytes = unwrap_bytes(builtin_crypto_blake3(&[b(b"hello")]).unwrap());
        assert_eq!(from_str, from_bytes);
    }

    #[test]
    fn sha512_str_matches_sha512_bytes() {
        let from_str = unwrap_bytes(builtin_crypto_sha512_str(&[s("hello")]).unwrap());
        let from_bytes = unwrap_bytes(builtin_crypto_sha512(&[b(b"hello")]).unwrap());
        assert_eq!(from_str, from_bytes);
    }

    // --- HMAC: RFC 4231-style vector ----------------------------------------

    #[test]
    fn hmac_sha256_rfc_style_vector() {
        // Famous "key" / "The quick brown fox jumps over the lazy dog" vector.
        // (Wikipedia's canonical HMAC-SHA-256 example.)
        let key = b(b"key");
        let data = b(b"The quick brown fox jumps over the lazy dog");
        let out = unwrap_bytes(builtin_crypto_hmac_sha256(&[key, data]).unwrap());
        let expected =
            from_hex("f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8");
        assert_eq!(out, expected);
    }

    #[test]
    fn hmac_sha512_has_correct_length() {
        let out = unwrap_bytes(
            builtin_crypto_hmac_sha512(&[b(b"key"), b(b"data")]).unwrap(),
        );
        assert_eq!(out.len(), 64);
    }

    // --- constant-time eq ---------------------------------------------------

    #[test]
    fn eq_constant_equal() {
        assert!(unwrap_bool(
            builtin_crypto_eq_constant(&[b(b"abc"), b(b"abc")]).unwrap()
        ));
    }

    #[test]
    fn eq_constant_unequal_same_length() {
        assert!(!unwrap_bool(
            builtin_crypto_eq_constant(&[b(b"abc"), b(b"abd")]).unwrap()
        ));
    }

    #[test]
    fn eq_constant_unequal_lengths_returns_false_no_panic() {
        // Must not panic. Spec: returns false.
        assert!(!unwrap_bool(
            builtin_crypto_eq_constant(&[b(b"abc"), b(b"abcd")]).unwrap()
        ));
    }

    // --- ChaCha20-Poly1305 --------------------------------------------------

    #[test]
    fn chacha_roundtrip() {
        let key = builtin_crypto_chacha_key(&[]).unwrap();
        let nonce = builtin_crypto_chacha_nonce(&[]).unwrap();
        let plain = b(b"plain");
        let ct = builtin_crypto_chacha_encrypt(&[key.clone(), nonce.clone(), plain.clone()])
            .unwrap();
        let dec = builtin_crypto_chacha_decrypt(&[key, nonce, ct]).unwrap();
        assert_eq!(unwrap_bytes(dec), b"plain".to_vec());
    }

    #[test]
    fn chacha_key_and_nonce_have_correct_length() {
        let k = unwrap_bytes(builtin_crypto_chacha_key(&[]).unwrap());
        let n = unwrap_bytes(builtin_crypto_chacha_nonce(&[]).unwrap());
        assert_eq!(k.len(), 32);
        assert_eq!(n.len(), 12);
    }

    #[test]
    fn chacha_tamper_detection() {
        let key = builtin_crypto_chacha_key(&[]).unwrap();
        let nonce = builtin_crypto_chacha_nonce(&[]).unwrap();
        let ct = builtin_crypto_chacha_encrypt(&[
            key.clone(),
            nonce.clone(),
            b(b"hello there general kenobi"),
        ])
        .unwrap();
        let mut tampered = unwrap_bytes(ct);
        tampered[0] ^= 0x01;
        let err = builtin_crypto_chacha_decrypt(&[key, nonce, b(&tampered)]).unwrap_err();
        assert!(
            err.contains("DecryptionFailed"),
            "expected DecryptionFailed, got: {err}"
        );
    }

    #[test]
    fn chacha_wrong_key_length_errs() {
        let nonce = builtin_crypto_chacha_nonce(&[]).unwrap();
        let err = builtin_crypto_chacha_encrypt(&[
            b(&[0u8; 16]), // too short
            nonce,
            b(b"plain"),
        ])
        .unwrap_err();
        assert!(err.contains("InvalidKeyLength"), "got: {err}");
    }

    #[test]
    fn chacha_wrong_nonce_length_errs() {
        let key = builtin_crypto_chacha_key(&[]).unwrap();
        let err = builtin_crypto_chacha_encrypt(&[
            key,
            b(&[0u8; 8]), // too short
            b(b"plain"),
        ])
        .unwrap_err();
        assert!(err.contains("InvalidNonceLength"), "got: {err}");
    }

    // --- AES-GCM ------------------------------------------------------------

    fn aes_key() -> NativeValue {
        let mut k = [0u8; 32];
        OsRng.fill_bytes(&mut k);
        b(&k)
    }

    fn aes_nonce() -> NativeValue {
        let mut n = [0u8; 12];
        OsRng.fill_bytes(&mut n);
        b(&n)
    }

    #[test]
    fn aes_gcm_roundtrip() {
        let key = aes_key();
        let nonce = aes_nonce();
        let ct = builtin_crypto_aes_gcm_encrypt(&[key.clone(), nonce.clone(), b(b"secret")])
            .unwrap();
        let dec = builtin_crypto_aes_gcm_decrypt(&[key, nonce, ct]).unwrap();
        assert_eq!(unwrap_bytes(dec), b"secret".to_vec());
    }

    #[test]
    fn aes_gcm_tamper_detection() {
        let key = aes_key();
        let nonce = aes_nonce();
        let ct =
            builtin_crypto_aes_gcm_encrypt(&[key.clone(), nonce.clone(), b(b"sensitive")])
                .unwrap();
        let mut tampered = unwrap_bytes(ct);
        // Flip a bit in the middle to avoid hitting the auth-tag-only region.
        let mid = tampered.len() / 2;
        tampered[mid] ^= 0x80;
        let err = builtin_crypto_aes_gcm_decrypt(&[key, nonce, b(&tampered)]).unwrap_err();
        assert!(err.contains("DecryptionFailed"), "got: {err}");
    }

    #[test]
    fn aes_gcm_wrong_key_length_errs() {
        let err = builtin_crypto_aes_gcm_encrypt(&[
            b(&[0u8; 16]), // AES-128 key, but we only support 256
            aes_nonce(),
            b(b"x"),
        ])
        .unwrap_err();
        assert!(err.contains("InvalidKeyLength"), "got: {err}");
    }

    // --- password hashing ---------------------------------------------------

    #[test]
    fn hash_password_starts_with_argon2_marker() {
        let h = unwrap_str(builtin_crypto_hash_password(&[s("hunter2")]).unwrap());
        assert!(
            h.starts_with("$argon2"),
            "expected PHC string starting with $argon2, got: {h}"
        );
    }

    #[test]
    fn verify_password_correct() {
        let h = unwrap_str(builtin_crypto_hash_password(&[s("hunter2")]).unwrap());
        let ok =
            unwrap_bool(builtin_crypto_verify_password(&[s("hunter2"), s(&h)]).unwrap());
        assert!(ok);
    }

    #[test]
    fn verify_password_wrong() {
        let h = unwrap_str(builtin_crypto_hash_password(&[s("hunter2")]).unwrap());
        let ok = unwrap_bool(
            builtin_crypto_verify_password(&[s("wrong"), s(&h)]).unwrap(),
        );
        assert!(!ok);
    }

    #[test]
    fn verify_password_garbage_errs() {
        let err = builtin_crypto_verify_password(&[s("x"), s("garbage")]).unwrap_err();
        assert!(err.contains("InvalidHashFormat"), "got: {err}");
    }

    // --- Ed25519 ------------------------------------------------------------

    fn keypair() -> (NativeValue, NativeValue) {
        let kp = builtin_crypto_ed25519_keypair(&[]).unwrap();
        match kp {
            NativeValue::Tuple2(sk, pk) => (*sk, *pk),
            other => panic!("expected Tuple2, got {other:?}"),
        }
    }

    #[test]
    fn ed25519_keypair_lengths() {
        let (sk, pk) = keypair();
        assert_eq!(unwrap_bytes(sk).len(), 32);
        assert_eq!(unwrap_bytes(pk).len(), 32);
    }

    #[test]
    fn ed25519_sign_and_verify_roundtrip() {
        let (sk, pk) = keypair();
        let msg = b(b"msg");
        let sig = builtin_crypto_ed25519_sign(&[sk, msg.clone()]).unwrap();
        let ok = unwrap_bool(
            builtin_crypto_ed25519_verify(&[pk, msg, sig]).unwrap(),
        );
        assert!(ok);
    }

    #[test]
    fn ed25519_verify_rejects_tampered_message() {
        let (sk, pk) = keypair();
        let sig = builtin_crypto_ed25519_sign(&[sk, b(b"msg")]).unwrap();
        let ok = unwrap_bool(
            builtin_crypto_ed25519_verify(&[pk, b(b"tampered"), sig]).unwrap(),
        );
        assert!(!ok);
    }

    // --- random -------------------------------------------------------------

    #[test]
    fn random_bytes_length() {
        let out = unwrap_bytes(builtin_crypto_random_bytes(&[i(32)]).unwrap());
        assert_eq!(out.len(), 32);
    }

    #[test]
    fn random_bytes_two_calls_differ() {
        let a = unwrap_bytes(builtin_crypto_random_bytes(&[i(32)]).unwrap());
        let b_ = unwrap_bytes(builtin_crypto_random_bytes(&[i(32)]).unwrap());
        // Astronomically unlikely to collide.
        assert_ne!(a, b_);
    }

    #[test]
    fn random_int_singleton() {
        let v = unwrap_int(builtin_crypto_random_int(&[i(0), i(0)]).unwrap());
        assert_eq!(v, 0);
    }

    #[test]
    fn random_int_dice_in_range() {
        for _ in 0..100 {
            let v = unwrap_int(builtin_crypto_random_int(&[i(1), i(6)]).unwrap());
            assert!((1..=6).contains(&v), "out of range: {v}");
        }
    }

    #[test]
    fn random_int_low_greater_than_high_errs() {
        let err = builtin_crypto_random_int(&[i(10), i(1)]).unwrap_err();
        assert!(err.contains("InternalError"), "got: {err}");
    }

    // --- registration -------------------------------------------------------

    #[test]
    fn register_installs_all_functions() {
        let mut registry = NativeRegistry::new();
        let mut interner = Interner::new();
        register(&mut registry, &mut interner);

        let names = [
            "crypto_sha256",
            "crypto_sha384",
            "crypto_sha512",
            "crypto_blake3",
            "crypto_sha256_str",
            "crypto_sha512_str",
            "crypto_blake3_str",
            "crypto_hmac_sha256",
            "crypto_hmac_sha512",
            "crypto_eq_constant",
            "crypto_chacha_encrypt",
            "crypto_chacha_decrypt",
            "crypto_chacha_key",
            "crypto_chacha_nonce",
            "crypto_aes_gcm_encrypt",
            "crypto_aes_gcm_decrypt",
            "crypto_hash_password",
            "crypto_verify_password",
            "crypto_ed25519_keypair",
            "crypto_ed25519_sign",
            "crypto_ed25519_verify",
            "crypto_random_bytes",
            "crypto_random_int",
        ];
        for name in names {
            let sym = interner.intern(name);
            assert!(
                registry.get(sym).is_some(),
                "{name} not registered"
            );
        }
    }
}
