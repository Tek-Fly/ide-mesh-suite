use super::{ChatCompletionRequest, ChatCompletionResponse, ChatMessage, ChatChoice, TokenUsage, LLMClient, LLMError, LLMProvider, Model, ChatStream, StreamResult};
use async_openai::{
    Client,
    config::OpenAIConfig,
    types::{
        CreateChatCompletionRequest,
        CreateChatCompletionResponse,
        CreateChatCompletionStreamResponse,
        ChatCompletionRequestMessage,
        ChatCompletionRequestUserMessage,
        ChatCompletionRequestAssistantMessage,
        ChatCompletionRequestSystemMessage,
        Role,
    },
};
use async_trait::async_trait;
use futures::{Stream, StreamExt};
use std::pin::Pin;
use tracing::{error, debug};

pub struct OpenAIClient {
    client: Client<OpenAIConfig>,
    org_id: Option<String>,
}

impl OpenAIClient {
    pub fn new(api_key: String, org_id: Option<String>, base_url: Option<String>) -> Self {
        let mut config = OpenAIConfig::new().with_api_key(api_key);
        
        if let Some(org) = &org_id {
            config = config.with_org_id(org);
        }
        
        if let Some(url) = base_url {
            config = config.with_api_base(url);
        }
        
        Self {
            client: Client::with_config(config),
            org_id,
        }
    }
    
    fn convert_messages(&self, messages: Vec<ChatMessage>) -> Vec<ChatCompletionRequestMessage> {
        messages.into_iter().map(|msg| {
            match msg.role.as_str() {
                "system" => ChatCompletionRequestMessage::System(
                    ChatCompletionRequestSystemMessage {
                        content: msg.content,
                        name: None,
                    }
                ),
                "user" => ChatCompletionRequestMessage::User(
                    ChatCompletionRequestUserMessage {
                        content: async_openai::types::ChatCompletionRequestUserMessageContent::Text(msg.content),
                        name: None,
                    }
                ),
                "assistant" => ChatCompletionRequestMessage::Assistant(
                    ChatCompletionRequestAssistantMessage {
                        content: Some(msg.content),
                        name: None,
                        tool_calls: None,
                        function_call: None,
                    }
                ),
                _ => ChatCompletionRequestMessage::User(
                    ChatCompletionRequestUserMessage {
                        content: async_openai::types::ChatCompletionRequestUserMessageContent::Text(msg.content),
                        name: None,
                    }
                ),
            }
        }).collect()
    }
}

#[async_trait]
impl LLMClient for OpenAIClient {
    async fn chat_completion(&self, request: ChatCompletionRequest) -> Result<ChatCompletionResponse, LLMError> {
        let openai_request = CreateChatCompletionRequest {
            model: request.model.clone(),
            messages: self.convert_messages(request.messages),
            temperature: request.temperature,
            max_tokens: request.max_tokens,
            stream: Some(false),
            ..Default::default()
        };
        
        match self.client.chat().create(openai_request).await {
            Ok(response) => {
                Ok(ChatCompletionResponse {
                    id: response.id,
                    model: response.model,
                    choices: response.choices.into_iter().map(|choice| ChatChoice {
                        index: choice.index,
                        message: ChatMessage {
                            role: "assistant".to_string(),
                            content: choice.message.content.unwrap_or_default(),
                        },
                        finish_reason: choice.finish_reason.map(|r| format!("{:?}", r)),
                    }).collect(),
                    usage: TokenUsage {
                        prompt_tokens: response.usage.map(|u| u.prompt_tokens as u32).unwrap_or(0),
                        completion_tokens: response.usage.map(|u| u.completion_tokens as u32).unwrap_or(0),
                        total_tokens: response.usage.map(|u| u.total_tokens as u32).unwrap_or(0),
                    },
                })
            }
            Err(e) => {
                error!("OpenAI API error: {}", e);
                Err(LLMError::ApiError(e.to_string()))
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
        let request = CreateChatCompletionRequest {
            model: model.to_string(),
            messages: vec![
                ChatCompletionRequestMessage::User(ChatCompletionRequestUserMessage {
                    content: async_openai::types::ChatCompletionRequestUserMessageContent::Text(prompt.to_string()),
                    name: None,
                })
            ],
            temperature,
            max_tokens,
            stream: Some(true),
            ..Default::default()
        };
        
        let stream = self.client.chat().create_stream(request).await
            .map_err(|e| LLMError::ApiError(e.to_string()))?;
        
        let mapped_stream = stream.map(|result| match result {
            Ok(response) => {
                if let Some(choice) = response.choices.first() {
                    Ok(choice.delta.content.clone().unwrap_or_default())
                } else {
                    Ok(String::new())
                }
            }
            Err(e) => {
                error!("Stream error: {}", e);
                Err(LLMError::ApiError(e.to_string()))
            }
        });
        
        Ok(Box::pin(mapped_stream))
    }
    
    async fn list_models(&self) -> Result<Vec<Model>, LLMError> {
        match self.client.models().list().await {
            Ok(response) => {
                let models = response.data.into_iter()
                    .filter(|m| m.id.contains("gpt") || m.id == "o3")
                    .map(|m| {
                        let (context_window, max_tokens) = match m.id.as_str() {
                            "gpt-4-turbo-preview" | "gpt-4-0125-preview" => (128000, 4096),
                            "gpt-4" | "gpt-4-0613" => (8192, 4096),
                            "gpt-4-32k" | "gpt-4-32k-0613" => (32768, 4096),
                            "gpt-3.5-turbo" | "gpt-3.5-turbo-0125" => (16385, 4096),
                            "o3" => (200000, 8192), // Hypothetical O3 model
                            _ => (4096, 2048),
                        };
                        
                        Model {
                            id: m.id.clone(),
                            name: m.id, // OpenAI uses ID as name
                            provider: LLMProvider::OpenAI,
                            context_window,
                            max_tokens,
                        }
                    })
                    .collect();
                
                Ok(models)
            }
            Err(e) => {
                error!("Failed to list OpenAI models: {}", e);
                Err(LLMError::ApiError(e.to_string()))
            }
        }
    }
}