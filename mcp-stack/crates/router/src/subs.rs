use std::{collections::HashMap, sync::Arc};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rand::{distributions::Alphanumeric, Rng};
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, DisplayFromStr};
use sqlx::{sqlite::SqlitePoolOptions, FromRow, Pool, Row, Sqlite};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::crypto::KeyManager;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    Basic,
    Pro,
    Enterprise,
}

impl Tier {
    pub fn as_str(&self) -> &'static str {
        match self {
            Tier::Basic => "basic",
            Tier::Pro => "pro",
            Tier::Enterprise => "enterprise",
        }
    }

    pub fn from_str(value: &str) -> Self {
        match value {
            "pro" => Tier::Pro,
            "enterprise" => Tier::Enterprise,
            _ => Tier::Basic,
        }
    }
}

impl std::fmt::Display for Tier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for Tier {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Tier::from_str(s))
    }
}

#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionRecord {
    pub user_id: String,
    #[serde_as(as = "DisplayFromStr")]
    pub tier: Tier,
    pub expires_at: Option<DateTime<Utc>>,
    pub max_tokens: i64,
    pub max_requests: i64,
    pub max_concurrent: i32,
    pub tokens_used: i64,
    pub requests_used: i64,
}

impl SubscriptionRecord {
    pub fn check_quota(&self, tokens: i64) -> Result<(), EnforcementError> {
        if let Some(expiry) = self.expires_at {
            if expiry < Utc::now() {
                return Err(EnforcementError::Expired);
            }
        }
        if self.requests_used >= self.max_requests {
            return Err(EnforcementError::RequestsExceeded);
        }
        if self.tokens_used + tokens > self.max_tokens {
            return Err(EnforcementError::TokensExceeded);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct UserRecord {
    pub id: String,
    pub email: String,
    pub name: Option<String>,
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ApiTokenRecord {
    pub id: String,
    pub user_id: String,
    pub token: String,
    pub scope: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ProviderRecord {
    pub id: String,
    pub slug: String,
    pub display_name: String,
    pub description: Option<String>,
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ProviderKeyRecord {
    pub provider_id: String,
    pub name: String,
    pub encrypted_value: Vec<u8>,
    pub nonce: Vec<u8>,
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct UpstreamRecord {
    pub name: String,
    pub kind: String,
    pub command: Option<String>,
    pub args: Option<String>,
    pub url: Option<String>,
    pub bearer: Option<String>,
    pub provider_slug: Option<String>,
}

impl UpstreamRecord {
    pub fn with_args_vec(mut self, args: &[String]) -> Self {
        if args.is_empty() {
            self.args = None;
        } else {
            self.args = Some(serde_json::to_string(args).unwrap_or_default());
        }
        self
    }

    pub fn args_vec(&self) -> Vec<String> {
        self.args
            .as_ref()
            .and_then(|raw| serde_json::from_str::<Vec<String>>(raw).ok())
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewProvider {
    pub slug: String,
    pub display_name: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionPreset {
    pub name: Tier,
    pub max_tokens: i64,
    pub max_requests: i64,
    pub max_concurrent: i32,
}

impl SubscriptionPreset {
    pub fn defaults() -> Vec<Self> {
        vec![
            Self {
                name: Tier::Basic,
                max_tokens: 100_000,
                max_requests: 1_000,
                max_concurrent: 1,
            },
            Self {
                name: Tier::Pro,
                max_tokens: 1_000_000,
                max_requests: 10_000,
                max_concurrent: 3,
            },
            Self {
                name: Tier::Enterprise,
                max_tokens: 10_000_000,
                max_requests: 100_000,
                max_concurrent: 10,
            },
        ]
    }
}

#[derive(Clone)]
pub struct SubscriptionStore {
    pool: Pool<Sqlite>,
    cache: Arc<RwLock<HashMap<String, SubscriptionRecord>>>,
    crypto: Arc<KeyManager>,
}

impl SubscriptionStore {
    pub async fn new(database_url: &str, crypto: Arc<KeyManager>) -> Result<Self> {
        let pool = SqlitePoolOptions::new()
            .max_connections(10)
            .connect(database_url)
            .await
            .context("connect sqlite")?;
        sqlx::migrate!("../../migrations").run(&pool).await?;
        Ok(Self {
            pool,
            cache: Arc::new(RwLock::new(HashMap::new())),
            crypto,
        })
    }

    pub fn pool(&self) -> &Pool<Sqlite> {
        &self.pool
    }

    pub fn crypto(&self) -> Arc<KeyManager> {
        self.crypto.clone()
    }

    pub async fn issue_token(&self, user_id: &str, scope: &str) -> Result<ApiTokenRecord> {
        let token_value: String = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(48)
            .map(char::from)
            .collect();
        let id = Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO api_tokens (id, user_id, token, scope) VALUES (?1, ?2, ?3, ?4)")
            .bind(&id)
            .bind(user_id)
            .bind(&token_value)
            .bind(scope)
            .execute(&self.pool)
            .await?;
        Ok(ApiTokenRecord {
            id,
            user_id: user_id.to_string(),
            token: token_value,
            scope: scope.to_string(),
        })
    }

    pub async fn list_tokens(&self, user_id: Option<&str>) -> Result<Vec<ApiTokenRecord>> {
        let rows = if let Some(user_id) = user_id {
            sqlx::query_as::<_, ApiTokenRecord>(
                "SELECT id, user_id, token, scope FROM api_tokens WHERE user_id = ?1",
            )
            .bind(user_id)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query_as::<_, ApiTokenRecord>("SELECT id, user_id, token, scope FROM api_tokens")
                .fetch_all(&self.pool)
                .await?
        };
        Ok(rows)
    }

    pub async fn get_subscription(&self, user_id: &str) -> Result<Option<SubscriptionRecord>> {
        if let Some(record) = self.cache.read().await.get(user_id).cloned() {
            return Ok(Some(record));
        }
        let row = sqlx::query(
            "SELECT user_id, tier, expires_at, max_tokens, max_requests, max_concurrent, tokens_used, requests_used FROM subscriptions WHERE user_id = ?1",
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let tier: String = row.try_get("tier")?;
        let record = SubscriptionRecord {
            user_id: row.try_get("user_id")?,
            tier: Tier::from_str(&tier),
            expires_at: row
                .try_get::<Option<String>, _>("expires_at")?
                .and_then(|ts| DateTime::parse_from_rfc3339(&ts).ok())
                .map(|dt| dt.with_timezone(&Utc)),
            max_tokens: row.try_get("max_tokens")?,
            max_requests: row.try_get("max_requests")?,
            max_concurrent: row.try_get("max_concurrent")?,
            tokens_used: row.try_get("tokens_used")?,
            requests_used: row.try_get("requests_used")?,
        };
        self.cache
            .write()
            .await
            .insert(user_id.to_string(), record.clone());
        Ok(Some(record))
    }

    pub async fn upsert_subscription(
        &self,
        user_id: &str,
        tier: Tier,
        expires_at: Option<DateTime<Utc>>,
        quotas: Option<(i64, i64, i32)>,
    ) -> Result<SubscriptionRecord> {
        let preset = quotas.unwrap_or_else(|| match tier {
            Tier::Basic => (100_000, 1_000, 1),
            Tier::Pro => (1_000_000, 10_000, 3),
            Tier::Enterprise => (10_000_000, 100_000, 10),
        });
        sqlx::query(
            "INSERT INTO subscriptions (user_id, tier, expires_at, max_tokens, max_requests, max_concurrent, tokens_used, requests_used)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6,
                     COALESCE((SELECT tokens_used FROM subscriptions WHERE user_id = ?1), 0),
                     COALESCE((SELECT requests_used FROM subscriptions WHERE user_id = ?1), 0))
             ON CONFLICT(user_id) DO UPDATE SET tier = excluded.tier, expires_at = excluded.expires_at,
                     max_tokens = excluded.max_tokens, max_requests = excluded.max_requests, max_concurrent = excluded.max_concurrent",
        )
        .bind(user_id)
        .bind(tier.as_str())
        .bind(expires_at.map(|ts| ts.to_rfc3339()))
        .bind(preset.0)
        .bind(preset.1)
        .bind(preset.2)
        .execute(&self.pool)
        .await?;
        self.cache.write().await.remove(user_id);
        let record = self
            .get_subscription(user_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("failed to load subscription after upsert"))?;
        Ok(record)
    }

    pub async fn record_usage(&self, user_id: &str, tokens: i64, provider: &str) -> Result<()> {
        sqlx::query(
            "UPDATE subscriptions SET tokens_used = tokens_used + ?1, requests_used = requests_used + 1 WHERE user_id = ?2",
        )
        .bind(tokens)
        .bind(user_id)
        .execute(&self.pool)
        .await?;
        sqlx::query("INSERT INTO usage_counters (provider, user_id, tokens) VALUES (?1, ?2, ?3)")
            .bind(provider)
            .bind(user_id)
            .bind(tokens)
            .execute(&self.pool)
            .await?;
        self.cache.write().await.remove(user_id);
        Ok(())
    }

    pub async fn ensure_user(&self, email: &str, name: Option<&str>) -> Result<UserRecord> {
        let id = Uuid::new_v4().to_string();
        sqlx::query("INSERT OR IGNORE INTO users (id, email, name) VALUES (?1, ?2, ?3)")
            .bind(&id)
            .bind(email)
            .bind(name)
            .execute(&self.pool)
            .await?;
        let user = sqlx::query_as::<_, UserRecord>(
            "SELECT id, email, name, created_at FROM users WHERE email = ?1",
        )
        .bind(email)
        .fetch_one(&self.pool)
        .await?;
        Ok(user)
    }

    pub async fn list_users(&self) -> Result<Vec<UserRecord>> {
        let users =
            sqlx::query_as::<_, UserRecord>("SELECT id, email, name, created_at FROM users")
                .fetch_all(&self.pool)
                .await?;
        Ok(users)
    }

    pub async fn put_provider(&self, provider: &NewProvider) -> Result<ProviderRecord> {
        let id = Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO providers (id, slug, display_name, description) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(slug) DO UPDATE SET display_name = excluded.display_name, description = excluded.description",
        )
        .bind(&id)
        .bind(&provider.slug)
        .bind(&provider.display_name)
        .bind(&provider.description)
        .execute(&self.pool)
        .await?;
        let record = sqlx::query_as::<_, ProviderRecord>(
            "SELECT id, slug, display_name, description, created_at FROM providers WHERE slug = ?1",
        )
        .bind(&provider.slug)
        .fetch_one(&self.pool)
        .await?;
        Ok(record)
    }

    pub async fn list_providers(&self) -> Result<Vec<ProviderRecord>> {
        let providers = sqlx::query_as::<_, ProviderRecord>(
            "SELECT id, slug, display_name, description, created_at FROM providers",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(providers)
    }

    pub async fn store_provider_key(
        &self,
        provider_slug: &str,
        name: &str,
        value: &[u8],
    ) -> Result<()> {
        let provider: ProviderRecord = sqlx::query_as(
            "SELECT id, slug, display_name, description, created_at FROM providers WHERE slug = ?1",
        )
        .bind(provider_slug)
        .fetch_one(&self.pool)
        .await?;
        let (nonce, encrypted) = self.crypto.encrypt(value)?;
        sqlx::query(
            "INSERT INTO provider_keys (provider_id, name, encrypted_value, nonce) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(provider_id, name) DO UPDATE SET encrypted_value = excluded.encrypted_value, nonce = excluded.nonce",
        )
        .bind(&provider.id)
        .bind(name)
        .bind(encrypted)
        .bind(nonce)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn fetch_provider_key(
        &self,
        provider_slug: &str,
        name: &str,
    ) -> Result<Option<Vec<u8>>> {
        let row = sqlx::query_as::<_, ProviderKeyRecord>(
            "SELECT provider_id, name, encrypted_value, nonce, created_at FROM provider_keys
             WHERE provider_id = (SELECT id FROM providers WHERE slug = ?1 LIMIT 1) AND name = ?2",
        )
        .bind(provider_slug)
        .bind(name)
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let plaintext = self.crypto.decrypt(&row.nonce, &row.encrypted_value)?;
        Ok(Some(plaintext))
    }

    pub async fn upsert_upstream(&self, upstream: &UpstreamRecord) -> Result<()> {
        sqlx::query(
            "INSERT INTO upstreams (name, kind, command, args, url, bearer, provider_slug)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(name) DO UPDATE SET kind = excluded.kind, command = excluded.command,
                 args = excluded.args, url = excluded.url, bearer = excluded.bearer, provider_slug = excluded.provider_slug",
        )
        .bind(&upstream.name)
        .bind(&upstream.kind)
        .bind(&upstream.command)
        .bind(&upstream.args)
        .bind(&upstream.url)
        .bind(&upstream.bearer)
        .bind(&upstream.provider_slug)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_upstreams(&self) -> Result<Vec<UpstreamRecord>> {
        let records = sqlx::query_as::<_, UpstreamRecord>(
            "SELECT name, kind, command, args, url, bearer, provider_slug FROM upstreams",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(records)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EnforcementError {
    #[error("no active subscription")]
    NoSubscription,
    #[error("subscription expired")]
    Expired,
    #[error("request quota exceeded")]
    RequestsExceeded,
    #[error("token quota exceeded")]
    TokensExceeded,
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use chrono::Duration;
    use std::sync::Arc;

    #[test]
    fn subscription_quota_enforcement() {
        let record = SubscriptionRecord {
            user_id: "user".into(),
            tier: Tier::Pro,
            expires_at: Some(Utc::now() + Duration::minutes(5)),
            max_tokens: 100,
            max_requests: 2,
            max_concurrent: 1,
            tokens_used: 50,
            requests_used: 1,
        };
        assert!(record.check_quota(25).is_ok());
        let err = record.check_quota(60).expect_err("tokens exceeded");
        assert!(matches!(err, EnforcementError::TokensExceeded));
    }

    #[tokio::test]
    async fn store_round_trip_flow() -> Result<()> {
        let manager = Arc::new(KeyManager::from_bytes(&[9u8; 32])?);
        let store = SubscriptionStore::new("sqlite::memory:?cache=shared", manager.clone()).await?;

        let provider = store
            .put_provider(&NewProvider {
                slug: "openai".into(),
                display_name: "OpenAI".into(),
                description: Some("LLM provider".into()),
            })
            .await?;

        store
            .store_provider_key(&provider.slug, "api_key", b"super-secret")
            .await?;
        let key = store
            .fetch_provider_key(&provider.slug, "api_key")
            .await?
            .expect("key stored");
        assert_eq!(key, b"super-secret");

        let user = store
            .ensure_user("tester@example.com", Some("Tester"))
            .await?;
        let subscription = store
            .upsert_subscription(&user.id, Tier::Basic, None, Some((200, 5, 1)))
            .await?;
        assert_eq!(subscription.tokens_used, 0);

        store.record_usage(&user.id, 42, &provider.slug).await?;
        let updated = store
            .get_subscription(&user.id)
            .await?
            .expect("subscription exists");
        assert_eq!(updated.tokens_used, 42);
        assert_eq!(updated.requests_used, 1);

        let token = store.issue_token(&user.id, "default").await?;
        let tokens = store.list_tokens(None).await?;
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].id, token.id);

        store
            .upsert_upstream(
                &UpstreamRecord {
                    name: "demo".into(),
                    kind: "http".into(),
                    command: None,
                    args: Some("[]".into()),
                    url: Some("http://localhost".into()),
                    bearer: None,
                    provider_slug: Some(provider.slug.clone()),
                }
                .with_args_vec(&["--flag".into()]),
            )
            .await?;
        let upstreams = store.list_upstreams().await?;
        assert_eq!(upstreams.len(), 1);

        Ok(())
    }
}
