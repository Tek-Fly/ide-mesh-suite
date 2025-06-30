// Chat WebSocket Service for IDE-Mesh Flutter PWA
// Built with prayer and excellence

import 'dart:async';
import 'dart:convert';
import 'package:flutter/foundation.dart';
import 'package:logging/logging.dart';
import 'websocket_bridge.dart';

/// Chat message model
class ChatMessage {
  final String id;
  final String content;
  final String role; // 'user', 'assistant', 'system'
  final String? model;
  final DateTime timestamp;
  final Map<String, dynamic>? metadata;
  final bool isStreaming;

  ChatMessage({
    required this.id,
    required this.content,
    required this.role,
    this.model,
    DateTime? timestamp,
    this.metadata,
    this.isStreaming = false,
  }) : timestamp = timestamp ?? DateTime.now();

  factory ChatMessage.fromJson(Map<String, dynamic> json) {
    return ChatMessage(
      id: json['id'] as String,
      content: json['content'] as String,
      role: json['role'] as String,
      model: json['model'] as String?,
      timestamp: DateTime.parse(json['timestamp'] as String),
      metadata: json['metadata'] as Map<String, dynamic>?,
      isStreaming: json['isStreaming'] as bool? ?? false,
    );
  }

  Map<String, dynamic> toJson() {
    return {
      'id': id,
      'content': content,
      'role': role,
      'model': model,
      'timestamp': timestamp.toIso8601String(),
      'metadata': metadata,
      'isStreaming': isStreaming,
    };
  }

  ChatMessage copyWith({
    String? content,
    bool? isStreaming,
  }) {
    return ChatMessage(
      id: id,
      content: content ?? this.content,
      role: role,
      model: model,
      timestamp: timestamp,
      metadata: metadata,
      isStreaming: isStreaming ?? this.isStreaming,
    );
  }
}

/// Chat session model
class ChatSession {
  final String id;
  final String title;
  final List<ChatMessage> messages;
  final DateTime createdAt;
  final DateTime? updatedAt;
  final Map<String, dynamic>? settings;

  ChatSession({
    required this.id,
    required this.title,
    List<ChatMessage>? messages,
    DateTime? createdAt,
    this.updatedAt,
    this.settings,
  })  : messages = messages ?? [],
        createdAt = createdAt ?? DateTime.now();

  factory ChatSession.fromJson(Map<String, dynamic> json) {
    return ChatSession(
      id: json['id'] as String,
      title: json['title'] as String,
      messages: (json['messages'] as List<dynamic>?)
              ?.map((m) => ChatMessage.fromJson(m as Map<String, dynamic>))
              .toList() ??
          [],
      createdAt: DateTime.parse(json['createdAt'] as String),
      updatedAt: json['updatedAt'] != null
          ? DateTime.parse(json['updatedAt'] as String)
          : null,
      settings: json['settings'] as Map<String, dynamic>?,
    );
  }

  Map<String, dynamic> toJson() {
    return {
      'id': id,
      'title': title,
      'messages': messages.map((m) => m.toJson()).toList(),
      'createdAt': createdAt.toIso8601String(),
      'updatedAt': updatedAt?.toIso8601String(),
      'settings': settings,
    };
  }
}

/// Chat WebSocket Service
class ChatWebSocketService {
  static final _logger = Logger('ChatWebSocketService');
  
  final String baseUrl;
  final String? authToken;
  late final WebSocketBridge _bridge;
  
  // State management
  final Map<String, ChatSession> _sessions = {};
  final Map<String, StreamController<ChatMessage>> _messageStreams = {};
  final _sessionController = StreamController<List<ChatSession>>.broadcast();
  final _typingController = StreamController<Map<String, bool>>.broadcast();
  
  // Current session
  String? _currentSessionId;
  
  ChatWebSocketService({
    required this.baseUrl,
    this.authToken,
  }) {
    final wsUrl = baseUrl.replaceFirst(RegExp(r'^https?://'), 'wss://');
    _bridge = WebSocketManager.getBridge(
      'chat',
      '$wsUrl/ws/chat',
      headers: {
        if (authToken != null) 'Authorization': 'Bearer $authToken',
      },
    );
    
    _setupListeners();
  }
  
  // Getters
  Stream<ConnectionState> get connectionState => _bridge.connectionState;
  Stream<List<ChatSession>> get sessions => _sessionController.stream;
  Stream<Map<String, bool>> get typingIndicators => _typingController.stream;
  bool get isConnected => _bridge.isConnected;
  ChatSession? get currentSession => 
      _currentSessionId != null ? _sessions[_currentSessionId!] : null;
  
  /// Connect to chat service
  Future<void> connect() async {
    await _bridge.connect(authToken: authToken);
    
    // Load sessions after connection
    if (_bridge.isConnected) {
      await loadSessions();
    }
  }
  
  /// Disconnect from chat service
  Future<void> disconnect() async {
    await _bridge.disconnect();
  }
  
  /// Load chat sessions
  Future<void> loadSessions() async {
    try {
      final response = await _bridge.request('getSessions', {});
      final sessions = (response['sessions'] as List<dynamic>)
          .map((s) => ChatSession.fromJson(s as Map<String, dynamic>))
          .toList();
      
      _sessions.clear();
      for (final session in sessions) {
        _sessions[session.id] = session;
      }
      
      _sessionController.add(sessions);
    } catch (e) {
      _logger.severe('Failed to load sessions', e);
      rethrow;
    }
  }
  
  /// Create a new chat session
  Future<ChatSession> createSession({
    required String title,
    Map<String, dynamic>? settings,
  }) async {
    try {
      final response = await _bridge.request('createSession', {
        'title': title,
        'settings': settings,
      });
      
      final session = ChatSession.fromJson(response['session'] as Map<String, dynamic>);
      _sessions[session.id] = session;
      _sessionController.add(_sessions.values.toList());
      
      return session;
    } catch (e) {
      _logger.severe('Failed to create session', e);
      rethrow;
    }
  }
  
  /// Delete a chat session
  Future<void> deleteSession(String sessionId) async {
    try {
      await _bridge.request('deleteSession', {
        'sessionId': sessionId,
      });
      
      _sessions.remove(sessionId);
      _messageStreams[sessionId]?.close();
      _messageStreams.remove(sessionId);
      
      if (_currentSessionId == sessionId) {
        _currentSessionId = null;
      }
      
      _sessionController.add(_sessions.values.toList());
    } catch (e) {
      _logger.severe('Failed to delete session', e);
      rethrow;
    }
  }
  
  /// Select a chat session
  Future<void> selectSession(String sessionId) async {
    if (!_sessions.containsKey(sessionId)) {
      throw Exception('Session not found: $sessionId');
    }
    
    _currentSessionId = sessionId;
    
    // Subscribe to session updates
    _bridge.subscribe('session:$sessionId').listen((data) {
      _handleSessionUpdate(sessionId, data);
    });
  }
  
  /// Send a chat message
  Future<void> sendMessage(
    String content, {
    String? model,
    Map<String, dynamic>? metadata,
  }) async {
    if (_currentSessionId == null) {
      throw Exception('No session selected');
    }
    
    final message = ChatMessage(
      id: _generateId(),
      content: content,
      role: 'user',
      model: model,
      metadata: metadata,
    );
    
    // Add message to session immediately
    _sessions[_currentSessionId!]!.messages.add(message);
    _notifyMessageStream(_currentSessionId!, message);
    
    try {
      await _bridge.request('sendMessage', {
        'sessionId': _currentSessionId,
        'message': message.toJson(),
      });
    } catch (e) {
      _logger.severe('Failed to send message', e);
      // Remove message on failure
      _sessions[_currentSessionId!]!.messages.removeLast();
      rethrow;
    }
  }
  
  /// Stream a message response
  Stream<ChatMessage> streamMessage(String sessionId) {
    _messageStreams.putIfAbsent(
      sessionId,
      () => StreamController<ChatMessage>.broadcast(),
    );
    
    return _messageStreams[sessionId]!.stream;
  }
  
  /// Send typing indicator
  void sendTypingIndicator(bool isTyping) {
    if (_currentSessionId == null) return;
    
    _bridge.publish('typing', {
      'sessionId': _currentSessionId,
      'isTyping': isTyping,
    });
  }
  
  /// Stop message generation
  Future<void> stopGeneration() async {
    if (_currentSessionId == null) return;
    
    try {
      await _bridge.request('stopGeneration', {
        'sessionId': _currentSessionId,
      });
    } catch (e) {
      _logger.severe('Failed to stop generation', e);
      rethrow;
    }
  }
  
  /// Clear session messages
  Future<void> clearSession(String sessionId) async {
    try {
      await _bridge.request('clearSession', {
        'sessionId': sessionId,
      });
      
      _sessions[sessionId]?.messages.clear();
      _sessionController.add(_sessions.values.toList());
    } catch (e) {
      _logger.severe('Failed to clear session', e);
      rethrow;
    }
  }
  
  /// Export session as JSON
  Map<String, dynamic> exportSession(String sessionId) {
    final session = _sessions[sessionId];
    if (session == null) {
      throw Exception('Session not found: $sessionId');
    }
    
    return session.toJson();
  }
  
  /// Import session from JSON
  Future<ChatSession> importSession(Map<String, dynamic> json) async {
    try {
      final response = await _bridge.request('importSession', {
        'session': json,
      });
      
      final session = ChatSession.fromJson(response['session'] as Map<String, dynamic>);
      _sessions[session.id] = session;
      _sessionController.add(_sessions.values.toList());
      
      return session;
    } catch (e) {
      _logger.severe('Failed to import session', e);
      rethrow;
    }
  }
  
  // Private methods
  
  void _setupListeners() {
    // Listen for new messages
    _bridge.subscribe('messages').listen((data) {
      _handleNewMessage(data);
    });
    
    // Listen for typing indicators
    _bridge.subscribe('typing').listen((data) {
      _handleTypingIndicator(data);
    });
    
    // Listen for session updates
    _bridge.subscribe('sessions').listen((data) {
      _handleSessionsUpdate(data);
    });
  }
  
  void _handleNewMessage(Map<String, dynamic> data) {
    final sessionId = data['sessionId'] as String;
    final messageData = data['message'] as Map<String, dynamic>;
    final message = ChatMessage.fromJson(messageData);
    
    // Add to session
    final session = _sessions[sessionId];
    if (session != null) {
      session.messages.add(message);
      _notifyMessageStream(sessionId, message);
    }
  }
  
  void _handleSessionUpdate(String sessionId, Map<String, dynamic> data) {
    final type = data['type'] as String;
    
    switch (type) {
      case 'messageStart':
        final message = ChatMessage(
          id: data['messageId'] as String,
          content: '',
          role: 'assistant',
          model: data['model'] as String?,
          isStreaming: true,
        );
        _sessions[sessionId]?.messages.add(message);
        _notifyMessageStream(sessionId, message);
        break;
        
      case 'messageChunk':
        final messageId = data['messageId'] as String;
        final chunk = data['chunk'] as String;
        final session = _sessions[sessionId];
        if (session != null) {
          final messageIndex = session.messages.indexWhere((m) => m.id == messageId);
          if (messageIndex != -1) {
            final message = session.messages[messageIndex];
            session.messages[messageIndex] = message.copyWith(
              content: message.content + chunk,
            );
            _notifyMessageStream(sessionId, session.messages[messageIndex]);
          }
        }
        break;
        
      case 'messageEnd':
        final messageId = data['messageId'] as String;
        final session = _sessions[sessionId];
        if (session != null) {
          final messageIndex = session.messages.indexWhere((m) => m.id == messageId);
          if (messageIndex != -1) {
            final message = session.messages[messageIndex];
            session.messages[messageIndex] = message.copyWith(
              isStreaming: false,
            );
            _notifyMessageStream(sessionId, session.messages[messageIndex]);
          }
        }
        break;
    }
  }
  
  void _handleTypingIndicator(Map<String, dynamic> data) {
    final sessionId = data['sessionId'] as String;
    final isTyping = data['isTyping'] as bool;
    final userId = data['userId'] as String?;
    
    _typingController.add({
      '${sessionId}_${userId ?? 'assistant'}': isTyping,
    });
  }
  
  void _handleSessionsUpdate(Map<String, dynamic> data) {
    final type = data['type'] as String;
    
    switch (type) {
      case 'sessionCreated':
      case 'sessionUpdated':
        final sessionData = data['session'] as Map<String, dynamic>;
        final session = ChatSession.fromJson(sessionData);
        _sessions[session.id] = session;
        _sessionController.add(_sessions.values.toList());
        break;
        
      case 'sessionDeleted':
        final sessionId = data['sessionId'] as String;
        _sessions.remove(sessionId);
        _sessionController.add(_sessions.values.toList());
        break;
    }
  }
  
  void _notifyMessageStream(String sessionId, ChatMessage message) {
    final controller = _messageStreams[sessionId];
    if (controller != null && !controller.isClosed) {
      controller.add(message);
    }
  }
  
  String _generateId() {
    return '${DateTime.now().millisecondsSinceEpoch}_${UniqueKey().toString()}';
  }
  
  /// Dispose of resources
  void dispose() {
    _sessionController.close();
    _typingController.close();
    
    for (final controller in _messageStreams.values) {
      controller.close();
    }
    _messageStreams.clear();
    
    WebSocketManager.removeBridge('chat');
  }
}