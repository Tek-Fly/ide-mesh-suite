use super::{ChatCompletionRequest, ChatCompletionResponse, ChatMessage, ChatChoice, TokenUsage, LLMClient, LLMError, LLMProvider, Model, ChatStream};
use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures::{Stream, StreamExt};
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::pin::Pin;
use tracing::{error, debug};

const ANTHROPIC_API_BASE: &str = "https://api.anthropic.com";
const ANTHROPIC_API_VERSION: &str = "2023-06-01";

pub struct AnthropicClient {
    client: Client,
    api_key: String,
    base_url: String,
}

#[derive(Debug, Serialize)]
struct AnthropicRequest {
    model: String,
    messages: Vec<AnthropicMessage>,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
struct AnthropicMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct AnthropicResponse {
    id: String,
    model: String,
    content: Vec<AnthropicContent>,
    usage: AnthropicUsage,
}

#[derive(Debug, Deserialize)]
struct AnthropicContent {
    #[serde(rename = "type")]
    content_type: String,
    text: String,
}

#[derive(Debug, Deserialize)]
struct AnthropicUsage {
    input_tokens: u32,
    output_tokens: u32,
}

#[derive(Debug, Deserialize)]
struct StreamEvent {
    #[serde(rename = "type")]
    event_type: String,
    #[serde(flatten)]
    data: serde_json::Value,
}

impl AnthropicClient {
    pub fn new(api_key: String, base_url: Option<String>) -> Self {
        Self {
            client: Client::new(),
            api_key,
            base_url: base_url.unwrap_or_else(|| ANTHROPIC_API_BASE.to_string()),
        }
    }
    
    fn convert_messages(&self, messages: Vec<ChatMessage>) -> Vec<AnthropicMessage> {
        messages.into_iter()
            .filter(|m| m.role != "system") // Anthropic doesn't use system messages in the same way
            .map(|msg| AnthropicMessage {
                role: if msg.role == "assistant" { "assistant".to_string() } else { "user".to_string() },
                content: msg.content,
            })
            .collect()
    }
    
    fn extract_system_prompt(&self, messages: &[ChatMessage]) -> Option<String> {
        messages.iter()
            .find(|m| m.role == "system")
            .map(|m| m.content.clone())
    }
}

#[async_trait]
impl LLMClient for AnthropicClient {
    async fn chat_completion(&self, request: ChatCompletionRequest) -> Result<ChatCompletionResponse, LLMError> {
        let mut anthropic_messages = self.convert_messages(request.messages.clone());
        
        // Handle system prompt
        if let Some(system_prompt) = self.extract_system_prompt(&request.messages) {
            if let Some(first_msg) = anthropic_messages.first_mut() {
                if first_msg.role == "user" {
                    first_msg.content = format!("{}\n\n{}", system_prompt, first_msg.content);
                }
            }
        }
        
        let anthropic_request = AnthropicRequest {
            model: request.model.clone(),
            messages: anthropic_messages,
            max_tokens: request.max_tokens.unwrap_or(4096),
            temperature: request.temperature,
            stream: Some(false),
        };
        
        let response = self.client
            .post(format!("{}/v1/messages", self.base_url))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_API_VERSION)
            .header("content-type", "application/json")
            .json(&anthropic_request)
            .send()
            .await
            .map_err(|e| LLMError::NetworkError(e.to_string()))?;
        
        match response.status() {
            StatusCode::OK => {
                let anthropic_response: AnthropicResponse = response.json().await
                    .map_err(|e| LLMError::ApiError(format!("Failed to parse response: {}", e)))?;
                
                let content = anthropic_response.content
                    .into_iter()
                    .map(|c| c.text)
                    .collect::<Vec<_>>()
                    .join("");
                
                Ok(ChatCompletionResponse {
                    id: anthropic_response.id,
                    model: anthropic_response.model,
                    choices: vec![ChatChoice {
                        index: 0,
                        message: ChatMessage {
                            role: "assistant".to_string(),
                            content,
                        },
                        finish_reason: Some("stop".to_string()),
                    }],
                    usage: TokenUsage {
                        prompt_tokens: anthropic_response.usage.input_tokens,
                        completion_tokens: anthropic_response.usage.output_tokens,
                        total_tokens: anthropic_response.usage.input_tokens + anthropic_response.usage.output_tokens,
                    },
                })
            }
            StatusCode::TOO_MANY_REQUESTS => Err(LLMError::RateLimitExceeded),
            StatusCode::UNAUTHORIZED => Err(LLMError::AuthenticationFailed),
            status => {
                let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
                Err(LLMError::ApiError(format!("API error ({}): {}", status, error_text)))
            }
        }
    }
    
    async fn stream_completion(
        &self,
        model: &str,
        prompt: &str,
        temperature: Option<f32>,
        max_tokens: Option<u32>,
    ) -> Result<ChatStream, LLMError> {
        let anthropic_request = AnthropicRequest {
            model: model.to_string(),
            messages: vec![AnthropicMessage {
                role: "user".to_string(),
                content: prompt.to_string(),
            }],
            max_tokens: max_tokens.unwrap_or(4096),
            temperature,
            stream: Some(true),
        };
        
        let response = self.client
            .post(format!("{}/v1/messages", self.base_url))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_API_VERSION)
            .header("content-type", "application/json")
            .header("accept", "text/event-stream")
            .json(&anthropic_request)
            .send()
            .await
            .map_err(|e| LLMError::NetworkError(e.to_string()))?;
        
        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            return match status {
                StatusCode::TOO_MANY_REQUESTS => Err(LLMError::RateLimitExceeded),
                StatusCode::UNAUTHORIZED => Err(LLMError::AuthenticationFailed),
                _ => Err(LLMError::ApiError(format!("API error ({}): {}", status, error_text))),
            };
        }
        
        let stream = response
            .bytes_stream()
            .eventsource()
            .map(|result| {
                match result {
                    Ok(event) => {
                        if event.event == "content_block_delta" {
                            if let Ok(data) = serde_json::from_str::<serde_json::Value>(&event.data) {
                                if let Some(text) = data["delta"]["text"].as_str() {
                                    return Ok(text.to_string());
                                }
                            }
                        }
                        Ok(String::new())
                    }
                    Err(e) => {
                        error!("Stream error: {}", e);
                        Err(LLMError::ApiError(e.to_string()))
                    }
                }
            });
        
        Ok(Box::pin(stream))
    }
    
    async fn list_models(&self) -> Result<Vec<Model>, LLMError> {
        // Anthropic doesn't have a models endpoint, so we return a static list
        Ok(vec![
            Model {
                id: "claude-3-opus-20240229".to_string(),
                name: "Claude 3 Opus".to_string(),
                provider: LLMProvider::Anthropic,
                context_window: 200000,
                max_tokens: 4096,
            },
            Model {
                id: "claude-3-sonnet-20240229".to_string(),
                name: "Claude 3 Sonnet".to_string(),
                provider: LLMProvider::Anthropic,
                context_window: 200000,
                max_tokens: 4096,
            },
            Model {
                id: "claude-3-haiku-20240307".to_string(),
                name: "Claude 3 Haiku".to_string(),
                provider: LLMProvider::Anthropic,
                context_window: 200000,
                max_tokens: 4096,
            },
        ])
    }
}