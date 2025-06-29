use crate::config::Config;
use crate::llm::{AnthropicClient, LLMProvider, OpenAIClient};
use crate::services::{ConversationService, TokenMeterService, UserService};
use anyhow::Result;
use dashmap::DashMap;
use redis::aio::ConnectionManager;
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct AppState {
    pub config: Config,
    pub db: PgPool,
    pub redis: ConnectionManager,
    pub openai_client: Arc<OpenAIClient>,
    pub anthropic_client: Arc<AnthropicClient>,
    pub user_service: Arc<UserService>,
    pub conversation_service: Arc<ConversationService>,
    pub token_meter_service: Arc<TokenMeterService>,
    pub active_sessions: DashMap<String, SessionState>,
    pub model_status: Arc<RwLock<ModelStatusCache>>,
}

#[derive(Clone)]
pub struct SessionState {
    pub user_id: String,
    pub conversation_id: Option<String>,
    pub last_activity: chrono::DateTime<chrono::Utc>,
    pub provider: LLMProvider,
}

#[derive(Default)]
pub struct ModelStatusCache {
    pub openai_models: Vec<ModelInfo>,
    pub anthropic_models: Vec<ModelInfo>,
    pub last_updated: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Clone, Debug)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub available: bool,
    pub max_tokens: u32,
    pub supports_streaming: bool,
    pub supports_functions: bool,
}

impl AppState {
    pub async fn new(config: Config) -> Result<Self> {
        // Validate configuration
        config.validate()?;
        
        // Initialize database pool
        let db = PgPoolOptions::new()
            .max_connections(32)
            .connect(&config.database_url)
            .await?;
        
        // Run migrations
        sqlx::migrate!("./migrations").run(&db).await?;
        
        // Initialize Redis connection
        let redis_client = redis::Client::open(config.redis_url.as_str())?;
        let redis = ConnectionManager::new(redis_client).await?;
        
        // Initialize LLM clients
        let openai_client = Arc::new(OpenAIClient::new(
            config.openai_api_key.clone(),
            config.openai_org_id.clone(),
            config.openai_base_url.clone(),
        ));
        
        let anthropic_client = Arc::new(AnthropicClient::new(
            config.anthropic_api_key.clone(),
            config.anthropic_base_url.clone(),
        ));
        
        // Initialize services
        let user_service = Arc::new(UserService::new(db.clone()));
        let conversation_service = Arc::new(ConversationService::new(db.clone(), redis.clone()));
        let token_meter_service = Arc::new(TokenMeterService::new(
            db.clone(),
            redis.clone(),
            config.max_tokens_per_day,
            config.max_tokens_per_month,
        ));
        
        Ok(Self {
            config,
            db,
            redis,
            openai_client,
            anthropic_client,
            user_service,
            conversation_service,
            token_meter_service,
            active_sessions: DashMap::new(),
            model_status: Arc::new(RwLock::new(ModelStatusCache::default())),
        })
    }
    
    pub async fn refresh_model_status(&self) -> Result<()> {
        let mut status = self.model_status.write().await;
        
        // Fetch OpenAI models
        let openai_models = self.openai_client.list_models().await?;
        status.openai_models = openai_models.into_iter().map(|m| ModelInfo {
            id: m.id.clone(),
            name: m.name,
            available: true,
            max_tokens: self.get_model_max_tokens(&m.id),
            supports_streaming: true,
            supports_functions: m.id.contains("gpt-4") || m.id.contains("gpt-3.5-turbo"),
        }).collect();
        
        // Fetch Anthropic models (static list for now)
        status.anthropic_models = vec![
            ModelInfo {
                id: "claude-3-opus-20240229".to_string(),
                name: "Claude 3 Opus".to_string(),
                available: true,
                max_tokens: 4096,
                supports_streaming: true,
                supports_functions: false,
            },
            ModelInfo {
                id: "claude-3-sonnet-20240229".to_string(),
                name: "Claude 3 Sonnet".to_string(),
                available: true,
                max_tokens: 4096,
                supports_streaming: true,
                supports_functions: false,
            },
            ModelInfo {
                id: "claude-3-haiku-20240307".to_string(),
                name: "Claude 3 Haiku".to_string(),
                available: true,
                max_tokens: 4096,
                supports_streaming: true,
                supports_functions: false,
            },
        ];
        
        status.last_updated = Some(chrono::Utc::now());
        
        Ok(())
    }
    
    fn get_model_max_tokens(&self, model_id: &str) -> u32 {
        match model_id {
            "gpt-4-turbo-preview" | "gpt-4-0125-preview" => 128000,
            "gpt-4" | "gpt-4-0613" => 8192,
            "gpt-4-32k" | "gpt-4-32k-0613" => 32768,
            "gpt-3.5-turbo" | "gpt-3.5-turbo-0125" => 16385,
            "o3" if self.config.enable_o3_model => 200000, // Hypothetical O3 model
            _ => 4096,
        }
    }
}