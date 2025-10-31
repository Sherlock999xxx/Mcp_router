use std::{env, sync::Arc};

use anyhow::{anyhow, Context};
use base64::{engine::general_purpose::STANDARD, Engine};
use ring::{
    aead::{self, Aad, LessSafeKey, Nonce, UnboundKey},
    rand::{SecureRandom, SystemRandom},
};

const ENV_MASTER_KEY: &str = "MCP_STACK_MASTER_KEY";
const NONCE_LEN: usize = 12;

#[derive(Clone)]
pub struct Encryptor {
    key: Arc<LessSafeKey>,
    rng: SystemRandom,
}

impl Encryptor {
    pub fn from_env() -> anyhow::Result<Self> {
        let rng = SystemRandom::new();
        let key_material = if let Ok(value) = env::var(ENV_MASTER_KEY) {
            let bytes = STANDARD
                .decode(value.trim())
                .context("decode base64 master key")?;
            if bytes.len() != aead::AES_256_GCM.key_len() {
                return Err(anyhow!(
                    "invalid {} length: expected {} bytes",
                    ENV_MASTER_KEY,
                    aead::AES_256_GCM.key_len()
                ));
            }
            bytes
        } else {
            // Fallback for development: generate ephemeral key.
            let mut bytes = vec![0u8; aead::AES_256_GCM.key_len()];
            rng.fill(&mut bytes)
                .map_err(|err| anyhow!("generate ephemeral encryption key: {err}"))?;
            tracing::warn!(
                "{} is not set; generated ephemeral encryption key (secrets will not persist across restarts)",
                ENV_MASTER_KEY
            );
            bytes
        };
        let unbound = UnboundKey::new(&aead::AES_256_GCM, &key_material)
            .map_err(|err| anyhow!("construct AES-256-GCM key: {err}"))?;
        Ok(Self {
            key: Arc::new(LessSafeKey::new(unbound)),
            rng,
        })
    }

    pub fn encrypt(&self, plaintext: &[u8]) -> anyhow::Result<String> {
        let mut nonce_bytes = [0u8; NONCE_LEN];
        self.rng
            .fill(&mut nonce_bytes)
            .map_err(|err| anyhow!("generate encryption nonce: {err}"))?;
        let nonce = Nonce::assume_unique_for_key(nonce_bytes);
        let mut in_out = plaintext.to_vec();
        self.key
            .seal_in_place_append_tag(nonce, Aad::empty(), &mut in_out)
            .map_err(|err| anyhow!("encrypt provider secret: {err}"))?;
        let mut payload = Vec::with_capacity(NONCE_LEN + in_out.len());
        payload.extend_from_slice(&nonce_bytes);
        payload.extend_from_slice(&in_out);
        Ok(STANDARD.encode(payload))
    }

    pub fn decrypt(&self, ciphertext: &str) -> anyhow::Result<Vec<u8>> {
        let payload = STANDARD
            .decode(ciphertext.trim())
            .context("decode encrypted payload")?;
        if payload.len() < NONCE_LEN {
            return Err(anyhow!("ciphertext too short"));
        }
        let (nonce_bytes, data) = payload.split_at(NONCE_LEN);
        let nonce = Nonce::assume_unique_for_key({
            let mut bytes = [0u8; NONCE_LEN];
            bytes.copy_from_slice(nonce_bytes);
            bytes
        });
        let mut buffer = data.to_vec();
        let decrypted = self
            .key
            .open_in_place(nonce, Aad::empty(), &mut buffer)
            .map_err(|err| anyhow!("decrypt provider secret: {err}"))?;
        Ok(decrypted.to_vec())
    }
}
