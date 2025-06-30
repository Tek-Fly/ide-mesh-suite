// Flutter WebSocket Bridge Implementation
// Built with prayer and excellence

import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';
import 'package:web_socket_channel/web_socket_channel.dart';
import 'package:web_socket_channel/status.dart' as status;
import 'package:flutter/foundation.dart';
import 'package:rxdart/rxdart.dart';
import 'package:logging/logging.dart';

/// Message types for WebSocket communication
enum MessageType {
  connect,
  disconnect,
  authenticate,
  subscribe,
  unsubscribe,
  publish,
  request,
  response,
  error,
  ping,
  pong,
  streamStart,
  streamData,
  streamEnd,
}

/// WebSocket message structure
class WebSocketMessage {
  final String id;
  final MessageType type;
  final String? channel;
  final Map<String, dynamic>? data;
  final String? error;
  final DateTime timestamp;

  WebSocketMessage({
    required this.id,
    required this.type,
    this.channel,
    this.data,
    this.error,
    DateTime? timestamp,
  }) : timestamp = timestamp ?? DateTime.now();

  factory WebSocketMessage.fromJson(Map<String, dynamic> json) {
    return WebSocketMessage(
      id: json['id'] as String,
      type: MessageType.values.firstWhere(
        (e) => e.toString().split('.').last == json['type'],
      ),
      channel: json['channel'] as String?,
      data: json['data'] as Map<String, dynamic>?,
      error: json['error'] as String?,
      timestamp: DateTime.parse(json['timestamp'] as String),
    );
  }

  Map<String, dynamic> toJson() {
    return {
      'id': id,
      'type': type.toString().split('.').last,
      'channel': channel,
      'data': data,
      'error': error,
      'timestamp': timestamp.toIso8601String(),
    };
  }
}

/// Connection state
enum ConnectionState {
  disconnected,
  connecting,
  connected,
  reconnecting,
  error,
}

/// WebSocket Bridge for Flutter PWA
class WebSocketBridge {
  static final _logger = Logger('WebSocketBridge');
  
  final String url;
  final Map<String, String>? headers;
  final Duration pingInterval;
  final Duration reconnectDelay;
  final int maxReconnectAttempts;
  
  WebSocketChannel? _channel;
  StreamSubscription? _subscription;
  Timer? _pingTimer;
  Timer? _reconnectTimer;
  int _reconnectAttempts = 0;
  String? _authToken;
  
  // State management
  final _connectionStateController = BehaviorSubject<ConnectionState>.seeded(
    ConnectionState.disconnected,
  );
  final _messageController = StreamController<WebSocketMessage>.broadcast();
  final _errorController = StreamController<String>.broadcast();
  
  // Channel subscriptions
  final Map<String, StreamController<Map<String, dynamic>>> _channelControllers = {};
  final Map<String, Set<String>> _channelSubscriptions = {};
  
  // Request/Response handling
  final Map<String, Completer<Map<String, dynamic>>> _pendingRequests = {};
  
  WebSocketBridge({
    required this.url,
    this.headers,
    this.pingInterval = const Duration(seconds: 30),
    this.reconnectDelay = const Duration(seconds: 5),
    this.maxReconnectAttempts = 10,
  });
  
  // Getters
  Stream<ConnectionState> get connectionState => _connectionStateController.stream;
  Stream<WebSocketMessage> get messages => _messageController.stream;
  Stream<String> get errors => _errorController.stream;
  ConnectionState get currentState => _connectionStateController.value;
  bool get isConnected => currentState == ConnectionState.connected;
  
  /// Connect to WebSocket server
  Future<void> connect({String? authToken}) async {
    if (isConnected) {
      _logger.info('Already connected to WebSocket');
      return;
    }
    
    _authToken = authToken;
    _connectionStateController.add(ConnectionState.connecting);
    _reconnectAttempts = 0;
    
    try {
      final uri = Uri.parse(url);
      final wsUrl = uri.replace(
        scheme: uri.scheme == 'https' ? 'wss' : 'ws',
      );
      
      final connectHeaders = {
        ...?headers,
        if (_authToken != null) 'Authorization': 'Bearer $_authToken',
      };
      
      _channel = WebSocketChannel.connect(
        wsUrl,
        protocols: ['v1.taas.websocket'],
      );
      
      _subscription = _channel!.stream.listen(
        _handleMessage,
        onError: _handleError,
        onDone: _handleDone,
        cancelOnError: false,
      );
      
      // Send connect message
      _send(WebSocketMessage(
        id: _generateId(),
        type: MessageType.connect,
        data: {
          'version': '1.0.0',
          'client': 'flutter_pwa',
          'capabilities': ['streaming', 'binary', 'compression'],
        },
      ));
      
      _connectionStateController.add(ConnectionState.connected);
      _startPingTimer();
      
      _logger.info('WebSocket connected to $url');
      
      // Resubscribe to channels
      for (final subscriptions in _channelSubscriptions.entries) {
        for (final subId in subscriptions.value) {
          _send(WebSocketMessage(
            id: subId,
            type: MessageType.subscribe,
            channel: subscriptions.key,
          ));
        }
      }
      
    } catch (e) {
      _logger.severe('Failed to connect to WebSocket', e);
      _connectionStateController.add(ConnectionState.error);
      _errorController.add('Connection failed: $e');
      _scheduleReconnect();
    }
  }
  
  /// Disconnect from WebSocket server
  Future<void> disconnect() async {
    _logger.info('Disconnecting from WebSocket');
    
    _cancelTimers();
    
    if (_channel != null) {
      _send(WebSocketMessage(
        id: _generateId(),
        type: MessageType.disconnect,
      ));
      
      await _subscription?.cancel();
      await _channel?.sink.close(status.normalClosure);
    }
    
    _channel = null;
    _subscription = null;
    _connectionStateController.add(ConnectionState.disconnected);
  }
  
  /// Authenticate the connection
  Future<void> authenticate(String token) async {
    _authToken = token;
    
    if (!isConnected) {
      await connect(authToken: token);
      return;
    }
    
    _send(WebSocketMessage(
      id: _generateId(),
      type: MessageType.authenticate,
      data: {'token': token},
    ));
  }
  
  /// Subscribe to a channel
  Stream<Map<String, dynamic>> subscribe(String channel) {
    _logger.info('Subscribing to channel: $channel');
    
    // Create channel controller if it doesn't exist
    _channelControllers.putIfAbsent(
      channel,
      () => StreamController<Map<String, dynamic>>.broadcast(),
    );
    
    // Track subscription
    final subscriptionId = _generateId();
    _channelSubscriptions.putIfAbsent(channel, () => {}).add(subscriptionId);
    
    // Send subscribe message if connected
    if (isConnected) {
      _send(WebSocketMessage(
        id: subscriptionId,
        type: MessageType.subscribe,
        channel: channel,
      ));
    }
    
    return _channelControllers[channel]!.stream;
  }
  
  /// Unsubscribe from a channel
  void unsubscribe(String channel, {String? subscriptionId}) {
    _logger.info('Unsubscribing from channel: $channel');
    
    final subscriptions = _channelSubscriptions[channel];
    if (subscriptions == null || subscriptions.isEmpty) return;
    
    if (subscriptionId != null) {
      subscriptions.remove(subscriptionId);
    } else {
      subscriptions.clear();
    }
    
    // Remove channel if no more subscriptions
    if (subscriptions.isEmpty) {
      _channelSubscriptions.remove(channel);
      _channelControllers[channel]?.close();
      _channelControllers.remove(channel);
      
      // Send unsubscribe message if connected
      if (isConnected) {
        _send(WebSocketMessage(
          id: _generateId(),
          type: MessageType.unsubscribe,
          channel: channel,
        ));
      }
    }
  }
  
  /// Publish data to a channel
  void publish(String channel, Map<String, dynamic> data) {
    if (!isConnected) {
      _logger.warning('Cannot publish: not connected');
      return;
    }
    
    _send(WebSocketMessage(
      id: _generateId(),
      type: MessageType.publish,
      channel: channel,
      data: data,
    ));
  }
  
  /// Send a request and wait for response
  Future<Map<String, dynamic>> request(
    String method,
    Map<String, dynamic> params, {
    Duration timeout = const Duration(seconds: 30),
  }) async {
    if (!isConnected) {
      throw Exception('WebSocket not connected');
    }
    
    final requestId = _generateId();
    final completer = Completer<Map<String, dynamic>>();
    _pendingRequests[requestId] = completer;
    
    _send(WebSocketMessage(
      id: requestId,
      type: MessageType.request,
      data: {
        'method': method,
        'params': params,
      },
    ));
    
    try {
      return await completer.future.timeout(
        timeout,
        onTimeout: () {
          _pendingRequests.remove(requestId);
          throw TimeoutException('Request timeout: $method');
        },
      );
    } catch (e) {
      _pendingRequests.remove(requestId);
      rethrow;
    }
  }
  
  /// Send binary data
  void sendBinary(Uint8List data, {String? channel}) {
    if (!isConnected) {
      _logger.warning('Cannot send binary: not connected');
      return;
    }
    
    // Prepend channel info if provided
    if (channel != null) {
      final channelBytes = utf8.encode(channel);
      final channelLength = channelBytes.length;
      final message = Uint8List(4 + channelLength + data.length);
      
      // Write channel length (4 bytes)
      message.buffer.asByteData().setUint32(0, channelLength);
      
      // Write channel name
      message.setRange(4, 4 + channelLength, channelBytes);
      
      // Write data
      message.setRange(4 + channelLength, message.length, data);
      
      _channel!.sink.add(message);
    } else {
      _channel!.sink.add(data);
    }
  }
  
  // Private methods
  
  void _handleMessage(dynamic message) {
    try {
      if (message is String) {
        final json = jsonDecode(message) as Map<String, dynamic>;
        final wsMessage = WebSocketMessage.fromJson(json);
        
        _messageController.add(wsMessage);
        
        switch (wsMessage.type) {
          case MessageType.response:
            _handleResponse(wsMessage);
            break;
          case MessageType.publish:
            _handlePublish(wsMessage);
            break;
          case MessageType.error:
            _handleErrorMessage(wsMessage);
            break;
          case MessageType.pong:
            _logger.fine('Received pong');
            break;
          default:
            _logger.fine('Received message: ${wsMessage.type}');
        }
      } else if (message is Uint8List) {
        _handleBinaryMessage(message);
      }
    } catch (e) {
      _logger.severe('Error handling message', e);
      _errorController.add('Message handling error: $e');
    }
  }
  
  void _handleResponse(WebSocketMessage message) {
    final completer = _pendingRequests.remove(message.id);
    if (completer != null && !completer.isCompleted) {
      if (message.error != null) {
        completer.completeError(Exception(message.error));
      } else {
        completer.complete(message.data ?? {});
      }
    }
  }
  
  void _handlePublish(WebSocketMessage message) {
    if (message.channel == null) return;
    
    final controller = _channelControllers[message.channel!];
    if (controller != null && !controller.isClosed) {
      controller.add(message.data ?? {});
    }
  }
  
  void _handleErrorMessage(WebSocketMessage message) {
    _logger.severe('WebSocket error: ${message.error}');
    _errorController.add(message.error ?? 'Unknown error');
  }
  
  void _handleBinaryMessage(Uint8List data) {
    // Handle binary messages (e.g., for streaming)
    _logger.fine('Received binary message: ${data.length} bytes');
  }
  
  void _handleError(error) {
    _logger.severe('WebSocket error', error);
    _connectionStateController.add(ConnectionState.error);
    _errorController.add('WebSocket error: $error');
    _scheduleReconnect();
  }
  
  void _handleDone() {
    _logger.info('WebSocket connection closed');
    _connectionStateController.add(ConnectionState.disconnected);
    _scheduleReconnect();
  }
  
  void _send(WebSocketMessage message) {
    if (_channel?.sink == null) {
      _logger.warning('Cannot send message: channel not available');
      return;
    }
    
    try {
      final json = jsonEncode(message.toJson());
      _channel!.sink.add(json);
      _logger.fine('Sent message: ${message.type}');
    } catch (e) {
      _logger.severe('Error sending message', e);
      _errorController.add('Send error: $e');
    }
  }
  
  void _startPingTimer() {
    _pingTimer?.cancel();
    _pingTimer = Timer.periodic(pingInterval, (_) {
      if (isConnected) {
        _send(WebSocketMessage(
          id: _generateId(),
          type: MessageType.ping,
        ));
      }
    });
  }
  
  void _scheduleReconnect() {
    if (_reconnectAttempts >= maxReconnectAttempts) {
      _logger.severe('Max reconnection attempts reached');
      _connectionStateController.add(ConnectionState.error);
      return;
    }
    
    _reconnectAttempts++;
    _connectionStateController.add(ConnectionState.reconnecting);
    
    _reconnectTimer?.cancel();
    _reconnectTimer = Timer(reconnectDelay, () {
      _logger.info('Attempting to reconnect (attempt $_reconnectAttempts)');
      connect(authToken: _authToken);
    });
  }
  
  void _cancelTimers() {
    _pingTimer?.cancel();
    _reconnectTimer?.cancel();
  }
  
  String _generateId() {
    return '${DateTime.now().millisecondsSinceEpoch}_${_randomString(8)}';
  }
  
  String _randomString(int length) {
    const chars = 'abcdefghijklmnopqrstuvwxyz0123456789';
    final random = DateTime.now().millisecondsSinceEpoch;
    return List.generate(
      length,
      (i) => chars[(random + i) % chars.length],
    ).join();
  }
  
  /// Dispose of resources
  void dispose() {
    disconnect();
    _connectionStateController.close();
    _messageController.close();
    _errorController.close();
    
    for (final controller in _channelControllers.values) {
      controller.close();
    }
    _channelControllers.clear();
  }
}

/// WebSocket manager for managing multiple connections
class WebSocketManager {
  static final _logger = Logger('WebSocketManager');
  static final Map<String, WebSocketBridge> _bridges = {};
  
  /// Get or create a WebSocket bridge
  static WebSocketBridge getBridge(
    String name,
    String url, {
    Map<String, String>? headers,
    Duration? pingInterval,
    Duration? reconnectDelay,
    int? maxReconnectAttempts,
  }) {
    return _bridges.putIfAbsent(
      name,
      () => WebSocketBridge(
        url: url,
        headers: headers,
        pingInterval: pingInterval ?? const Duration(seconds: 30),
        reconnectDelay: reconnectDelay ?? const Duration(seconds: 5),
        maxReconnectAttempts: maxReconnectAttempts ?? 10,
      ),
    );
  }
  
  /// Remove a bridge
  static void removeBridge(String name) {
    final bridge = _bridges.remove(name);
    bridge?.dispose();
  }
  
  /// Dispose all bridges
  static void disposeAll() {
    for (final bridge in _bridges.values) {
      bridge.dispose();
    }
    _bridges.clear();
  }
}