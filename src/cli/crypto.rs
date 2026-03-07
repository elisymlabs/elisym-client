use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use argon2::Argon2;
use serde::{Deserialize, Serialize};

use super::error::{CliError, Result};

const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;

/// All secret fields bundled for encryption/decryption.
#[derive(Serialize, Deserialize)]
pub struct SecretsBundle {
    pub nostr_secret_key: String,
    pub solana_secret_key: String,
    pub llm_api_key: String,
    #[serde(default)]
    pub customer_llm_api_key: Option<String>,
}

/// Encrypted secrets stored in config.toml.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptionSection {
    pub ciphertext: String, // bs58
    pub salt: String,       // bs58
    pub nonce: String,      // bs58
}

/// Derive a 256-bit key from password + salt using Argon2id.
fn derive_key(password: &str, salt: &[u8]) -> Result<[u8; 32]> {
    let argon2 = Argon2::default();
    let mut key = [0u8; 32];
    argon2
        .hash_password_into(password.as_bytes(), salt, &mut key)
        .map_err(|e| CliError::Other(format!("key derivation failed: {}", e)))?;
    Ok(key)
}

fn random_bytes<const N: usize>() -> [u8; N] {
    let mut buf = [0u8; N];
    getrandom::getrandom(&mut buf).expect("failed to generate random bytes");
    buf
}

/// Encrypt a secrets bundle with a password (AES-256-GCM + Argon2id).
pub fn encrypt_secrets(bundle: &SecretsBundle, password: &str) -> Result<EncryptionSection> {
    let plaintext = serde_json::to_vec(bundle)
        .map_err(|e| CliError::Other(format!("failed to serialize secrets: {}", e)))?;

    let salt = random_bytes::<SALT_LEN>();
    let nonce_bytes = random_bytes::<NONCE_LEN>();

    let key = derive_key(password, &salt)?;
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key));
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_ref())
        .map_err(|e| CliError::Other(format!("encryption failed: {}", e)))?;

    Ok(EncryptionSection {
        ciphertext: bs58::encode(&ciphertext).into_string(),
        salt: bs58::encode(salt).into_string(),
        nonce: bs58::encode(nonce_bytes).into_string(),
    })
}

/// Decrypt a secrets bundle with a password.
pub fn decrypt_secrets(section: &EncryptionSection, password: &str) -> Result<SecretsBundle> {
    let ciphertext = bs58::decode(&section.ciphertext)
        .into_vec()
        .map_err(|e| CliError::Other(format!("invalid ciphertext encoding: {}", e)))?;
    let salt = bs58::decode(&section.salt)
        .into_vec()
        .map_err(|e| CliError::Other(format!("invalid salt encoding: {}", e)))?;
    let nonce_bytes = bs58::decode(&section.nonce)
        .into_vec()
        .map_err(|e| CliError::Other(format!("invalid nonce encoding: {}", e)))?;

    let key = derive_key(password, &salt)?;
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key));
    let nonce = Nonce::from_slice(&nonce_bytes);

    let plaintext = cipher
        .decrypt(nonce, ciphertext.as_ref())
        .map_err(|_| CliError::Other("wrong password or corrupted data".into()))?;

    let bundle: SecretsBundle = serde_json::from_slice(&plaintext)
        .map_err(|e| CliError::Other(format!("failed to parse decrypted secrets: {}", e)))?;

    Ok(bundle)
}
