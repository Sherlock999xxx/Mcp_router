use anyhow::Context;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{sqlite::SqlitePoolOptions, SqlitePool};
use uuid::Uuid;

#[derive(Clone)]
pub struct Database {
    pool: SqlitePool,
}

impl Database {
    pub async fn connect(path: &str) -> anyhow::Result<Self> {
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect(&format!("sqlite://{}", path))
            .await
            .with_context(|| format!("failed to open sqlite db at {}", path))?;
        Ok(Self { pool })
    }

    pub async fn run_migrations(&self) -> anyhow::Result<()> {
        sqlx::migrate!("../../migrations").run(&self.pool).await?;
        Ok(())
    }

    pub async fn list_users(&self) -> anyhow::Result<Vec<User>> {
        let users = sqlx::query_as::<_, User>(
            "SELECT id, email, display_name, created_at FROM users ORDER BY created_at DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(users)
    }

    pub async fn create_user(
        &self,
        email: String,
        display_name: Option<String>,
    ) -> anyhow::Result<User> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        sqlx::query(
            "INSERT INTO users (id, email, display_name, created_at) VALUES (?1, ?2, ?3, ?4)",
        )
        .bind(&id)
        .bind(&email)
        .bind(&display_name)
        .bind(now)
        .execute(&self.pool)
        .await?;

        Ok(User {
            id,
            email,
            display_name,
            created_at: now,
        })
    }

    pub async fn list_subscriptions(&self) -> anyhow::Result<Vec<Subscription>> {
        let rows = sqlx::query_as::<_, Subscription>(
            "SELECT id, user_id, tier, expires_at, created_at FROM subscriptions ORDER BY created_at DESC"
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn create_subscription(
        &self,
        user_id: String,
        tier: String,
        expires_at: Option<DateTime<Utc>>,
    ) -> anyhow::Result<Subscription> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        sqlx::query(
            "INSERT INTO subscriptions (id, user_id, tier, expires_at, created_at) VALUES (?1, ?2, ?3, ?4, ?5)"
        )
        .bind(&id)
        .bind(&user_id)
        .bind(&tier)
        .bind(expires_at)
        .bind(now)
        .execute(&self.pool)
        .await?;

        Ok(Subscription {
            id,
            user_id,
            tier,
            expires_at,
            created_at: now,
        })
    }

    pub async fn list_providers(&self) -> anyhow::Result<Vec<Provider>> {
        let rows = sqlx::query_as::<_, Provider>(
            "SELECT id, name, kind, created_at FROM providers ORDER BY created_at DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn create_provider(&self, name: String, kind: String) -> anyhow::Result<Provider> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        sqlx::query("INSERT INTO providers (id, name, kind, created_at) VALUES (?1, ?2, ?3, ?4)")
            .bind(&id)
            .bind(&name)
            .bind(&kind)
            .bind(now)
            .execute(&self.pool)
            .await?;

        Ok(Provider {
            id,
            name,
            kind,
            created_at: now,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct User {
    pub id: String,
    pub email: String,
    pub display_name: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Subscription {
    pub id: String,
    pub user_id: String,
    pub tier: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub created_at: DateTime<Utc>,
}
