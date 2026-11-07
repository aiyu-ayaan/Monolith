import 'package:http/http.dart' as http_pkg;

import 'auth.dart';
import 'session_storage.dart';
import 'collections.dart';
import 'files.dart';
import 'http.dart';
import 'realtime.dart';

/// Main client interface for communicating with a Monolith backend.
class MonolithClient {
  final HttpClient http;
  final RealtimeService realtime;
  final FilesService files;

  final Map<String, AuthService> _authServices = {};
  final Map<String, Collection<dynamic>> _collections = {};

  MonolithClient(ClientOptions options)
      : this._shared(HttpClient(options), options);

  MonolithClient._shared(this.http, ClientOptions options)
      : realtime = RealtimeService(http),
        files = FilesService(http) {
    // The app-user service is created first so its token hooks are installed on http.
    _authServices['users'] = AuthService.configured(
      http,
      storage: options.storage,
      redirectUri: options.redirectUri,
      urlLauncher: options.urlLauncher,
    );
  }

  factory MonolithClient.fromOptions(ClientOptions options) =>
      MonolithClient(options);

  /// Returns the AuthService for the given collection (defaults to 'users').
  AuthService auth([String collection = 'users']) {
    return _authServices.putIfAbsent(
      collection,
      () => AuthService(http, collection),
    );
  }

  /// Returns a Collection handle for executing typed CRUD and queries.
  Collection<T> collection<T>([
    String name = 'default',
    T Function(Map<String, dynamic>)? decoder,
  ]) {
    final cacheKey = '$name:${T.toString()}';
    final existing = _collections[cacheKey];
    if (existing != null) {
      return existing as Collection<T>;
    }

    final col = Collection<T>(name, http, realtime, decoder);
    _collections[cacheKey] = col;
    return col;
  }

  /// Executes transactional batch operations against the app database.
  Future<List<BatchResponseItem>> batch(
    List<BatchOperation> operations,
  ) async {
    final path = '/api/v1/apps/${Uri.encodeComponent(http.app)}/batch';
    final res = await http.request<dynamic>(
      'POST',
      path,
      body: {'operations': operations.map((e) => e.toJson()).toList()},
    );

    if (res is List) {
      return res
          .whereType<Map<String, dynamic>>()
          .map(BatchResponseItem.fromJson)
          .toList();
    }
    return const [];
  }

  /// Closes all active connections and resources.
  void close() {
    for (final service in _authServices.values) {
      service.dispose();
    }
    realtime.dispose();
    http.close();
  }
}

/// Entry point with a ready client: `final mono = await Monolith.init(url: ..., app: ...)`.
class Monolith {
  Monolith._();

  static final Map<String, Future<MonolithClient>> _instances = {};

  /// Creates the client, restores the stored session and returns it. The same [url] and
  /// [app] twice give the same instance (later options are ignored).
  static Future<MonolithClient> init({
    required String url,
    String app = 'default',
    SessionStorage? storage,
    String? key,
    String? redirectUri,
    UrlLauncher? urlLauncher,
    http_pkg.Client? client,
    Map<String, String>? headers,
  }) {
    final id = '${url.replaceAll(RegExp(r'/+$'), '')}|$app';
    return _instances.putIfAbsent(id, () async {
      final c = MonolithClient(ClientOptions(
        url: url,
        app: app,
        key: key,
        client: client,
        headers: headers,
        storage: storage,
        redirectUri: redirectUri,
        urlLauncher: urlLauncher,
      ));
      await c.auth().restore();
      return c;
    });
  }

  /// Closes and forgets every instance made by [init] (tests, hot restart).
  static Future<void> reset() async {
    final all = _instances.values.toList();
    _instances.clear();
    for (final f in all) {
      try {
        (await f).close();
      } catch (_) {}
    }
  }
}

/// Convenience factory for creating a [MonolithClient].
MonolithClient createClient(ClientOptions options) {
  return MonolithClient(options);
}
