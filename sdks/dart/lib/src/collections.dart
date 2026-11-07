import 'http.dart';
import 'realtime.dart';

/// Typed collection API providing CRUD, keyset listing, count, and realtime subscriptions.
class Collection<T> {
  final String name;
  final HttpClient _http;
  final RealtimeService? _realtime;
  final T Function(Map<String, dynamic>)? _decoder;

  Collection(
    this.name,
    this._http, [
    this._realtime,
    this._decoder,
  ]);

  String get basePath =>
      '/api/v1/apps/${Uri.encodeComponent(_http.app)}/collections/${Uri.encodeComponent(name)}/records';

  /// Lists collection records using keyset or page pagination.
  Future<ListResult<T>> list({
    ListOptions? options,
    int? page,
    int? perPage,
    String? cursor,
    int? limit,
    String? filter,
    String? sort,
    String? expand,
    String? fields,
    String? total,
  }) async {
    final query = <String, dynamic>{
      if (options != null) ...options.toQueryParameters(),
      if (page != null) 'page': page,
      if (perPage != null) 'perPage': perPage,
      if (cursor != null) 'cursor': cursor,
      if (limit != null) 'limit': limit,
      if (filter != null) 'filter': filter,
      if (sort != null) 'sort': sort,
      if (expand != null) 'expand': expand,
      if (fields != null) 'fields': fields,
      if (total != null) 'total': total,
    };

    final res = await _http.request<Map<String, dynamic>>(
      'GET',
      basePath,
      query: query.isNotEmpty ? query : null,
    );

    return ListResult<T>.fromJson(res, _decoder);
  }

  /// Retrieves a single record by its ID.
  Future<T> getOne(
    String id, {
    String? expand,
    String? fields,
  }) async {
    final query = <String, dynamic>{
      if (expand != null) 'expand': expand,
      if (fields != null) 'fields': fields,
    };

    final res = await _http.request<Map<String, dynamic>>(
      'GET',
      '$basePath/${Uri.encodeComponent(id)}',
      query: query.isNotEmpty ? query : null,
    );

    return _decode(res);
  }

  /// Creates a new record in this collection.
  Future<T> create(Map<String, dynamic> data) async {
    final res = await _http.request<Map<String, dynamic>>(
      'POST',
      basePath,
      body: data,
    );
    return _decode(res);
  }

  /// Partially updates an existing record.
  Future<T> update(String id, Map<String, dynamic> data) async {
    final res = await _http.request<Map<String, dynamic>>(
      'PATCH',
      '$basePath/${Uri.encodeComponent(id)}',
      body: data,
    );
    return _decode(res);
  }

  /// Completely replaces an existing record.
  Future<T> replace(String id, Map<String, dynamic> data) async {
    final res = await _http.request<Map<String, dynamic>>(
      'PUT',
      '$basePath/${Uri.encodeComponent(id)}',
      body: data,
    );
    return _decode(res);
  }

  /// Deletes a record by its ID.
  Future<bool> delete(String id) async {
    await _http.request<void>(
      'DELETE',
      '$basePath/${Uri.encodeComponent(id)}',
    );
    return true;
  }

  /// Counts matching records in this collection.
  Future<int> count({CountOptions? options, String? filter}) async {
    final query = <String, dynamic>{
      if (options != null) ...options.toQueryParameters(),
      if (filter != null) 'filter': filter,
    };

    final res = await _http.request<Map<String, dynamic>>(
      'GET',
      '$basePath:count',
      query: query.isNotEmpty ? query : null,
    );

    return (res['count'] as num?)?.toInt() ?? 0;
  }

  /// Subscribes to realtime events for this collection.
  void Function() subscribe(
    RealtimeListener<T> listener, [
    RealtimeSubscriptionOptions? options,
  ]) {
    final rt = _realtime;
    if (rt == null) {
      throw StateError(
        'Realtime service is not available on this client instance.',
      );
    }
    return rt.subscribe<T>(
      name,
      listener,
      options: options,
      decoder: _decoder,
    );
  }

  T _decode(Map<String, dynamic> json) {
    final decoder = _decoder;
    if (decoder != null) {
      return decoder(json);
    } else if (T == RecordModel) {
      return RecordModel.fromJson(json) as T;
    } else {
      return json as T;
    }
  }
}
