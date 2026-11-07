import 'dart:async';
import 'dart:convert';

import 'auth_models.dart';
import 'errors.dart';
import 'http.dart';
import 'session_storage.dart';

/// Authentication session information containing the bearer token and optional user data.
class AuthSession {
  final String token;
  final Map<String, dynamic>? user;

  const AuthSession({
    required this.token,
    this.user,
  });

  factory AuthSession.fromJson(Map<String, dynamic> json) {
    return AuthSession(
      token: json['token'] as String? ?? '',
      user: json['user'] as Map<String, dynamic>?,
    );
  }

  Map<String, dynamic> toJson() => {
        'token': token,
        if (user != null) 'user': user,
      };

  @override
  String toString() => 'AuthSession(token: $token, user: $user)';
}

/// Callback triggered on authentication state change.
typedef AuthChangeListener = void Function(String? token);

/// Opens a URL in a browser (injected: this package has no platform dependency).
/// Returns false when the URL could not be opened.
typedef UrlLauncher = Future<bool> Function(Uri url);

/// Authentication service.
///
/// For the system collection (`_superusers` / `system`) it is the console login. For app
/// users (the default `users` collection) it also manages a full session: sign-up/in,
/// provider sign-in, refresh before expiry and once after a 401 (single-flight), restore from
/// [SessionStorage], and a [changes] stream (doc 24 §2).
class AuthService {
  final HttpClient _http;
  final String collection;
  final Set<AuthChangeListener> _listeners = {};
  final StreamController<String?> _streamController =
      StreamController<String?>.broadcast();

  final SessionStorage _storage;
  final DateTime Function() _now;
  final Duration _refreshSkew;
  final Duration _providersTtl;

  /// Opens provider sign-in URLs; set by the app (Flutter: url_launcher).
  UrlLauncher? urlLauncher;

  /// Where the server sends the browser after provider sign-in.
  String redirectUri;

  final StreamController<AuthState> _changes =
      StreamController<AuthState>.broadcast();
  final StreamController<MonolithException> _redirectErrors =
      StreamController<MonolithException>.broadcast();
  AuthState _state = const AuthUnknown();
  UserSession? _session;
  Future<UserSession?>? _refreshing;
  Future<void>? _restoring;
  PublicAuth? _providers;
  DateTime? _providersAt;

  AuthService(HttpClient http, [String collection = 'users'])
      : this.configured(http, collection: collection);

  /// Full constructor: [storage] (default memory), [redirectUri], [urlLauncher], and for tests
  /// a [now] clock. The access token is refreshed when it expires within [refreshSkew].
  AuthService.configured(
    this._http, {
    this.collection = 'users',
    SessionStorage? storage,
    String? redirectUri,
    this.urlLauncher,
    DateTime Function()? now,
    Duration refreshSkew = const Duration(seconds: 30),
    Duration providersTtl = const Duration(seconds: 60),
  })  : _storage = storage ?? MemorySessionStorage(),
        _now = now ?? DateTime.now,
        _refreshSkew = refreshSkew,
        _providersTtl = providersTtl,
        redirectUri = redirectUri ?? 'monolith-${_http.app}://auth' {
    if (!_isSystem && _http.tokenResolver == null) {
      _http.tokenResolver = _resolveToken;
      _http.onUnauthorized = _onUnauthorized;
    }
  }

  bool get _isSystem => collection == '_superusers' || collection == 'system';

  String get _storageKey => 'monolith.session:${_http.baseUrl}:${_http.app}';

  String get _base => '/api/v1/apps/${Uri.encodeComponent(_http.app)}/auth';

  /// Current authentication token, if any.
  String? get token => _http.getToken();

  /// Sets or clears the active auth token and notifies listeners. Low level: a different
  /// token drops the app-user session (use [signIn] and [signOut] for sessions).
  void setToken(String? token) {
    if (_session != null && _session!.token != token) {
      _dropSession(persist: true);
    }
    _http.setToken(token);
    _notify(token);
  }

  /// Registers an auth change listener and returns an unsubscribe callback.
  void Function() onChange(AuthChangeListener listener) {
    _listeners.add(listener);
    return () {
      _listeners.remove(listener);
    };
  }

  /// Broadcast stream of authentication token changes.
  Stream<String?> get onAuthStateChange => _streamController.stream;

  void _notify(String? token) {
    for (final listener in List.of(_listeners)) {
      try {
        listener(token);
      } catch (_) {
        // Ignore listener exceptions
      }
    }
    if (!_streamController.isClosed) {
      _streamController.add(token);
    }
  }

  /// Signs in with email and password.
  Future<AuthSession> signInWithPassword(String email, String password) async {
    if (!_isSystem) {
      final s = await _login(email, password);
      return AuthSession(token: s.token, user: s.user.toJson());
    }
    const path = '/api/v1/system/auth/login';

    final res = await _http.request<Map<String, dynamic>>(
      'POST',
      path,
      body: {'email': email, 'password': password},
    );

    final session = AuthSession.fromJson(res);
    if (session.token.isNotEmpty) {
      setToken(session.token);
    }
    return session;
  }

  /// Completes system installation with the one-time token and initial superuser credentials.
  Future<AuthSession> install(
    String token,
    String email,
    String password,
  ) async {
    final res = await _http.request<Map<String, dynamic>>(
      'POST',
      '/api/v1/system/install',
      body: {
        'token': token,
        'email': email,
        'password': password,
      },
    );

    final session = AuthSession.fromJson(res);
    if (session.token.isNotEmpty) {
      setToken(session.token);
    }
    return session;
  }

  /// Clears the auth token and the session at once, then ends the server session with the
  /// refresh token (best effort; errors are ignored). Completes when the server answered.
  Future<void> signOut() async {
    final rt = _session?.refreshToken;
    _dropSession(persist: true);
    _http.setToken(null);
    _notify(null);
    if (rt != null) {
      try {
        await _http.request<void>('POST', '$_base/logout',
            body: {'refreshToken': rt}, refresh: false);
      } catch (_) {
        // the local session is gone either way
      }
    }
  }

  /// Closes the streams.
  void dispose() {
    _streamController.close();
    _changes.close();
    _redirectErrors.close();
  }

  // ---- app users (doc 24 §2) ----

  void _requireAppUsers() {
    if (_isSystem) {
      throw StateError(
          'app-user auth is not available on the system collection');
    }
  }

  /// Current state. [AuthUnknown] until [restore] finished or a sign-in happened.
  AuthState get state => _state;

  /// Changes of [state] (broadcast, no replay: read [state] first).
  Stream<AuthState> get changes => _changes.stream;

  /// The signed-in app user, if any.
  AppUser? get user => _session?.user;

  /// Errors of a provider redirect handled in the background (e.g. by a deep-link listener).
  Stream<MonolithException> get redirectErrors => _redirectErrors.stream;

  void _emit(AuthState s) {
    _state = s;
    if (!_changes.isClosed) _changes.add(s);
  }

  /// Loads the stored session. Idempotent. Refreshes in the background when it is (nearly)
  /// expired. Ends in [AuthSignedIn] or [AuthSignedOut].
  Future<void> restore() => _restoring ??= _restore();

  Future<void> _restore() async {
    _requireAppUsers();
    UserSession? s;
    try {
      final raw = await _storage.read(_storageKey);
      if (raw != null) {
        final json = jsonDecode(raw);
        if (json is Map<String, dynamic>) {
          final parsed = UserSession.fromJson(json);
          if (parsed.token.isNotEmpty && parsed.user.id.isNotEmpty) s = parsed;
        }
        if (s == null) await _storage.delete(_storageKey);
      }
    } catch (_) {
      s = null;
      try {
        await _storage.delete(_storageKey);
      } catch (_) {}
    }
    // A sign-in that raced with restore wins.
    if (_state is! AuthUnknown) return;
    if (s == null) {
      _emit(const AuthSignedOut());
      return;
    }
    _session = s;
    _http.setToken(s.token);
    _emit(AuthSignedIn(s.user));
    if (_needsRefresh(s) && s.refreshToken != null) {
      unawaited(_refresh().then<void>((_) {}, onError: (Object _) {}));
    }
  }

  /// Which methods the app offers. Cached for 60 s; [force] bypasses the cache.
  Future<PublicAuth> providers({bool force = false}) async {
    _requireAppUsers();
    final cached = _providers;
    if (!force &&
        cached != null &&
        _now().difference(_providersAt!) < _providersTtl) {
      return cached;
    }
    final res = await _http.request<Map<String, dynamic>>(
        'GET', '$_base/providers',
        refresh: false);
    _providers = PublicAuth.fromJson(res);
    _providersAt = _now();
    return _providers!;
  }

  /// Creates an email/password account and signs it in.
  Future<AppUser> signUp(String email, String password, {String? name}) async {
    _requireAppUsers();
    final res =
        await _http.request<Map<String, dynamic>>('POST', '$_base/register',
            body: {
              'email': email,
              'password': password,
              if (name != null && name.isNotEmpty) 'name': name,
            },
            refresh: false);
    return (await _adopt(res)).user;
  }

  /// Signs in with email and password.
  Future<AppUser> signIn(String email, String password) async =>
      (await _login(email, password)).user;

  Future<UserSession> _login(String email, String password) async {
    _requireAppUsers();
    final res = await _http.request<Map<String, dynamic>>(
        'POST', '$_base/login',
        body: {'email': email, 'password': password}, refresh: false);
    return _adopt(res);
  }

  /// Builds the provider start URL, opens it with [urlLauncher] when one is set, and returns
  /// it. The session arrives later through [handleRedirect].
  Future<Uri> signInWithProvider(String id, {String? redirectUri}) async {
    _requireAppUsers();
    final url = Uri.parse(_http.buildUrl(
        '$_base/oauth/${Uri.encodeComponent(id)}/start',
        {'redirectUri': redirectUri ?? this.redirectUri}));
    final launcher = urlLauncher;
    if (launcher != null) {
      final ok = await launcher(url);
      if (!ok) {
        throw const MonolithException(Problem(
            type: 'launch_failed',
            title: 'Could not open the browser',
            status: 0));
      }
    }
    return url;
  }

  /// Completes a provider sign-in from the redirect [uri]
  /// (`...#token=&refreshToken=&expiresAt=` or `...#error=&error_description=`).
  /// Returns null when [uri] is not an auth redirect; throws [MonolithException]
  /// (type `oauth_failed`) when the fragment carries an error.
  Future<AppUser?> handleRedirect(Uri uri) async {
    _requireAppUsers();
    if (!uri.hasFragment || uri.fragment.isEmpty) return null;
    Map<String, String> f;
    try {
      f = Uri.splitQueryString(uri.fragment);
    } catch (_) {
      return null;
    }
    final error = f['error'];
    if (error != null && error.isNotEmpty) {
      final problem = Problem(
        type: 'oauth_failed',
        title: error,
        status: 0,
        detail: f['error_description'],
      );
      throw MonolithException(problem);
    }
    final tk = f['token'];
    if (tk == null || tk.isEmpty) return null;
    _http.setToken(tk);
    try {
      final me = await _http.request<Map<String, dynamic>>('GET', '$_base/me',
          refresh: false);
      final s = UserSession(
        token: tk,
        refreshToken: f['refreshToken'],
        expiresAt:
            f['expiresAt'] != null ? DateTime.tryParse(f['expiresAt']!) : null,
        user: AppUser.fromJson(me),
      );
      await _apply(s);
      return s.user;
    } catch (_) {
      _http.setToken(null);
      rethrow;
    }
  }

  /// Like [handleRedirect] but never throws: errors go to [redirectErrors]. For deep-link
  /// listeners.
  Future<void> handleRedirectQuietly(Uri uri) async {
    try {
      await handleRedirect(uri);
    } on MonolithException catch (e) {
      if (!_redirectErrors.isClosed) _redirectErrors.add(e);
    } catch (e) {
      if (!_redirectErrors.isClosed) {
        _redirectErrors.add(MonolithException(Problem(
            type: 'oauth_failed',
            title: 'Sign-in failed',
            status: 0,
            detail: '$e')));
      }
    }
  }

  /// Changes the display name.
  Future<AppUser> updateProfile({required String name}) async {
    _requireAppUsers();
    final res = await _http.request<Map<String, dynamic>>('PATCH', '$_base/me',
        body: {'name': name});
    final u = AppUser.fromJson(res);
    final s = _session;
    if (s != null) {
      await _apply(s.copyWith(user: u));
    }
    return u;
  }

  /// Changes the password. The server ends every session, so when the email is known this
  /// signs in again with the new password (errors of that step are ignored).
  Future<void> changePassword(String oldPassword, String newPassword) async {
    _requireAppUsers();
    await _http.request<void>('POST', '$_base/password',
        body: {'oldPassword': oldPassword, 'newPassword': newPassword});
    final email = _session?.user.email;
    if (email != null) {
      try {
        await _login(email, newPassword);
      } catch (_) {
        // old tokens die by themselves; the next refresh signs out
      }
    }
  }

  /// A valid access token, refreshed first when it expires within 30 s. Null when signed out.
  Future<String?> accessToken() async {
    if (!_isSystem) await restore();
    final s = _session;
    if (s == null) return _http.getToken();
    if (_needsRefresh(s) && s.refreshToken != null) {
      try {
        await _refresh();
      } catch (_) {
        if (_expired(s)) rethrow;
      }
    }
    return _session?.token;
  }

  bool _needsRefresh(UserSession s) =>
      s.expiresAt != null &&
      !_now().isBefore(s.expiresAt!.subtract(_refreshSkew));

  bool _expired(UserSession s) =>
      s.expiresAt != null && !_now().isBefore(s.expiresAt!);

  Future<String?> _resolveToken() async {
    final s = _session;
    if (s == null) return null;
    if (_needsRefresh(s) && s.refreshToken != null) {
      try {
        await _refresh();
      } catch (_) {
        // use the old token; a 401 comes back through _onUnauthorized
      }
    }
    return _session?.token;
  }

  Future<bool> _onUnauthorized() async {
    final s = _session;
    if (s == null || s.refreshToken == null) return false;
    try {
      final n = await _refresh(failedToken: s.token);
      return n != null;
    } catch (_) {
      return false;
    }
  }

  /// Single-flight refresh: concurrent callers share one future. [failedToken] is the token
  /// that got a 401; when another caller already replaced it, no second refresh happens.
  Future<UserSession?> _refresh({String? failedToken}) {
    final running = _refreshing;
    if (running != null) return running;
    final s = _session;
    if (s == null || s.refreshToken == null) return Future.value(null);
    if (failedToken != null && s.token != failedToken) return Future.value(s);
    final f = _doRefresh(s);
    _refreshing = f;
    return f.whenComplete(() => _refreshing = null);
  }

  Future<UserSession?> _doRefresh(UserSession old) async {
    try {
      final res = await _http.request<Map<String, dynamic>>(
          'POST', '$_base/refresh',
          body: {'refreshToken': old.refreshToken}, refresh: false);
      final n = UserSession.fromJson(res);
      final merged = UserSession(
        token: n.token,
        refreshToken: n.refreshToken ?? old.refreshToken,
        expiresAt: n.expiresAt,
        user: n.user.id.isEmpty ? old.user : n.user,
      );
      await _apply(merged);
      return merged;
    } on MonolithException catch (e) {
      if (e.status == 401 && identical(_session, old)) {
        // refresh token rejected: the session is over
        _dropSession(persist: true);
        _http.setToken(null);
        _notify(null);
      }
      rethrow;
    }
  }

  Future<UserSession> _adopt(Map<String, dynamic> res) async {
    final s = UserSession.fromJson(res);
    await _apply(s);
    return s;
  }

  /// Makes [s] the current session: http token, listeners, state, storage.
  Future<void> _apply(UserSession s) async {
    final tokenChanged = _http.getToken() != s.token;
    _session = s;
    _http.setToken(s.token);
    if (tokenChanged) _notify(s.token);
    final next = AuthSignedIn(s.user);
    if (_state != next) _emit(next);
    try {
      await _storage.write(_storageKey, jsonEncode(s.toJson()));
    } catch (_) {
      // storage failure must not break sign-in
    }
  }

  void _dropSession({required bool persist}) {
    _session = null;
    if (_state is! AuthSignedOut) _emit(const AuthSignedOut());
    if (persist) {
      unawaited(_storage.delete(_storageKey).catchError((Object _) {}));
    }
  }
}
