import 'dart:async';
import 'dart:convert';
import 'dart:math';

import 'package:http/http.dart' as http;

import 'errors.dart';

/// Configuration options for initializing a Monolith client.
class ClientOptions {
  final String url;
  final String app;
  final String? key;
  final String? token;
  final http.Client? client;
  final http.Client Function()? clientFactory;
  final Map<String, String>? headers;

  const ClientOptions({
    required this.url,
    this.app = 'default',
    this.key,
    this.token,
    this.client,
    this.clientFactory,
    this.headers,
  });
}

/// Query parameters for listing records.
class ListOptions {
  final int? page;
  final int? perPage;
  final String? cursor;
  final int? limit;
  final String? filter;
  final String? sort;
  final String? expand;
  final String? fields;
  final String? total;

  const ListOptions({
    this.page,
    this.perPage,
    this.cursor,
    this.limit,
    this.filter,
    this.sort,
    this.expand,
    this.fields,
    this.total,
  });

  Map<String, dynamic> toQueryParameters() {
    return {
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
  }
}

/// Paginated list result.
class ListResult<T> {
  final List<T> items;
  final int? total;
  final String? cursor;
  final String? nextCursor;
  final String? prevCursor;
  final Map<String, dynamic> raw;

  const ListResult({
    required this.items,
    this.total,
    this.cursor,
    this.nextCursor,
    this.prevCursor,
    this.raw = const {},
  });

  factory ListResult.fromJson(
    Map<String, dynamic> json, [
    T Function(Map<String, dynamic>)? itemDecoder,
  ]) {
    final rawItems = json['items'] as List? ?? const [];
    final items = rawItems.map<T>((item) {
      if (item is Map<String, dynamic>) {
        if (itemDecoder != null) {
          return itemDecoder(item);
        } else if (T == RecordModel) {
          return RecordModel.fromJson(item) as T;
        } else {
          return item as T;
        }
      }
      return item as T;
    }).toList();

    final total = (json['total'] as num?)?.toInt();
    final cursor = json['cursor'] as String?;
    final nextCursor =
        json['nextCursor'] as String? ?? json['next_cursor'] as String?;
    final prevCursor =
        json['prevCursor'] as String? ?? json['prev_cursor'] as String?;

    return ListResult<T>(
      items: items,
      total: total,
      cursor: cursor,
      nextCursor: nextCursor,
      prevCursor: prevCursor,
      raw: json,
    );
  }
}

/// Query options for record count.
class CountOptions {
  final String? filter;

  const CountOptions({this.filter});

  Map<String, dynamic> toQueryParameters() {
    return {
      if (filter != null) 'filter': filter,
    };
  }
}

/// Generic record model returned from collections.
class RecordModel {
  final String id;
  final String created;
  final String updated;
  final int v;
  final Map<String, dynamic> data;

  const RecordModel({
    required this.id,
    required this.created,
    required this.updated,
    required this.v,
    this.data = const {},
  });

  factory RecordModel.fromJson(Map<String, dynamic> json) {
    return RecordModel(
      id: json['id'] as String? ?? '',
      created: json['created'] as String? ?? '',
      updated: json['updated'] as String? ?? '',
      v: (json['_v'] as num?)?.toInt() ?? 1,
      data: json,
    );
  }

  dynamic operator [](String key) => data[key];
  dynamic get(String key) => data[key];

  Map<String, dynamic> toJson() => data;

  @override
  String toString() => 'RecordModel(id: $id, data: $data)';
}

/// Individual batch operation definition.
class BatchOperation {
  final String method;
  final String path;
  final dynamic body;

  const BatchOperation({
    required this.method,
    required this.path,
    this.body,
  });

  Map<String, dynamic> toJson() => {
        'method': method,
        'path': path,
        if (body != null) 'body': body,
      };
}

/// Result of an executed batch operation.
class BatchResponseItem {
  final int status;
  final dynamic body;

  const BatchResponseItem({
    required this.status,
    this.body,
  });

  factory BatchResponseItem.fromJson(Map<String, dynamic> json) {
    return BatchResponseItem(
      status: (json['status'] as num?)?.toInt() ?? 200,
      body: json['body'],
    );
  }

  Map<String, dynamic> toJson() => {
        'status': status,
        if (body != null) 'body': body,
      };
}

/// Internal HTTP client with retries, auth injection, and RFC 9457 error mapping.
class HttpClient {
  final String baseUrl;
  final String app;
  final http.Client _client;
  final http.Client Function()? _clientFactory;
  final bool _isCustomClient;
  final Map<String, String> defaultHeaders;
  final Random _random = Random();

  String? _token;
  String? _apiKey;

  HttpClient(ClientOptions options)
      : baseUrl = options.url.replaceAll(RegExp(r'/+$'), ''),
        app = options.app,
        _token = options.token,
        _apiKey = options.key,
        _client =
            options.client ?? options.clientFactory?.call() ?? http.Client(),
        _clientFactory = options.clientFactory ??
            (options.client != null ? () => options.client! : null),
        _isCustomClient =
            options.client != null || options.clientFactory != null,
        defaultHeaders = Map.unmodifiable(options.headers ?? const {});

  http.Client get rawClient => _client;

  http.Client createInnerClient() => (_clientFactory ?? () => http.Client())();

  String? getToken() => _token;
  void setToken(String? token) => _token = token;

  String? getApiKey() => _apiKey;
  void setApiKey(String? key) => _apiKey = key;

  Uri buildUri(String path, [Map<String, dynamic>? query]) {
    final cleanPath = path.startsWith('/') ? path : '/$path';
    final baseUri = Uri.parse('$baseUrl$cleanPath');

    if (query == null || query.isEmpty) {
      return baseUri;
    }

    final queryParams = <String, dynamic>{};
    for (final entry in baseUri.queryParametersAll.entries) {
      if (entry.value.length == 1) {
        queryParams[entry.key] = entry.value.first;
      } else {
        queryParams[entry.key] = entry.value;
      }
    }

    for (final entry in query.entries) {
      final v = entry.value;
      if (v == null) continue;
      if (v is Iterable) {
        queryParams[entry.key] = v.map((e) => e.toString()).toList();
      } else {
        queryParams[entry.key] = v.toString();
      }
    }

    return baseUri.replace(queryParameters: queryParams);
  }

  String buildUrl(String path, [Map<String, dynamic>? query]) {
    return buildUri(path, query).toString();
  }

  Future<T> request<T>(
    String method,
    String path, {
    Map<String, dynamic>? query,
    Object? body,
    Map<String, String>? headers,
    T Function(dynamic)? decoder,
  }) async {
    final uri = buildUri(path, query);
    final reqHeaders = <String, String>{
      ...defaultHeaders,
      if (headers != null) ...headers,
    };

    final authToken = _token ?? _apiKey;
    if (authToken != null &&
        !reqHeaders.containsKey('authorization') &&
        !reqHeaders.containsKey('Authorization')) {
      reqHeaders['authorization'] = 'Bearer $authToken';
    }

    final isGet = method.toUpperCase() == 'GET';
    final maxAttempts = isGet ? 4 : 1;

    for (var attempt = 0; attempt < maxAttempts; attempt++) {
      http.Response response;
      try {
        final request = http.Request(method, uri);
        request.headers.addAll(reqHeaders);

        if (body != null) {
          if (body is String) {
            request.body = body;
          } else if (body is List<int>) {
            request.bodyBytes = body;
          } else {
            if (!request.headers.containsKey('content-type') &&
                !request.headers.containsKey('Content-Type')) {
              request.headers['content-type'] = 'application/json';
            }
            request.body = jsonEncode(body);
          }
        }

        final streamed = await _client.send(request);
        response = await http.Response.fromStream(streamed);
      } catch (err) {
        if (attempt < maxAttempts - 1) {
          final backoffMs =
              min(100 * (1 << attempt) + _random.nextInt(50), 2000);
          await Future<void>.delayed(Duration(milliseconds: backoffMs));
          continue;
        }
        throw MonolithException(Problem(
          type: 'network_error',
          title: 'Network Error',
          status: 0,
          detail: err.toString(),
        ));
      }

      if (response.statusCode == 204) {
        return null as T;
      }

      final contentType = response.headers['content-type'] ?? '';
      final isJson = contentType.contains('application/json') ||
          contentType.contains('problem+json');
      final reqId = response.headers['x-request-id'];

      if (response.statusCode < 200 || response.statusCode >= 300) {
        if (isGet && response.statusCode == 503 && attempt < maxAttempts - 1) {
          final retryAfter = response.headers['retry-after'];
          final delayMs = retryAfter != null
              ? (int.tryParse(retryAfter) ?? 1) * 1000
              : 100 * (1 << attempt);
          await Future<void>.delayed(Duration(milliseconds: delayMs));
          continue;
        }

        Problem problem;
        if (isJson) {
          try {
            final decoded = jsonDecode(response.body);
            if (decoded is Map<String, dynamic>) {
              problem = Problem.fromJson(
                decoded,
                defaultStatus: response.statusCode,
                defaultRequestId: reqId,
              );
            } else {
              problem = Problem(
                type: 'http_error',
                title: response.reasonPhrase ?? 'HTTP ${response.statusCode}',
                status: response.statusCode,
                detail: response.body,
                requestId: reqId,
              );
            }
          } catch (_) {
            problem = Problem(
              type: 'http_error',
              title: response.reasonPhrase ?? 'HTTP ${response.statusCode}',
              status: response.statusCode,
              detail: response.body,
              requestId: reqId,
            );
          }
        } else {
          problem = Problem(
            type: 'http_error',
            title: response.reasonPhrase ?? 'HTTP ${response.statusCode}',
            status: response.statusCode,
            detail: response.body.isNotEmpty ? response.body : null,
            requestId: reqId,
          );
        }

        throw MonolithException(problem);
      }

      if (isJson) {
        final decoded = jsonDecode(response.body);
        if (decoder != null) {
          return decoder(decoded);
        }
        return decoded as T;
      }

      if (decoder != null) {
        return decoder(response.body);
      }
      return response.body as T;
    }

    throw const MonolithException(Problem(
      type: 'request_failed',
      title: 'Request failed after max attempts',
      status: 0,
    ));
  }

  void close() {
    if (!_isCustomClient) {
      _client.close();
    }
  }
}
