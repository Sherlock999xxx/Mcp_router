use std::{collections::HashMap, sync::Arc};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{sqlite::SqlitePoolOptions, Pool, Row, Sqlite};
use tokio::sync::Mutex;
use uuid::Uuid;

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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionRecord {
    pub user_id: String,
    pub tier: Tier,
    pub expires_at: Option<DateTime<Utc>>,
    pub max_tokens: i64,
    pub max_requests: i64,
    pub max_concurrent: i32,
    pub tokens_used: i64,
    pub requests_used: i64,
}

#[derive(Clone)]
pub struct SubscriptionStore {
    pool: Pool<Sqlite>,
    cache: Arc<Mutex<HashMap<String, SubscriptionRecord>>>,
}

impl SubscriptionStore {
    pub async fn new(database_url: &str) -> anyhow::Result<Self> {
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect(database_url)
            .await?;
        sqlx::migrate!("../../migrations").run(&pool).await?;
        Ok(Self {
            pool,
            cache: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub async fn issue_token(&self, user_id: &str, tier: Tier) -> anyhow::Result<String> {
        let token = Uuid::new_v4().to_string();
        let id = Uuid::new_v4().to_string();
        sqlx::query(r#"INSERT INTO api_tokens (id, user_id, token, scope) VALUES (?, ?, ?, ?)"#)
            .bind(id)
            .bind(user_id)
            .bind(&token)
            .bind(tier.as_str())
            .execute(&self.pool)
            .await?;
        Ok(token)
    }

    pub async fn get_subscription(
        &self,
        user_id: &str,
    ) -> anyhow::Result<Option<SubscriptionRecord>> {
        if let Some(record) = self.cache.lock().await.get(user_id).cloned() {
            return Ok(Some(record));
        }
        let row = sqlx::query(
            r#"SELECT tier, expires_at, max_tokens, max_requests, max_concurrent, tokens_used, requests_used
            FROM subscriptions WHERE user_id = ?"#,
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let tier: String = row.try_get("tier")?;
        let expires_at: Option<String> = row.try_get("expires_at")?;
        let max_tokens: i64 = row.try_get("max_tokens")?;
        let max_requests: i64 = row.try_get("max_requests")?;
        let max_concurrent: i32 = row.try_get("max_concurrent")?;
        let tokens_used: i64 = row.try_get("tokens_used")?;
        let requests_used: i64 = row.try_get("requests_used")?;
        let record = SubscriptionRecord {
            user_id: user_id.to_string(),
            tier: match tier.as_str() {
                "pro" => Tier::Pro,
                "enterprise" => Tier::Enterprise,
                _ => Tier::Basic,
            },
            expires_at: expires_at
                .as_deref()
                .and_then(|ts| DateTime::parse_from_rfc3339(ts).ok())
                .map(|dt| dt.with_timezone(&Utc)),
            max_tokens,
            max_requests,
            max_concurrent,
            tokens_used,
            requests_used,
        };
        self.cache
            .lock()
            .await
            .insert(user_id.to_string(), record.clone());
        Ok(Some(record))
    }

    pub async fn record_usage(&self, user_id: &str, tokens: i64) -> anyhow::Result<()> {
        sqlx::query(
            r#"UPDATE subscriptions SET tokens_used = tokens_used + ?, requests_used = requests_used + 1 WHERE user_id = ?"#,
        )
        .bind(tokens)
        .bind(user_id)
        .execute(&self.pool)
        .await?;
        self.cache.lock().await.remove(user_id);
        Ok(())
    }

    pub async fn ensure_user(&self, email: &str) -> anyhow::Result<String> {
        let id = Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO users (id, email) VALUES (?, ?) ON CONFLICT(email) DO NOTHING")
            .bind(&id)
            .bind(email)
            .execute(&self.pool)
            .await?;
        Ok(id)
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
