use std::{collections::HashMap, sync::Arc};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Pool, Row, Sqlite};
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
    pub fn new(pool: Pool<Sqlite>) -> Self {
        Self {
            pool,
            cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn pool(&self) -> Pool<Sqlite> {
        self.pool.clone()
    }

    pub async fn issue_token(&self, user_id: &str, tier: Tier) -> anyhow::Result<String> {
        let token = Uuid::new_v4().to_string();
        sqlx::query(r#"INSERT INTO api_tokens (id, user_id, token, scope) VALUES (?, ?, ?, ?)"#)
            .bind(Uuid::new_v4().to_string())
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
        let record = SubscriptionRecord {
            user_id: user_id.to_string(),
            tier: match row.get::<String, _>("tier").as_str() {
                "pro" => Tier::Pro,
                "enterprise" => Tier::Enterprise,
                _ => Tier::Basic,
            },
            expires_at: row
                .get::<Option<String>, _>("expires_at")
                .and_then(|ts| DateTime::parse_from_rfc3339(&ts).ok())
                .map(|dt| dt.with_timezone(&Utc)),
            max_tokens: row.get::<i64, _>("max_tokens"),
            max_requests: row.get::<i64, _>("max_requests"),
            max_concurrent: row.get::<i32, _>("max_concurrent"),
            tokens_used: row.get::<i64, _>("tokens_used"),
            requests_used: row.get::<i64, _>("requests_used"),
        };
        self.cache
            .lock()
            .await
            .insert(user_id.to_string(), record.clone());
        Ok(Some(record))
    }

    pub async fn record_usage(
        &self,
        user_id: &str,
        provider: &str,
        tokens: i64,
    ) -> anyhow::Result<()> {
        sqlx::query(
            r#"UPDATE subscriptions SET tokens_used = tokens_used + ?, requests_used = requests_used + 1 WHERE user_id = ?"#,
        )
        .bind(tokens)
        .bind(user_id)
        .execute(&self.pool)
        .await?;
        sqlx::query(r#"INSERT INTO usage_counters (provider, user_id, tokens) VALUES (?, ?, ?)"#)
            .bind(provider)
            .bind(user_id)
            .bind(tokens)
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

    pub async fn list_users(&self) -> anyhow::Result<Vec<UserRecord>> {
        let rows = sqlx::query(r#"SELECT id, email, created_at FROM users ORDER BY email"#)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .into_iter()
            .map(|row| UserRecord {
                id: row.get::<String, _>("id"),
                email: row.get::<Option<String>, _>("email").unwrap_or_default(),
                created_at: row.get::<Option<String>, _>("created_at"),
            })
            .collect())
    }

    pub async fn create_user(&self, email: &str) -> anyhow::Result<UserRecord> {
        sqlx::query(r#"INSERT INTO users (id, email) VALUES (?, ?) ON CONFLICT(email) DO NOTHING"#)
            .bind(Uuid::new_v4().to_string())
            .bind(email)
            .execute(&self.pool)
            .await?;
        let row = sqlx::query(r#"SELECT id, email, created_at FROM users WHERE email = ?"#)
            .bind(email)
            .fetch_one(&self.pool)
            .await?;
        Ok(UserRecord {
            id: row.get::<String, _>("id"),
            email: row
                .get::<Option<String>, _>("email")
                .unwrap_or_else(|| email.to_string()),
            created_at: row.get::<Option<String>, _>("created_at"),
        })
    }

    pub async fn list_subscriptions(&self) -> anyhow::Result<Vec<SubscriptionRecord>> {
        let rows = sqlx::query(
            r#"SELECT user_id, tier, expires_at, max_tokens, max_requests, max_concurrent, tokens_used, requests_used FROM subscriptions"#,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| SubscriptionRecord {
                user_id: row.get::<String, _>("user_id"),
                tier: match row.get::<String, _>("tier").as_str() {
                    "pro" => Tier::Pro,
                    "enterprise" => Tier::Enterprise,
                    _ => Tier::Basic,
                },
                expires_at: row
                    .get::<Option<String>, _>("expires_at")
                    .and_then(|ts| DateTime::parse_from_rfc3339(&ts).ok())
                    .map(|dt| dt.with_timezone(&Utc)),
                max_tokens: row.get::<i64, _>("max_tokens"),
                max_requests: row.get::<i64, _>("max_requests"),
                max_concurrent: row.get::<i32, _>("max_concurrent"),
                tokens_used: row.get::<i64, _>("tokens_used"),
                requests_used: row.get::<i64, _>("requests_used"),
            })
            .collect())
    }

    pub async fn set_subscription(
        &self,
        user_id: &str,
        tier: Tier,
        expires_at: Option<DateTime<Utc>>,
        max_tokens: i64,
        max_requests: i64,
        max_concurrent: i32,
    ) -> anyhow::Result<()> {
        let expires = expires_at.map(|dt| dt.to_rfc3339());
        sqlx::query(
            r#"INSERT INTO subscriptions (user_id, tier, expires_at, max_tokens, max_requests, max_concurrent, tokens_used, requests_used)
                VALUES (?, ?, ?, ?, ?, ?, 0, 0)
                ON CONFLICT(user_id) DO UPDATE SET
                    tier=excluded.tier,
                    expires_at=excluded.expires_at,
                    max_tokens=excluded.max_tokens,
                    max_requests=excluded.max_requests,
                    max_concurrent=excluded.max_concurrent"#,
        )
        .bind(user_id)
        .bind(tier.as_str())
        .bind(expires)
        .bind(max_tokens)
        .bind(max_requests)
        .bind(max_concurrent)
        .execute(&self.pool)
        .await?;
        self.cache.lock().await.remove(user_id);
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserRecord {
    pub id: String,
    pub email: String,
    pub created_at: Option<String>,
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
