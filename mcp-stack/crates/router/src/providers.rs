use std::sync::Arc;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{Pool, Row, Sqlite};
use uuid::Uuid;

use crate::crypto::Encryptor;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderRecord {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub endpoint: Option<String>,
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProviderRequest {
    pub name: String,
    pub kind: String,
    #[serde(default)]
    pub endpoint: Option<String>,
    #[serde(default)]
    pub metadata: Option<Value>,
    #[serde(default)]
    pub api_key: Option<String>,
}

#[derive(Clone)]
pub struct ProviderStore {
    pool: Pool<Sqlite>,
    encryptor: Arc<Encryptor>,
}

impl ProviderStore {
    pub fn new(pool: Pool<Sqlite>, encryptor: Arc<Encryptor>) -> Self {
        Self { pool, encryptor }
    }

    pub fn pool(&self) -> Pool<Sqlite> {
        self.pool.clone()
    }

    pub async fn list(&self) -> anyhow::Result<Vec<ProviderRecord>> {
        let rows = sqlx::query(
            r#"SELECT id, name, kind, endpoint, metadata FROM providers ORDER BY name"#,
        )
        .fetch_all(&self.pool)
        .await?;
        let providers = rows
            .into_iter()
            .map(|row| ProviderRecord {
                id: row.get::<String, _>("id"),
                name: row.get::<String, _>("name"),
                kind: row.get::<String, _>("kind"),
                endpoint: row.get::<Option<String>, _>("endpoint"),
                metadata: row
                    .get::<Option<String>, _>("metadata")
                    .and_then(|value| serde_json::from_str::<Value>(&value).ok()),
            })
            .collect();
        Ok(providers)
    }

    pub async fn upsert(&self, request: ProviderRequest) -> anyhow::Result<ProviderRecord> {
        let provider_id = Uuid::new_v4().to_string();
        let metadata = request
            .metadata
            .clone()
            .map(|value| serde_json::to_string(&value))
            .transpose()?;
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            r#"INSERT INTO providers (id, name, kind, endpoint, metadata)
               VALUES (?, ?, ?, ?, ?)
               ON CONFLICT(name) DO UPDATE SET
                    kind=excluded.kind,
                    endpoint=excluded.endpoint,
                    metadata=excluded.metadata,
                    updated_at = CURRENT_TIMESTAMP"#,
        )
        .bind(&provider_id)
        .bind(&request.name)
        .bind(&request.kind)
        .bind(&request.endpoint)
        .bind(&metadata)
        .execute(&mut *tx)
        .await?;
        let row = sqlx::query(r#"SELECT id FROM providers WHERE name = ?"#)
            .bind(&request.name)
            .fetch_one(&mut *tx)
            .await?;
        let provider_id: String = row.get("id");
        if let Some(api_key) = request.api_key {
            let ciphertext = self
                .encryptor
                .encrypt(api_key.as_bytes())
                .context("encrypt provider api key")?;
            sqlx::query(
                r#"INSERT INTO provider_keys (id, provider_id, name, ciphertext)
                   VALUES (?, ?, ?, ?)
                   ON CONFLICT(provider_id, name) DO UPDATE SET ciphertext=excluded.ciphertext, updated_at=CURRENT_TIMESTAMP"#,
            )
            .bind(Uuid::new_v4().to_string())
            .bind(&provider_id)
            .bind("default")
            .bind(ciphertext)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(ProviderRecord {
            id: provider_id,
            name: request.name,
            kind: request.kind,
            endpoint: request.endpoint,
            metadata: request.metadata,
        })
    }
}
