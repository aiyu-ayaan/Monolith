import 'dart:async';

import 'http.dart';

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

/// Authentication service for signing in, installing, and managing auth tokens.
class AuthService {
  final HttpClient _http;
  final String collection;
  final Set<AuthChangeListener> _listeners = {};
  final StreamController<String?> _streamController =
      StreamController<String?>.broadcast();

  AuthService(this._http, [this.collection = 'users']);

  /// Current authentication token, if any.
  String? get token => _http.getToken();

  /// Sets or clears the active auth token and notifies listeners.
  void setToken(String? token) {
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
    final isSystem = collection == '_superusers' || collection == 'system';
    final path = isSystem
        ? '/api/v1/system/auth/login'
        : '/api/v1/apps/${Uri.encodeComponent(_http.app)}/auth/login';

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

  /// Clears the auth token and notifies all listeners.
  void signOut() {
    setToken(null);
  }

  /// Closes the auth state change stream controller.
  void dispose() {
    _streamController.close();
  }
}
