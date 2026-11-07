import 'auth.dart';
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

  MonolithClient(ClientOptions options) : this._shared(HttpClient(options));

  MonolithClient._shared(this.http)
      : realtime = RealtimeService(http),
        files = FilesService(http);

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

/// Convenience factory for creating a [MonolithClient].
MonolithClient createClient(ClientOptions options) {
  return MonolithClient(options);
}
