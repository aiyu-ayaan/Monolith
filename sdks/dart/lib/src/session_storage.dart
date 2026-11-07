/// Where the app-user session (JSON string) is kept between runs.
///
/// The pure Dart package ships [MemorySessionStorage]. `monolith_flutter` ships a secure
/// implementation; any key/value store can implement this.
abstract class SessionStorage {
  Future<String?> read(String key);
  Future<void> write(String key, String value);
  Future<void> delete(String key);
}

/// Default: lives as long as the process.
class MemorySessionStorage implements SessionStorage {
  final Map<String, String> _data = {};

  @override
  Future<String?> read(String key) async => _data[key];

  @override
  Future<void> write(String key, String value) async => _data[key] = value;

  @override
  Future<void> delete(String key) async => _data.remove(key);
}
