use anyhow::{Context, Result};
use sqlx::SqlitePool;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct UserRow {
    pub id: i64,
    pub email: String,
    pub password_hash: Option<String>,
    pub name: String,
    pub phone: Option<String>,
    pub role: String,
    pub oauth_provider: Option<String>,
    pub oauth_id: Option<String>,
    pub created_at: String,
}

pub async fn create_user(
    pool: &SqlitePool,
    email: &str,
    password_hash: Option<&str>,
    name: &str,
    phone: Option<&str>,
    role: &str,
    oauth_provider: Option<&str>,
    oauth_id: Option<&str>,
) -> Result<i64> {
    let id = sqlx::query_scalar(
        "INSERT INTO users (email, password_hash, name, phone, role, oauth_provider, oauth_id)
         VALUES (?, ?, ?, ?, ?, ?, ?)
         RETURNING id",
    )
    .bind(email)
    .bind(password_hash)
    .bind(name)
    .bind(phone)
    .bind(role)
    .bind(oauth_provider)
    .bind(oauth_id)
    .fetch_one(pool)
    .await
    .context("Failed to create user")?;

    Ok(id)
}

pub async fn get_user_by_email(pool: &SqlitePool, email: &str) -> Result<Option<UserRow>> {
    let user = sqlx::query_as::<_, UserRow>("SELECT * FROM users WHERE email = ?")
        .bind(email)
        .fetch_optional(pool)
        .await
        .context("Failed to get user by email")?;
    Ok(user)
}

pub async fn get_user_by_id(pool: &SqlitePool, id: i64) -> Result<Option<UserRow>> {
    let user = sqlx::query_as::<_, UserRow>("SELECT * FROM users WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("Failed to get user by id")?;
    Ok(user)
}

pub async fn get_user_by_oauth(
    pool: &SqlitePool,
    provider: &str,
    oauth_id: &str,
) -> Result<Option<UserRow>> {
    let user = sqlx::query_as::<_, UserRow>(
        "SELECT * FROM users WHERE oauth_provider = ? AND oauth_id = ?",
    )
    .bind(provider)
    .bind(oauth_id)
    .fetch_optional(pool)
    .await
    .context("Failed to get user by oauth")?;
    Ok(user)
}

pub async fn list_users(pool: &SqlitePool) -> Result<Vec<UserRow>> {
    let users = sqlx::query_as::<_, UserRow>("SELECT * FROM users ORDER BY id")
        .fetch_all(pool)
        .await
        .context("Failed to list users")?;
    Ok(users)
}

pub async fn update_user_role(pool: &SqlitePool, user_id: i64, role: &str) -> Result<()> {
    sqlx::query("UPDATE users SET role = ? WHERE id = ?")
        .bind(role)
        .bind(user_id)
        .execute(pool)
        .await
        .context("Failed to update user role")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{create_memory_pool, run_migrations};

    async fn setup() -> SqlitePool {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        pool
    }

    #[tokio::test]
    async fn create_and_get_user() {
        let pool = setup().await;

        let id = create_user(
            &pool,
            "alice@example.com",
            Some("hash123"),
            "Alice",
            Some("+1234"),
            "customer",
            None,
            None,
        )
        .await
        .unwrap();

        let user = get_user_by_email(&pool, "alice@example.com")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(user.id, id);
        assert_eq!(user.name, "Alice");
        assert_eq!(user.role, "customer");
        assert_eq!(user.phone.as_deref(), Some("+1234"));

        let user2 = get_user_by_id(&pool, id).await.unwrap().unwrap();
        assert_eq!(user2.email, "alice@example.com");
    }

    #[tokio::test]
    async fn duplicate_email_fails() {
        let pool = setup().await;

        create_user(
            &pool,
            "bob@example.com",
            None,
            "Bob",
            None,
            "customer",
            None,
            None,
        )
        .await
        .unwrap();

        let result = create_user(
            &pool,
            "bob@example.com",
            None,
            "Bob2",
            None,
            "customer",
            None,
            None,
        )
        .await;
        assert!(result.is_err());
    }
}
