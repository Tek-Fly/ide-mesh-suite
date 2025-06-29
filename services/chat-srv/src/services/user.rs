use anyhow::Result;
use sqlx::PgPool;
use uuid::Uuid;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct User {
    pub id: Uuid,
    pub email: String,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub is_active: bool,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateUserRequest {
    pub email: String,
    pub name: String,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ApiKey {
    pub id: Uuid,
    pub user_id: Uuid,
    pub key_hash: String,
    pub name: Option<String>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub is_active: bool,
}

pub struct UserService {
    db: PgPool,
}

impl UserService {
    pub fn new(db: PgPool) -> Self {
        Self { db }
    }
    
    pub async fn create_user(&self, request: CreateUserRequest) -> Result<User> {
        let user = sqlx::query_as::<_, User>(
            r#"
            INSERT INTO users (email, name, metadata)
            VALUES ($1, $2, $3)
            RETURNING *
            "#
        )
        .bind(&request.email)
        .bind(&request.name)
        .bind(request.metadata.unwrap_or(serde_json::json!({})))
        .fetch_one(&self.db)
        .await?;
        
        // Create default rate limits
        sqlx::query(
            r#"
            INSERT INTO rate_limits (user_id)
            VALUES ($1)
            "#
        )
        .bind(&user.id)
        .execute(&self.db)
        .await?;
        
        Ok(user)
    }
    
    pub async fn get_user(&self, user_id: &Uuid) -> Result<Option<User>> {
        let user = sqlx::query_as::<_, User>(
            r#"
            SELECT * FROM users
            WHERE id = $1 AND is_active = true
            "#
        )
        .bind(user_id)
        .fetch_optional(&self.db)
        .await?;
        
        Ok(user)
    }
    
    pub async fn get_user_by_email(&self, email: &str) -> Result<Option<User>> {
        let user = sqlx::query_as::<_, User>(
            r#"
            SELECT * FROM users
            WHERE email = $1 AND is_active = true
            "#
        )
        .bind(email)
        .fetch_optional(&self.db)
        .await?;
        
        Ok(user)
    }
    
    pub async fn validate_api_key(&self, key_hash: &str) -> Result<Option<User>> {
        let result = sqlx::query_as::<_, (Uuid, Option<DateTime<Utc>>)>(
            r#"
            SELECT user_id, expires_at
            FROM api_keys
            WHERE key_hash = $1 AND is_active = true
            "#
        )
        .bind(key_hash)
        .fetch_optional(&self.db)
        .await?;
        
        if let Some((user_id, expires_at)) = result {
            // Check if key is expired
            if let Some(expiry) = expires_at {
                if expiry < Utc::now() {
                    return Ok(None);
                }
            }
            
            // Update last used timestamp
            sqlx::query(
                r#"
                UPDATE api_keys
                SET last_used_at = NOW()
                WHERE key_hash = $1
                "#
            )
            .bind(key_hash)
            .execute(&self.db)
            .await?;
            
            // Get user
            self.get_user(&user_id).await
        } else {
            Ok(None)
        }
    }
    
    pub async fn create_api_key(&self, user_id: &Uuid, name: Option<String>, expires_at: Option<DateTime<Utc>>) -> Result<String> {
        use rand::Rng;
        use argon2::{Argon2, PasswordHasher};
        use argon2::password_hash::{SaltString, rand_core::OsRng};
        
        // Generate random API key
        let mut rng = rand::thread_rng();
        let key_bytes: Vec<u8> = (0..32).map(|_| rng.gen()).collect();
        let api_key = base64::encode(&key_bytes);
        
        // Hash the key
        let salt = SaltString::generate(&mut OsRng);
        let argon2 = Argon2::default();
        let key_hash = argon2.hash_password(api_key.as_bytes(), &salt)?.to_string();
        
        // Store in database
        sqlx::query(
            r#"
            INSERT INTO api_keys (user_id, key_hash, name, expires_at)
            VALUES ($1, $2, $3, $4)
            "#
        )
        .bind(user_id)
        .bind(&key_hash)
        .bind(name)
        .bind(expires_at)
        .execute(&self.db)
        .await?;
        
        Ok(api_key)
    }
    
    pub async fn revoke_api_key(&self, user_id: &Uuid, key_id: &Uuid) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE api_keys
            SET is_active = false
            WHERE id = $1 AND user_id = $2
            "#
        )
        .bind(key_id)
        .bind(user_id)
        .execute(&self.db)
        .await?;
        
        Ok(())
    }
}