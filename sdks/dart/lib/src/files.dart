import 'http.dart';

/// Metadata for an object stored in a Monolith storage bucket.
class StorageObject {
  final String bucket;
  final String key;
  final int size;
  final String contentType;
  final String blake3;
  final String created;
  final String updated;

  const StorageObject({
    required this.bucket,
    required this.key,
    required this.size,
    required this.contentType,
    required this.blake3,
    required this.created,
    required this.updated,
  });

  factory StorageObject.fromJson(Map<String, dynamic> json) {
    return StorageObject(
      bucket: json['bucket'] as String? ?? '',
      key: json['key'] as String? ?? '',
      size: (json['size'] as num?)?.toInt() ?? 0,
      contentType: json['contentType'] as String? ??
          json['content_type'] as String? ??
          '',
      blake3: json['blake3'] as String? ?? '',
      created: json['created'] as String? ?? '',
      updated: json['updated'] as String? ?? '',
    );
  }

  Map<String, dynamic> toJson() => {
        'bucket': bucket,
        'key': key,
        'size': size,
        'contentType': contentType,
        'blake3': blake3,
        'created': created,
        'updated': updated,
      };

  @override
  String toString() =>
      'StorageObject($bucket/$key, size: $size, type: $contentType)';
}

/// Query parameters for listing storage bucket objects.
class StorageListOptions {
  final String? prefix;
  final String? cursor;
  final int? limit;

  const StorageListOptions({
    this.prefix,
    this.cursor,
    this.limit,
  });

  Map<String, dynamic> toQueryParameters() {
    return {
      if (prefix != null) 'prefix': prefix,
      if (cursor != null) 'cursor': cursor,
      if (limit != null) 'limit': limit,
    };
  }
}

/// Paginated result of objects in a storage bucket.
class StorageListResult {
  final List<StorageObject> items;
  final String? nextCursor;

  const StorageListResult({
    required this.items,
    this.nextCursor,
  });

  factory StorageListResult.fromJson(Map<String, dynamic> json) {
    final rawItems = json['items'] as List? ?? const [];
    final items = rawItems
        .whereType<Map<String, dynamic>>()
        .map(StorageObject.fromJson)
        .toList();

    final nextCursor =
        json['nextCursor'] as String? ?? json['next_cursor'] as String?;

    return StorageListResult(
      items: items,
      nextCursor: nextCursor,
    );
  }
}

/// Service for managing storage bucket files and retrieving download URLs.
class FilesService {
  final HttpClient _http;

  FilesService(this._http);

  String bucketPath(String bucket) =>
      '/api/v1/apps/${Uri.encodeComponent(_http.app)}/storage/buckets/${Uri.encodeComponent(bucket)}/objects';

  /// Generates the public/authenticated URL for a bucket object with optional image thumbnail transformation.
  String getURL(
    String bucket,
    String key, {
    String? thumb,
  }) {
    final path = '${bucketPath(bucket)}/${Uri.encodeComponent(key)}';
    final query = thumb != null ? {'thumb': thumb} : null;
    return _http.buildUrl(path, query);
  }

  /// Uploads binary data or text to a storage bucket object.
  Future<StorageObject> upload(
    String bucket,
    String key,
    dynamic data, {
    String? contentType,
  }) async {
    final path = '${bucketPath(bucket)}/${Uri.encodeComponent(key)}';
    final headers = <String, String>{
      if (contentType != null) 'content-type': contentType,
    };

    final res = await _http.request<Map<String, dynamic>>(
      'PUT',
      path,
      body: data,
      headers: headers.isNotEmpty ? headers : null,
    );

    return StorageObject.fromJson(res);
  }

  /// Deletes an object from a storage bucket.
  Future<bool> delete(String bucket, String key) async {
    final path = '${bucketPath(bucket)}/${Uri.encodeComponent(key)}';
    await _http.request<void>('DELETE', path);
    return true;
  }

  /// Lists objects in a storage bucket.
  Future<StorageListResult> list(
    String bucket, {
    StorageListOptions? options,
    String? prefix,
    String? cursor,
    int? limit,
  }) async {
    final query = <String, dynamic>{
      if (options != null) ...options.toQueryParameters(),
      if (prefix != null) 'prefix': prefix,
      if (cursor != null) 'cursor': cursor,
      if (limit != null) 'limit': limit,
    };

    final res = await _http.request<Map<String, dynamic>>(
      'GET',
      bucketPath(bucket),
      query: query.isNotEmpty ? query : null,
    );

    return StorageListResult.fromJson(res);
  }
}
