import 'dart:async';

import 'package:app_links/app_links.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import 'package:monolith_client/monolith_client.dart';
import 'package:url_launcher/url_launcher.dart';

/// Session storage in the platform keystore (Keychain / EncryptedSharedPreferences).
/// When the keystore fails (unsupported platform, locked device) it falls back to memory for
/// the rest of the run, so sign-in still works but is not remembered.
class SecureSessionStorage implements SessionStorage {
  SecureSessionStorage({FlutterSecureStorage? storage})
    : _secure =
          storage ??
          const FlutterSecureStorage(
            aOptions: AndroidOptions(encryptedSharedPreferences: true),
          );

  final FlutterSecureStorage _secure;
  final MemorySessionStorage _memory = MemorySessionStorage();
  bool _failed = false;

  /// True once the keystore failed and memory is used.
  bool get usingMemory => _failed;

  @override
  Future<String?> read(String key) async {
    if (!_failed) {
      try {
        return await _secure.read(key: key);
      } catch (_) {
        _failed = true;
      }
    }
    return _memory.read(key);
  }

  @override
  Future<void> write(String key, String value) async {
    if (!_failed) {
      try {
        await _secure.write(key: key, value: value);
        return;
      } catch (_) {
        _failed = true;
      }
    }
    await _memory.write(key, value);
  }

  @override
  Future<void> delete(String key) async {
    if (!_failed) {
      try {
        await _secure.delete(key: key);
      } catch (_) {
        _failed = true;
      }
    }
    await _memory.delete(key);
  }
}

/// Flutter entry point.
///
/// ```dart
/// final mono = await MonolithFlutter.init(url: 'https://api.example.com', app: 'shop');
/// mono.collection('tasks').onSnapshot().listen(...);
/// ```
class MonolithFlutter {
  MonolithFlutter._();

  static final Set<MonolithClient> _linked = {};
  static final List<StreamSubscription<Uri>> _linkSubs = [];

  /// Creates the client (once per [url] + [app]), restores the stored session and, when
  /// [listenToDeepLinks] is true, completes provider sign-in from app links.
  ///
  /// Provider sign-in opens the system browser through `url_launcher`; the server then
  /// redirects to [redirectUri] (default `monolith-<app>://auth`). Register that scheme in the
  /// app (Android intent filter, iOS `CFBundleURLTypes`) and add the URI to the app's
  /// *Allowed redirect URLs* in the Admin UI. `app_links` delivers the link; its launch link
  /// arrives on the same stream.
  static Future<MonolithClient> init({
    required String url,
    String app = 'default',
    SessionStorage? storage,
    String? redirectUri,
    LaunchMode launchMode = LaunchMode.externalApplication,
    bool listenToDeepLinks = true,
    String? key,
  }) async {
    WidgetsFlutterBinding.ensureInitialized();
    final client = await Monolith.init(
      url: url,
      app: app,
      key: key,
      storage: storage ?? SecureSessionStorage(),
      redirectUri: redirectUri,
      urlLauncher: (u) => launchUrl(u, mode: launchMode),
    );
    if (listenToDeepLinks && _linked.add(client)) {
      final auth = client.auth();
      final sub = AppLinks().uriLinkStream.listen((uri) {
        if (matchesRedirect(uri, auth.redirectUri)) {
          auth.handleRedirectQuietly(uri);
        }
      });
      _linkSubs.add(sub);
    }
    return client;
  }

  /// Whether [uri] is the sign-in redirect [redirect] (same scheme and host, and path when
  /// [redirect] has one).
  static bool matchesRedirect(Uri uri, String redirect) {
    final r = Uri.tryParse(redirect);
    if (r == null) return false;
    if (uri.scheme != r.scheme || uri.host != r.host) return false;
    return r.path.isEmpty || r.path == '/' || uri.path == r.path;
  }

  /// Stops deep-link listening and closes every client (tests, hot restart).
  static Future<void> reset() async {
    for (final s in _linkSubs) {
      await s.cancel();
    }
    _linkSubs.clear();
    _linked.clear();
    await Monolith.reset();
  }
}
