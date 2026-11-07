import 'dart:async';
import 'dart:convert';
import 'dart:math';

import 'package:http/http.dart' as http;

import 'http.dart';

/// Realtime change event emitted by collection subscribers.
class RealtimeEvent<T> {
  /// `create`, `update`, `delete` or `leave` (the record stopped matching
  /// your filter or rule; it was not deleted). Mapped from the wire field `op`.
  final String action;
  final T record;
  final String collection;

  /// Change-log sequence number; the resume point after a reconnect.
  final int? seq;

  const RealtimeEvent({
    required this.action,
    required this.record,
    required this.collection,
    this.seq,
  });

  @override
  String toString() => 'RealtimeEvent($action in $collection: $record)';
}

/// Listener callback for realtime events.
typedef RealtimeListener<T> = void Function(RealtimeEvent<T> event);

/// Realtime subscription options. A [filter] or [ids] opens a dedicated connection.
class RealtimeSubscriptionOptions {
  /// Filter expression (same syntax as `list(filter:)`); only matching records arrive.
  final String? filter;

  /// Only these record ids.
  final List<String>? ids;

  const RealtimeSubscriptionOptions({this.filter, this.ids});
}

/// Options for `onSnapshot`.
class SnapshotOptions {
  final String? filter;

  /// Sort expression, e.g. `-created,title`. Also applied client-side to live changes.
  final String? sort;

  /// Records in the initial result (server cap 1000). Default 100.
  final int? limit;

  const SnapshotOptions({this.filter, this.sort, this.limit});
}

enum DocumentChangeType { added, modified, removed }

class DocumentChange<T> {
  final DocumentChangeType type;
  final T doc;

  const DocumentChange(this.type, this.doc);

  @override
  String toString() => 'DocumentChange($type, $doc)';
}

/// The current result set of a query plus what changed since the previous snapshot
/// (everything is `added` in the first one).
class QuerySnapshot<T> {
  final List<T> docs;
  final List<DocumentChange<T>> changes;

  const QuerySnapshot(this.docs, this.changes);
}

/// Error reported by the server on a snapshot stream (`event: error`).
class RealtimeException implements Exception {
  final String message;
  const RealtimeException(this.message);

  @override
  String toString() => 'RealtimeException: $message';
}

T _decodeRecord<T>(dynamic raw, T Function(Map<String, dynamic>)? decoder) {
  if (raw is T) return raw;
  if (raw is Map<String, dynamic>) {
    if (decoder != null) return decoder(raw);
    if (T == RecordModel) return RecordModel.fromJson(raw) as T;
  }
  return raw as T;
}

/// Client-side comparator for `-created,title` style sort strings.
int Function(Map<String, dynamic>, Map<String, dynamic>)? _comparator(
    String? sort) {
  if (sort == null) return null;
  final keys = <(String, int)>[
    for (final part in sort.split(',').map((e) => e.trim()))
      if (part.isNotEmpty)
        if (part.startsWith('-'))
          (part.substring(1), -1)
        else
          (part.startsWith('+') ? part.substring(1) : part, 1),
  ];
  if (keys.isEmpty) return null;
  return (a, b) {
    for (final (f, d) in keys) {
      final x = a[f];
      final y = b[f];
      if (x == y) continue;
      if (x == null) return -d;
      if (y == null) return d;
      final c = (x is num && y is num)
          ? x.compareTo(y)
          : x.toString().compareTo(y.toString());
      if (c != 0) return c * d;
    }
    return 0;
  };
}

typedef _Dispatch = void Function(String event, Map<String, dynamic> payload);

/// One SSE connection with reconnect, backoff and resume (`since`, or a fresh snapshot).
class _Stream {
  final HttpClient _http;
  final Map<String, String> Function() _params;
  final _Dispatch _dispatch;

  /// A snapshot stream re-snapshots on reconnect instead of replaying.
  final bool _snapshot;
  final Random _random = Random();

  http.Client? _client;
  StreamSubscription<String>? _sub;
  Completer<void>? _done;
  Timer? _timer;
  int _attempts = 0;
  int _lastSeq = 0;
  int _gen = 0;
  bool _stopped = true;
  bool connected = false;

  _Stream(this._http, this._params, this._dispatch, this._snapshot);

  void start() {
    _stopped = false;
    _connect();
  }

  /// Reopens now (the collection list changed). Keeps `_lastSeq` so nothing is missed.
  void restart() {
    _abort();
    if (!_stopped) _connect();
  }

  void stop() {
    _stopped = true;
    _timer?.cancel();
    _timer = null;
    _abort();
    _attempts = 0;
  }

  void _abort() {
    _gen++;
    connected = false;
    _sub?.cancel();
    _sub = null;
    _client?.close();
    _client = null;
    final done = _done;
    if (done != null && !done.isCompleted) done.complete();
  }

  Future<void> _connect() async {
    _timer?.cancel();
    _timer = null;
    final gen = ++_gen;
    final query = <String, dynamic>{..._params()};
    if (!_snapshot && _lastSeq > 0) query['since'] = _lastSeq;
    final uri = _http.buildUri(
      '/api/v1/apps/${Uri.encodeComponent(_http.app)}/realtime',
      query,
    );
    final headers = <String, String>{
      'accept': 'text/event-stream',
      'cache-control': 'no-cache',
    };
    final token = _http.getToken() ?? _http.getApiKey();
    if (token != null) headers['authorization'] = 'Bearer $token';

    final client = _http.createInnerClient();
    final done = Completer<void>();
    _client = client;
    _done = done;

    try {
      final request = http.Request('GET', uri)..headers.addAll(headers);
      final res = await client.send(request);
      if (gen != _gen) return;
      if (res.statusCode < 200 || res.statusCode >= 300) {
        throw StateError(
            'SSE connection failed: ${res.statusCode} ${res.reasonPhrase}');
      }
      connected = true;
      _attempts = 0;
      var buffer = '';
      _sub = res.stream.transform(utf8.decoder).listen(
        (chunk) {
          buffer = (buffer + chunk).replaceAll('\r\n', '\n');
          final blocks = buffer.split('\n\n');
          buffer = blocks.removeLast();
          for (final block in blocks) {
            if (gen != _gen) return;
            _parse(block);
          }
        },
        onError: (_) {
          if (!done.isCompleted) done.complete();
        },
        onDone: () {
          if (!done.isCompleted) done.complete();
        },
        cancelOnError: true,
      );
      await done.future;
    } catch (_) {
      // network error or abort
    }
    if (gen != _gen) return; // restarted or stopped meanwhile
    connected = false;
    client.close();
    if (_client == client) _client = null;
    if (!_stopped) _scheduleReconnect();
  }

  void _scheduleReconnect() {
    if (_timer != null) return;
    final delayMs =
        min(500 * (1 << min(_attempts, 5)) + _random.nextInt(200), 15000);
    _attempts++;
    _timer = Timer(Duration(milliseconds: delayMs), () {
      _timer = null;
      if (!_stopped) _connect();
    });
  }

  void _parse(String block) {
    var event = 'message';
    var data = '';
    for (final raw in block.split('\n')) {
      final line = raw.trim();
      if (line.isEmpty || line.startsWith(':')) continue;
      if (line.startsWith('event:')) {
        event = line.substring(6).trim();
      } else if (line.startsWith('data:')) {
        final v = line.substring(5).trim();
        data = data.isEmpty ? v : '$data\n$v';
      }
    }
    if (data.isEmpty) return;
    final Map<String, dynamic> payload;
    try {
      final parsed = jsonDecode(data);
      if (parsed is! Map<String, dynamic>) return;
      payload = parsed;
    } catch (_) {
      return;
    }
    final seq = payload['seq'];
    if (seq is num && seq > _lastSeq) _lastSeq = seq.toInt();
    if (event == 'resync') {
      // Missed more than the server keeps: reconnect from scratch.
      _lastSeq = 0;
      _dispatch(event, payload);
      restart();
      return;
    }
    _dispatch(event, payload);
  }
}

class _Entry {
  final void Function(RealtimeEvent<dynamic> event) dispatch;
  const _Entry(this.dispatch);
}

RealtimeEvent<dynamic> _toEvent(Map<String, dynamic> p) =>
    RealtimeEvent<dynamic>(
      action: p['op'] as String? ?? '',
      record: p['record'],
      collection: p['collection'] as String? ?? '',
      seq: (p['seq'] as num?)?.toInt(),
    );

void _safe(void Function() fn) {
  try {
    fn();
  } catch (_) {
    // a listener must not break the stream
  }
}

/// Manages SSE connections to the Monolith realtime endpoint. Plain subscriptions share
/// one connection; a `filter`/`ids` subscription or a snapshot query opens a dedicated one.
class RealtimeService {
  final HttpClient _http;
  final Map<String, Set<_Entry>> _listeners = {};
  final Set<_Stream> _dedicated = {};
  _Stream? _shared;
  bool _disposed = false;

  RealtimeService(this._http);

  bool get isConnected =>
      (_shared?.connected ?? false) || _dedicated.any((s) => s.connected);

  /// Live events for a collection. Without options, all subscriptions share one connection;
  /// a `filter` or `ids` opens a dedicated one. Returns the unsubscribe callback.
  /// After a reconnect the stream resumes from the last seen change (no gaps).
  void Function() subscribe<T>(
    String collection,
    RealtimeListener<T> listener, {
    RealtimeSubscriptionOptions? options,
    T Function(Map<String, dynamic>)? decoder,
  }) {
    if (_disposed) return () {};
    final entry = _Entry((ev) {
      final typed = RealtimeEvent<T>(
        action: ev.action,
        record: _decodeRecord<T>(ev.record, decoder),
        collection: ev.collection,
        seq: ev.seq,
      );
      _safe(() => listener(typed));
    });

    final filter = options?.filter;
    final ids = options?.ids;
    if ((filter != null && filter.isNotEmpty) ||
        (ids != null && ids.isNotEmpty)) {
      final stream = _Stream(
        _http,
        () => {
          'collections': collection,
          if (filter != null && filter.isNotEmpty) 'filter.$collection': filter,
          if (ids != null && ids.isNotEmpty) 'ids.$collection': ids.join(','),
        },
        (ev, p) {
          if (ev == 'record') entry.dispatch(_toEvent(p));
        },
        false,
      );
      _dedicated.add(stream);
      stream.start();
      return () {
        stream.stop();
        _dedicated.remove(stream);
      };
    }

    final set = _listeners[collection];
    final fresh = set == null;
    (set ?? (_listeners[collection] = <_Entry>{})).add(entry);
    final shared = _shared;
    if (shared == null) {
      (_shared = _Stream(
        _http,
        () => {'collections': _listeners.keys.join(',')},
        (ev, p) {
          if (ev != 'record') return;
          for (final e
              in List.of(_listeners[p['collection']] ?? const <_Entry>{})) {
            e.dispatch(_toEvent(p));
          }
        },
        false,
      ))
          .start();
    } else if (fresh) {
      shared.restart();
    }
    return () {
      final cur = _listeners[collection];
      cur?.remove(entry);
      final emptied = cur != null && cur.isEmpty;
      if (emptied) _listeners.remove(collection);
      if (_listeners.isEmpty) {
        _shared?.stop();
        _shared = null;
      } else if (emptied) {
        _shared?.restart();
      }
    };
  }

  /// Firestore-style query listener: emits the current result set, then again after every
  /// change that affects it. Reconnects re-snapshot, so the view always converges.
  /// The connection opens on listen and closes on cancel. Server errors arrive as stream errors.
  Stream<QuerySnapshot<T>> onSnapshot<T>(
    String collection, {
    SnapshotOptions? options,
    T Function(Map<String, dynamic>)? decoder,
  }) {
    final docs = <String, Map<String, dynamic>>{};
    final cmp = _comparator(options?.sort);
    _Stream? stream;
    late final StreamController<QuerySnapshot<T>> ctrl;

    List<T> ordered() {
      final all = docs.values.toList();
      if (cmp != null) all.sort(cmp);
      return [for (final d in all) _decodeRecord<T>(d, decoder)];
    }

    void onEvent(String ev, Map<String, dynamic> p) {
      final changes = <DocumentChange<T>>[];
      T dec(Map<String, dynamic> d) => _decodeRecord<T>(d, decoder);
      if (ev == 'snapshot') {
        final next = <String, Map<String, dynamic>>{};
        for (final it in (p['items'] as List? ?? const [])) {
          if (it is Map<String, dynamic>) next['${it['id']}'] = it;
        }
        next.forEach((id, d) => changes.add(DocumentChange(
            docs.containsKey(id)
                ? DocumentChangeType.modified
                : DocumentChangeType.added,
            dec(d))));
        docs.forEach((id, d) {
          if (!next.containsKey(id)) {
            changes.add(DocumentChange(DocumentChangeType.removed, dec(d)));
          }
        });
        docs
          ..clear()
          ..addAll(next);
      } else if (ev == 'record') {
        final rec = p['record'];
        if (rec is! Map<String, dynamic>) return;
        final id = '${rec['id']}';
        final op = p['op'];
        if (op == 'delete' || op == 'leave') {
          final old = docs.remove(id);
          if (old == null) return;
          changes.add(DocumentChange(DocumentChangeType.removed, dec(old)));
        } else {
          changes.add(DocumentChange(
              docs.containsKey(id)
                  ? DocumentChangeType.modified
                  : DocumentChangeType.added,
              dec(rec)));
          docs[id] = rec;
        }
      } else if (ev == 'error') {
        ctrl.addError(
            RealtimeException(p['message'] as String? ?? 'realtime error'));
        return;
      } else {
        return;
      }
      ctrl.add(QuerySnapshot<T>(ordered(), changes));
    }

    ctrl = StreamController<QuerySnapshot<T>>(
      onListen: () {
        if (_disposed) return;
        final s = _Stream(
          _http,
          () => {
            'collections': collection,
            'snapshot': '${options?.limit ?? 100}',
            if (options?.filter != null && options!.filter!.isNotEmpty)
              'filter.$collection': options.filter!,
            if (options?.sort != null && options!.sort!.isNotEmpty)
              'sort.$collection': options.sort!,
          },
          (ev, p) => _safe(() => onEvent(ev, p)),
          true,
        );
        stream = s;
        _dedicated.add(s);
        s.start();
      },
      onCancel: () {
        final s = stream;
        if (s != null) {
          s.stop();
          _dedicated.remove(s);
        }
      },
    );
    return ctrl.stream;
  }

  /// Stops every connection and drops all listeners.
  void disconnect() {
    _shared?.stop();
    _shared = null;
    _listeners.clear();
    for (final s in _dedicated.toList()) {
      s.stop();
    }
    _dedicated.clear();
  }

  /// Closes the realtime service and disconnects all listeners.
  void dispose() {
    _disposed = true;
    disconnect();
  }
}
