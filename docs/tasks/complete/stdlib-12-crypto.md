# Task: Stdlib — `crypto`

> Read `docs/tasks/stdlib-00-overview.md` first for the global rules.

## Scope

Implement the `crypto` module from `docs/stdlib.md` (section "`crypto` —
Cryptography").

Opinionated: only safe-by-default primitives. No raw ECB. No MD5. No DIY.

## Output File

- `crates/ferric_stdlib/src/crypto.rs`

Exposes `pub fn register(registry, interner)`.

## Required `NativeValue` Variants

```rust
// REQUIRED NATIVE VALUE VARIANTS:
//   Bytes(Vec<u8>)            // shared with str-bytes task
//   Tuple2(Box<NativeValue>, Box<NativeValue>)   // for ed25519_keypair
```

## Required Deps

```rust
// REQUIRED DEPS:
//   sha2 = "0.10"
//   blake3 = "1"
//   hmac = "0.12"
//   chacha20poly1305 = "0.10"
//   aes-gcm = "0.10"
//   argon2 = "0.5"
//   ed25519-dalek = { version = "2", features = ["rand_core"] }
//   rand = "0.8"
//   subtle = "2"            // for constant-time eq
```

## Function Surface (must register)

Hashing:

`sha256`, `sha384`, `sha512`, `blake3`, `sha256_str`, `sha512_str`,
`blake3_str`.

HMAC:

`hmac_sha256`, `hmac_sha512`.

Constant-time eq:

`eq_constant`.

ChaCha20-Poly1305:

`chacha_encrypt`, `chacha_decrypt`, `chacha_key`, `chacha_nonce`.

AES-GCM:

`aes_gcm_encrypt`, `aes_gcm_decrypt`.

Password hashing:

`hash_password`, `verify_password`.

Ed25519:

`ed25519_keypair`, `ed25519_sign`, `ed25519_verify`.

Random:

`random_bytes`, `random_int`.

## Implementation Notes

- Hash functions return raw `Bytes`. Callers can hex/base64 via `encode`.
- `sha*_str` UTF-8 encode the input first.
- `eq_constant` uses `subtle::ConstantTimeEq` to avoid timing leaks.
- ChaCha20-Poly1305 key is 32 bytes, nonce is 12 bytes. Wrong-length input
  errs with `CryptoError::InvalidKeyLength` / `InvalidNonceLength`.
- AES-GCM-256 (use `Aes256Gcm`) — key 32, nonce 12.
- Decryption auth failure → `CryptoError::DecryptionFailed`.
- `hash_password` uses Argon2id with the crate's default-secure parameters
  and emits the PHC string format (`$argon2id$v=19$...`). Salt is
  randomly generated per call.
- `verify_password` parses a PHC string and verifies. Malformed hash →
  `InvalidHashFormat`.
- `random_bytes(n)` uses `rand::thread_rng().fill(&mut buf)`.
- `random_int(low, high)` is **inclusive** on both bounds and uses
  cryptographically secure RNG (different from `rand` module which is
  non-crypto).

## Tests

- `sha256(b"abc")` matches the standard test vector
  (`ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad`).
- `sha256` and `sha256_str("abc")` produce the same bytes.
- `sha512(b"")` matches its known empty-input test vector.
- `blake3(b"")` matches its known empty-input vector.
- `hmac_sha256(b"key", b"The quick brown fox jumps over the lazy dog")`
  matches the RFC 4231 vector.
- `eq_constant(b"abc", b"abc")` true; `eq_constant(b"abc", b"abd")` false;
  unequal-length inputs return false (no panic).
- ChaCha20-Poly1305 roundtrip:
  - `key = chacha_key(); nonce = chacha_nonce(); ct = chacha_encrypt(key,
    nonce, b"plain"); chacha_decrypt(key, nonce, ct) == b"plain"`.
- ChaCha20-Poly1305 tamper detection:
  - mutate one byte of ciphertext; decrypt errs `DecryptionFailed`.
- ChaCha20 wrong key length errs.
- AES-GCM roundtrip + tamper detection.
- `hash_password("hunter2")` produces a string starting with `$argon2`;
  `verify_password("hunter2", hash)` → `Ok(true)`;
  `verify_password("wrong", hash)` → `Ok(false)`.
- `verify_password("x", "garbage")` errs.
- Ed25519:
  - `(sk, pk) = ed25519_keypair()`;
  - `sig = ed25519_sign(sk, b"msg")`;
  - `ed25519_verify(pk, b"msg", sig)` → `Ok(true)`;
  - `ed25519_verify(pk, b"tampered", sig)` → `Ok(false)`.
- `random_bytes(32).len()` == 32; two calls differ (with overwhelming
  probability).
- `random_int(0, 0)` → 0; `random_int(1, 6)` ∈ `[1, 6]` over 100 trials.

Aim for ~25 tests.

## Constraints

- Do **not** modify `lib.rs` or `Cargo.toml`.
- Argon2 password hashing tests can be slow — keep parameters at the
  crate's defaults; do not crank them in tests.

## Acceptance

- One file with the listed register coverage.
- Tests verify against published test vectors where available.
- No edits outside the new file.
