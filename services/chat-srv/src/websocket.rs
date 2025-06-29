use crate::state::AppState;
use axum::extract::ws::{Message, WebSocket};
use futures::{sink::SinkExt, stream::StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info, warn};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ClientMessage {
    #[serde(rename = "auth")]
    Auth { token: String },
    
    #[serde(rename = "chat")]
    Chat {
        message: String,
        model: Option<String>,
        conversation_id: Option<String>,
        temperature: Option<f32>,
        max_tokens: Option<u32>,
    },
    
    #[serde(rename = "stop")]
    Stop,
    
    #[serde(rename = "ping")]
    Ping,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ServerMessage {
    #[serde(rename = "connected")]
    Connected { session_id: String },
    
    #[serde(rename = "authenticated")]
    Authenticated { user_id: String },
    
    #[serde(rename = "chunk")]
    Chunk {
        content: String,
        model: String,
        finish_reason: Option<String>,
    },
    
    #[serde(rename = "error")]
    Error { message: String },
    
    #[serde(rename = "usage")]
    Usage {
        prompt_tokens: u32,
        completion_tokens: u32,
        total_tokens: u32,
        remaining_daily: u64,
        remaining_monthly: u64,
    },
    
    #[serde(rename = "pong")]
    Pong,
}

pub async fn handle_socket(socket: WebSocket, state: Arc<AppState>) {
    let session_id = Uuid::new_v4().to_string();
    let (mut sender, mut receiver) = socket.split();
    let (tx, mut rx) = mpsc::channel::<ServerMessage>(100);
    
    // Send connection confirmation
    let _ = tx.send(ServerMessage::Connected {
        session_id: session_id.clone(),
    }).await;
    
    // Task to send messages to the client
    let tx_clone = tx.clone();
    let send_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if let Ok(json) = serde_json::to_string(&msg) {
                if sender.send(Message::Text(json)).await.is_err() {
                    break;
                }
            }
        }
    });
    
    // Task to receive messages from the client
    let recv_task = tokio::spawn(async move {
        let mut authenticated = false;
        let mut user_id = None;
        
        while let Some(Ok(msg)) = receiver.next().await {
            match msg {
                Message::Text(text) => {
                    match serde_json::from_str::<ClientMessage>(&text) {
                        Ok(client_msg) => {
                            match client_msg {
                                ClientMessage::Auth { token } => {
                                    // Validate JWT token
                                    match validate_token(&state, &token).await {
                                        Ok(uid) => {
                                            authenticated = true;
                                            user_id = Some(uid.clone());
                                            let _ = tx_clone.send(ServerMessage::Authenticated {
                                                user_id: uid,
                                            }).await;
                                        }
                                        Err(e) => {
                                            let _ = tx_clone.send(ServerMessage::Error {
                                                message: format!("Authentication failed: {}", e),
                                            }).await;
                                        }
                                    }
                                }
                                
                                ClientMessage::Chat { message, model, conversation_id, temperature, max_tokens } => {
                                    if !authenticated {
                                        let _ = tx_clone.send(ServerMessage::Error {
                                            message: "Not authenticated".to_string(),
                                        }).await;
                                        continue;
                                    }
                                    
                                    // Handle chat message
                                    if let Some(uid) = &user_id {
                                        handle_chat_message(
                                            &state,
                                            &tx_clone,
                                            uid,
                                            message,
                                            model,
                                            conversation_id,
                                            temperature,
                                            max_tokens,
                                        ).await;
                                    }
                                }
                                
                                ClientMessage::Stop => {
                                    // TODO: Implement stream cancellation
                                    info!("Stop requested for session {}", session_id);
                                }
                                
                                ClientMessage::Ping => {
                                    let _ = tx_clone.send(ServerMessage::Pong).await;
                                }
                            }
                        }
                        Err(e) => {
                            warn!("Failed to parse client message: {}", e);
                            let _ = tx_clone.send(ServerMessage::Error {
                                message: "Invalid message format".to_string(),
                            }).await;
                        }
                    }
                }
                Message::Close(_) => {
                    info!("Client disconnected: {}", session_id);
                    break;
                }
                _ => {}
            }
        }
    });
    
    // Wait for tasks to complete
    tokio::select! {
        _ = send_task => {},
        _ = recv_task => {},
    }
    
    // Clean up session
    state.active_sessions.remove(&session_id);
    info!("WebSocket session {} closed", session_id);
}

async fn validate_token(state: &AppState, token: &str) -> Result<String, String> {
    // TODO: Implement proper JWT validation
    // For now, just return a dummy user ID
    Ok("user123".to_string())
}

async fn handle_chat_message(
    state: &AppState,
    tx: &mpsc::Sender<ServerMessage>,
    user_id: &str,
    message: String,
    model: Option<String>,
    conversation_id: Option<String>,
    temperature: Option<f32>,
    max_tokens: Option<u32>,
) {
    // Check token limits
    match state.token_meter_service.check_limits(user_id).await {
        Ok(false) => {
            let _ = tx.send(ServerMessage::Error {
                message: "Token limit exceeded".to_string(),
            }).await;
            return;
        }
        Err(e) => {
            error!("Failed to check token limits: {}", e);
            let _ = tx.send(ServerMessage::Error {
                message: "Internal error".to_string(),
            }).await;
            return;
        }
        _ => {}
    }
    
    // Determine model and provider
    let model = model.unwrap_or_else(|| state.config.default_openai_model.clone());
    let is_anthropic = model.starts_with("claude");
    
    // Create or get conversation
    let conv_id = match conversation_id {
        Some(id) => id,
        None => {
            match state.conversation_service.create_conversation(user_id, &model).await {
                Ok(conv) => conv.id,
                Err(e) => {
                    error!("Failed to create conversation: {}", e);
                    let _ = tx.send(ServerMessage::Error {
                        message: "Failed to create conversation".to_string(),
                    }).await;
                    return;
                }
            }
        }
    };
    
    // Add user message to conversation
    if let Err(e) = state.conversation_service.add_message(&conv_id, "user", &message).await {
        error!("Failed to add user message: {}", e);
    }
    
    // Stream response
    let tx_clone = tx.clone();
    tokio::spawn(async move {
        let mut total_tokens = 0u32;
        let mut completion_tokens = 0u32;
        let mut assistant_message = String::new();
        
        // Stream from appropriate provider
        if is_anthropic {
            // Stream from Anthropic
            match state.anthropic_client.stream_completion(
                &model,
                &message,
                temperature,
                max_tokens,
            ).await {
                Ok(mut stream) => {
                    while let Some(chunk) = stream.next().await {
                        match chunk {
                            Ok(text) => {
                                assistant_message.push_str(&text);
                                completion_tokens += estimate_tokens(&text);
                                
                                let _ = tx_clone.send(ServerMessage::Chunk {
                                    content: text,
                                    model: model.clone(),
                                    finish_reason: None,
                                }).await;
                            }
                            Err(e) => {
                                error!("Stream error: {}", e);
                                let _ = tx_clone.send(ServerMessage::Error {
                                    message: "Stream error".to_string(),
                                }).await;
                                break;
                            }
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to start stream: {}", e);
                    let _ = tx_clone.send(ServerMessage::Error {
                        message: "Failed to start stream".to_string(),
                    }).await;
                    return;
                }
            }
        } else {
            // Stream from OpenAI
            match state.openai_client.stream_completion(
                &model,
                &message,
                temperature,
                max_tokens,
            ).await {
                Ok(mut stream) => {
                    while let Some(chunk) = stream.next().await {
                        match chunk {
                            Ok(text) => {
                                assistant_message.push_str(&text);
                                completion_tokens += estimate_tokens(&text);
                                
                                let _ = tx_clone.send(ServerMessage::Chunk {
                                    content: text,
                                    model: model.clone(),
                                    finish_reason: None,
                                }).await;
                            }
                            Err(e) => {
                                error!("Stream error: {}", e);
                                let _ = tx_clone.send(ServerMessage::Error {
                                    message: "Stream error".to_string(),
                                }).await;
                                break;
                            }
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to start stream: {}", e);
                    let _ = tx_clone.send(ServerMessage::Error {
                        message: "Failed to start stream".to_string(),
                    }).await;
                    return;
                }
            }
        }
        
        // Save assistant message
        if !assistant_message.is_empty() {
            if let Err(e) = state.conversation_service.add_message(&conv_id, "assistant", &assistant_message).await {
                error!("Failed to save assistant message: {}", e);
            }
        }
        
        // Update token usage
        let prompt_tokens = estimate_tokens(&message);
        total_tokens = prompt_tokens + completion_tokens;
        
        if let Err(e) = state.token_meter_service.record_usage(
            user_id,
            &model,
            prompt_tokens,
            completion_tokens,
        ).await {
            error!("Failed to record token usage: {}", e);
        }
        
        // Get remaining limits
        match state.token_meter_service.get_remaining_tokens(user_id).await {
            Ok((daily, monthly)) => {
                let _ = tx_clone.send(ServerMessage::Usage {
                    prompt_tokens,
                    completion_tokens,
                    total_tokens,
                    remaining_daily: daily,
                    remaining_monthly: monthly,
                }).await;
            }
            Err(e) => {
                error!("Failed to get remaining tokens: {}", e);
            }
        }
        
        // Send completion signal
        let _ = tx_clone.send(ServerMessage::Chunk {
            content: String::new(),
            model,
            finish_reason: Some("stop".to_string()),
        }).await;
    });
}

fn estimate_tokens(text: &str) -> u32 {
    // Simple estimation: ~4 characters per token
    (text.len() as f32 / 4.0).ceil() as u32
}