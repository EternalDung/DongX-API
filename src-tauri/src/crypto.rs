use crate::error::AppError;
use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use rand::RngCore;

/// Encrypt plaintext using AES-256-GCM with a machine-bound key.
///
/// In production, the key is derived from the OS keyring (Windows DPAPI).
/// For now, we use a placeholder key — replace with keyring-based derivation.
///
/// Java comparison: this is like javax.crypto.Cipher with AES/GCM/NoPadding,
/// but Rust's type system enforces key/nonce correctness at compile time.
pub fn encrypt(plaintext: &str) -> Result<String, AppError> {
    let key = derive_key();
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| AppError::Crypto(format!("Invalid key: {}", e)))?;

    // Generate a random 12-byte nonce (like Java's SecureRandom IV)
    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| AppError::Crypto(format!("Encryption failed: {}", e)))?;

    // Prepend nonce to ciphertext, then base64 encode
    let mut combined = nonce_bytes.to_vec();
    combined.extend_from_slice(&ciphertext);
    Ok(BASE64.encode(&combined))
}

/// Decrypt ciphertext produced by encrypt()
pub fn decrypt(ciphertext_b64: &str) -> Result<String, AppError> {
    let key = derive_key();
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| AppError::Crypto(format!("Invalid key: {}", e)))?;

    let combined = BASE64
        .decode(ciphertext_b64)
        .map_err(|e| AppError::Crypto(format!("Base64 decode failed: {}", e)))?;

    if combined.len() < 12 {
        return Err(AppError::Crypto("Ciphertext too short".into()));
    }

    let nonce = Nonce::from_slice(&combined[..12]);
    let ciphertext = &combined[12..];

    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| AppError::Crypto(format!("Decryption failed: {}", e)))?;

    String::from_utf8(plaintext)
        .map_err(|e| AppError::Crypto(format!("UTF-8 decode failed: {}", e)))
}

/// Generate a random API key with prefix
pub fn generate_api_key() -> String {
    let mut bytes = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut bytes);
    let encoded = BASE64.encode(bytes);
    // URL-safe, no padding
    let encoded = encoded
        .replace('+', "-")
        .replace('/', "_")
        .trim_end_matches('=')
        .to_string();
    format!("sk-dongapi-{}", encoded)
}

/// Derive encryption key from machine identity.
///
/// TODO: Use `keyring` crate to store/retrieve a master key in Windows DPAPI.
/// For now, use a fixed placeholder — NOT for production.
fn derive_key() -> [u8; 32] {
    // Placeholder: 32 bytes of a fixed seed.
    // Replace with keyring-based derivation before storing real secrets.
    *b"DongX_PLACEHOLDER_KEY_32_BYTES!!"
}
