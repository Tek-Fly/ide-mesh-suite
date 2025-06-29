use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::env;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    // Server
    pub host: String,
    pub port: u16,
    
    // Database
    pub database_url: String,
    pub redis_url: String,
    
    // LLM Providers
    pub openai_api_key: String,
    pub openai_org_id: Option<String>,
    pub openai_base_url: Option<String>,
    pub anthropic_api_key: String,
    pub anthropic_base_url: Option<String>,
    
    // Authentication
    pub jwt_secret: String,
    pub jwt_expiry_hours: u64,
    
    // Rate Limiting
    pub rate_limit_requests: u64,
    pub rate_limit_window_secs: u64,
    
    // Token Limits
    pub max_tokens_per_request: u32,
    pub max_tokens_per_day: u64,
    pub max_tokens_per_month: u64,
    
    // Model Configuration
    pub default_openai_model: String,
    pub default_claude_model: String,
    pub enable_o3_model: bool,
    
    // Security
    pub enable_tls: bool,
    pub tls_cert_path: Option<String>,
    pub tls_key_path: Option<String>,
    
    // Monitoring
    pub enable_metrics: bool,
    pub metrics_port: u16,
    pub enable_tracing: bool,
    pub otlp_endpoint: Option<String>,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        dotenvy::dotenv().ok();
        
        Ok(Self {
            host: env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
            port: env::var("PORT")
                .unwrap_or_else(|_| "8080".to_string())
                .parse()
                .context("Invalid PORT")?,
            
            database_url: env::var("DATABASE_URL")
                .context("DATABASE_URL is required")?,
            redis_url: env::var("REDIS_URL")
                .unwrap_or_else(|_| "redis://localhost:6379".to_string()),
            
            openai_api_key: env::var("OPENAI_API_KEY")
                .context("OPENAI_API_KEY is required")?,
            openai_org_id: env::var("OPENAI_ORG_ID").ok(),
            openai_base_url: env::var("OPENAI_BASE_URL").ok(),
            anthropic_api_key: env::var("ANTHROPIC_API_KEY")
                .context("ANTHROPIC_API_KEY is required")?,
            anthropic_base_url: env::var("ANTHROPIC_BASE_URL").ok(),
            
            jwt_secret: env::var("JWT_SECRET")
                .context("JWT_SECRET is required")?,
            jwt_expiry_hours: env::var("JWT_EXPIRY_HOURS")
                .unwrap_or_else(|_| "24".to_string())
                .parse()
                .context("Invalid JWT_EXPIRY_HOURS")?,
            
            rate_limit_requests: env::var("RATE_LIMIT_REQUESTS")
                .unwrap_or_else(|_| "100".to_string())
                .parse()
                .context("Invalid RATE_LIMIT_REQUESTS")?,
            rate_limit_window_secs: env::var("RATE_LIMIT_WINDOW_SECS")
                .unwrap_or_else(|_| "60".to_string())
                .parse()
                .context("Invalid RATE_LIMIT_WINDOW_SECS")?,
            
            max_tokens_per_request: env::var("MAX_TOKENS_PER_REQUEST")
                .unwrap_or_else(|_| "4096".to_string())
                .parse()
                .context("Invalid MAX_TOKENS_PER_REQUEST")?,
            max_tokens_per_day: env::var("MAX_TOKENS_PER_DAY")
                .unwrap_or_else(|_| "1000000".to_string())
                .parse()
                .context("Invalid MAX_TOKENS_PER_DAY")?,
            max_tokens_per_month: env::var("MAX_TOKENS_PER_MONTH")
                .unwrap_or_else(|_| "10000000".to_string())
                .parse()
                .context("Invalid MAX_TOKENS_PER_MONTH")?,
            
            default_openai_model: env::var("DEFAULT_OPENAI_MODEL")
                .unwrap_or_else(|_| "gpt-4-turbo-preview".to_string()),
            default_claude_model: env::var("DEFAULT_CLAUDE_MODEL")
                .unwrap_or_else(|_| "claude-3-opus-20240229".to_string()),
            enable_o3_model: env::var("ENABLE_O3_MODEL")
                .unwrap_or_else(|_| "false".to_string())
                .parse()
                .context("Invalid ENABLE_O3_MODEL")?,
            
            enable_tls: env::var("ENABLE_TLS")
                .unwrap_or_else(|_| "false".to_string())
                .parse()
                .context("Invalid ENABLE_TLS")?,
            tls_cert_path: env::var("TLS_CERT_PATH").ok(),
            tls_key_path: env::var("TLS_KEY_PATH").ok(),
            
            enable_metrics: env::var("ENABLE_METRICS")
                .unwrap_or_else(|_| "true".to_string())
                .parse()
                .context("Invalid ENABLE_METRICS")?,
            metrics_port: env::var("METRICS_PORT")
                .unwrap_or_else(|_| "9090".to_string())
                .parse()
                .context("Invalid METRICS_PORT")?,
            enable_tracing: env::var("ENABLE_TRACING")
                .unwrap_or_else(|_| "true".to_string())
                .parse()
                .context("Invalid ENABLE_TRACING")?,
            otlp_endpoint: env::var("OTLP_ENDPOINT").ok(),
        })
    }
    
    pub fn validate(&self) -> Result<()> {
        if self.enable_tls {
            if self.tls_cert_path.is_none() || self.tls_key_path.is_none() {
                anyhow::bail!("TLS enabled but cert/key paths not provided");
            }
        }
        
        if self.enable_o3_model && self.openai_base_url.is_none() {
            tracing::warn!("O3 model enabled but no custom OpenAI base URL provided");
        }
        
        Ok(())
    }
}