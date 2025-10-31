use std::sync::Arc;

use anyhow::{anyhow, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use once_cell::sync::OnceCell;
use rand::RngCore;
use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM};
use ring::rand::{SecureRandom, SystemRandom};

static KEY_MANAGER: OnceCell<Arc<KeyManager>> = OnceCell::new();

pub fn global_key_manager() -> Result<Arc<KeyManager>> {
    KEY_MANAGER
        .get_or_try_init(|| {
            let key = KeyManager::from_env()?;
            Ok(Arc::new(key))
        })
        .cloned()
}

#[derive(Clone)]
pub struct KeyManager {
    key: LessSafeKey,
}

impl KeyManager {
    pub fn from_env() -> Result<Self> {
        if let Ok(value) = std::env::var("MCP_ROUTER_MASTER_KEY") {
            let bytes = hex::decode(value.trim())
                .map_err(|_| anyhow!("MCP_ROUTER_MASTER_KEY must be valid hex"))?;
            return Self::from_bytes(&bytes);
        }
        let mut key_bytes = [0u8; 32];
        SystemRandom::new()
            .fill(&mut key_bytes)
            .map_err(|_| anyhow!("failed to generate random key"))?;
        Self::from_bytes(&key_bytes)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != 32 {
            return Err(anyhow!("master key must be 32 bytes"));
        }
        let unbound =
            UnboundKey::new(&AES_256_GCM, bytes).map_err(|_| anyhow!("create unbound key"))?;
        Ok(Self {
            key: LessSafeKey::new(unbound),
        })
    }

    pub fn encrypt(&self, plaintext: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::assume_unique_for_key(nonce_bytes);
        let mut in_out = plaintext.to_vec();
        self.key
            .seal_in_place_append_tag(nonce, Aad::empty(), &mut in_out)
            .map_err(|_| anyhow!("encryption failed"))?;
        Ok((nonce_bytes.to_vec(), in_out))
    }

    pub fn decrypt(&self, nonce: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>> {
        if nonce.len() != 12 {
            return Err(anyhow!("invalid nonce length"));
        }
        let nonce =
            Nonce::try_assume_unique_for_key(nonce).map_err(|_| anyhow!("invalid nonce value"))?;
        let mut buffer = ciphertext.to_vec();
        let plaintext = self
            .key
            .open_in_place(nonce, Aad::empty(), &mut buffer)
            .map_err(|_| anyhow!("decryption failed"))?;
        Ok(plaintext.to_vec())
    }

    pub fn encrypt_to_base64(&self, plaintext: &[u8]) -> Result<String> {
        let (nonce, cipher) = self.encrypt(plaintext)?;
        let mut payload = nonce;
        payload.extend_from_slice(&cipher);
        Ok(BASE64.encode(payload))
    }

    pub fn decrypt_from_base64(&self, encoded: &str) -> Result<Vec<u8>> {
        let bytes = BASE64
            .decode(encoded)
            .map_err(|_| anyhow!("invalid base64 ciphertext"))?;
        if bytes.len() < 13 {
            return Err(anyhow!("ciphertext too short"));
        }
        let (nonce, cipher) = bytes.split_at(12);
        self.decrypt(nonce, cipher)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_encryption_and_base64() {
        let manager = KeyManager::from_bytes(&[7u8; 32]).expect("create key manager");
        let message = b"secret payload";

        let (nonce, ciphertext) = manager.encrypt(message).expect("encrypt");
        let decrypted = manager
            .decrypt(&nonce, &ciphertext)
            .expect("decrypt nonce/cipher");
        assert_eq!(decrypted, message);

        let encoded = manager
            .encrypt_to_base64(message)
            .expect("encode to base64");
        let decoded = manager
            .decrypt_from_base64(&encoded)
            .expect("decode from base64");
        assert_eq!(decoded, message);
    }
}
