import 'dart:async';
import 'dart:convert';
import 'dart:math';

import 'package:http/http.dart' as http;

import 'http.dart';

/// Realtime change event emitted by collection subscribers.
class RealtimeEvent<T> {
  final String action;
  final T record;
  final String collection;

  const RealtimeEvent({
    required this.action,
    required this.record,
    required this.collection,
  });

  @override
  String toString() => 'RealtimeEvent($action in $collection: $record)';
}

/// Listener callback for realtime events.
typedef RealtimeListener<T> = void Function(RealtimeEvent<T> event);

/// Realtime subscription filter options.
class RealtimeSubscriptionOptions {
  final String? filter;

  const RealtimeSubscriptionOptions({this.filter});
}

abstract class _SubscriptionEntry {
  String? get filter;
  void dispatch(RealtimeEvent<dynamic> event);
}

class _TypedSubscriptionEntry<T> implements _SubscriptionEntry {
  @override
  final String? filter;
  final RealtimeListener<T> listener;
  final T Function(Map<String, dynamic>)? decoder;

  _TypedSubscriptionEntry({
    required this.listener,
    this.filter,
    this.decoder,
  });

  @override
  void dispatch(RealtimeEvent<dynamic> event) {
    T record;
    if (event.record is T) {
      record = event.record as T;
    } else if (event.record is Map<String, dynamic>) {
      if (decoder != null) {
        record = decoder!(event.record as Map<String, dynamic>);
      } else if (T == RecordModel) {
        record =
            RecordModel.fromJson(event.record as Map<String, dynamic>) as T;
      } else {
        record = event.record as T;
      }
    } else {
      record = event.record as T;
    }

    final typedEvent = RealtimeEvent<T>(
      action: event.action,
      record: record,
      collection: event.collection,
    );

    try {
      listener(typedEvent);
    } catch (_) {
      // Ignore subscriber exceptions
    }
  }
}

/// Manages SSE multiplexed subscription connection to the Monolith realtime endpoint.
class RealtimeService {
  final HttpClient _http;
  final Map<String, Set<_SubscriptionEntry>> _subscriptions = {};
  final Random _random = Random();

  http.Client? _streamClient;
  bool _isConnecting = false;
  bool _isConnected = false;
  Timer? _reconnectTimer;
  int _reconnectAttempts = 0;
  bool _disposed = false;

  RealtimeService(this._http);

  bool get isConnected => _isConnected;

  /// Subscribes to events for a given collection and returns an unsubscribe callback.
  void Function() subscribe<T>(
    String collection,
    RealtimeListener<T> listener, {
    RealtimeSubscriptionOptions? options,
    T Function(Map<String, dynamic>)? decoder,
  }) {
    final entry = _TypedSubscriptionEntry<T>(
      listener: listener,
      filter: options?.filter,
      decoder: decoder,
    );

    final set =
        _subscriptions.putIfAbsent(collection, () => <_SubscriptionEntry>{});
    set.add(entry);

    _reconnect();

    return () {
      final currentSet = _subscriptions[collection];
      if (currentSet != null) {
        currentSet.remove(entry);
        if (currentSet.isEmpty) {
          _subscriptions.remove(collection);
        }
      }

      if (_subscriptions.isEmpty) {
        disconnect();
      } else {
        _reconnect();
      }
    };
  }

  /// Disconnects the active SSE stream.
  void disconnect() {
    _reconnectTimer?.cancel();
    _reconnectTimer = null;
    _streamClient?.close();
    _streamClient = null;
    _isConnected = false;
    _isConnecting = false;
    _reconnectAttempts = 0;
  }

  void _reconnect() {
    if (_disposed) return;
    if (_subscriptions.isEmpty) {
      disconnect();
      return;
    }

    _streamClient?.close();
    _streamClient = null;
    _connect();
  }

  Future<void> _connect() async {
    if (_isConnecting || _subscriptions.isEmpty || _disposed) return;
    _isConnecting = true;

    final collections = _subscriptions.keys.join(',');
    final path = '/api/v1/apps/${Uri.encodeComponent(_http.app)}/realtime';
    final uri = _http.buildUri(
      path,
      collections.isNotEmpty ? {'collections': collections} : null,
    );

    final headers = <String, String>{
      'accept': 'text/event-stream',
      'cache-control': 'no-cache',
    };

    final token = _http.getToken() ?? _http.getApiKey();
    if (token != null) {
      headers['authorization'] = 'Bearer $token';
    }

    final client = _http.createInnerClient();
    _streamClient = client;

    try {
      final request = http.Request('GET', uri);
      request.headers.addAll(headers);

      final streamedResponse = await client.send(request);
      if (streamedResponse.statusCode < 200 ||
          streamedResponse.statusCode >= 300) {
        throw StateError(
          'SSE connection failed: ${streamedResponse.statusCode} ${streamedResponse.reasonPhrase}',
        );
      }

      _isConnected = true;
      _isConnecting = false;
      _reconnectAttempts = 0;

      String buffer = '';
      await for (final chunk
          in streamedResponse.stream.transform(utf8.decoder)) {
        buffer += chunk;
        buffer = buffer.replaceAll('\r\n', '\n');
        final blocks = buffer.split('\n\n');
        buffer = blocks.removeLast();

        for (final block in blocks) {
          _parseEventBlock(block);
        }
      }
    } catch (_) {
      // Connection closed or interrupted
    } finally {
      _isConnected = false;
      _isConnecting = false;
      client.close();
      if (_streamClient == client) {
        _streamClient = null;
      }

      if (_subscriptions.isNotEmpty && !_disposed) {
        _scheduleReconnect();
      }
    }
  }

  void _scheduleReconnect() {
    if (_reconnectTimer != null || _disposed) return;
    final delayMs = min(
      500 * (1 << _reconnectAttempts) + _random.nextInt(200),
      15000,
    );
    _reconnectAttempts++;
    _reconnectTimer = Timer(Duration(milliseconds: delayMs), () {
      _reconnectTimer = null;
      _connect();
    });
  }

  void _parseEventBlock(String block) {
    String eventName = 'message';
    String data = '';

    final lines = block.split('\n');
    for (final rawLine in lines) {
      final line = rawLine.trim();
      if (line.isEmpty || line.startsWith(':')) continue; // ping/comment

      if (line.startsWith('event:')) {
        eventName = line.substring(6).trim();
      } else if (line.startsWith('data:')) {
        final val = line.substring(5).trim();
        data = data.isEmpty ? val : '$data\n$val';
      }
    }

    if (data.isEmpty) return;

    try {
      final parsed = jsonDecode(data);
      if (parsed is Map<String, dynamic>) {
        _dispatchEvent(eventName, parsed);
      }
    } catch (_) {
      // Ignore unparseable data
    }
  }

  void _dispatchEvent(String event, Map<String, dynamic> payload) {
    final collection = payload['collection'] as String? ?? event;
    final subscribers = _subscriptions[collection];
    if (subscribers == null || subscribers.isEmpty) return;

    final action = payload['action'] as String? ?? event;
    final recordData = payload['record'] ?? payload;

    final rtEvent = RealtimeEvent<dynamic>(
      action: action,
      record: recordData,
      collection: collection,
    );

    for (final entry in List.of(subscribers)) {
      entry.dispatch(rtEvent);
    }
  }

  /// Closes the realtime service and disconnects all listeners.
  void dispose() {
    _disposed = true;
    disconnect();
  }
}
