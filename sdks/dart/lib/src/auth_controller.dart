import 'dart:async';

import 'auth.dart';
import 'auth_models.dart';
import 'client.dart';
import 'errors.dart';

/// Email form visibility: follow the server, always hide, always show.
enum EmailMode { auto, hide, force }

/// Sign-up: follow the server, never offer it, or start in sign-up.
enum SignUpMode { auto, hide, only }

/// Which form the email section shows.
enum AuthMode { signIn, signUp }

/// Why an auth action failed (doc 24 §3).
enum AuthErrorCode {
  invalidCredentials('invalid_credentials'),
  emailTaken('email_taken'),
  weakPassword('weak_password'),
  invalidEmail('invalid_email'),
  signupClosed('signup_closed'),
  network('network'),
  redirectNotAllowed('redirect_not_allowed'),
  providerFailed('provider_failed'),
  unknown('unknown');

  /// Stable string for logs and i18n keys.
  final String wire;
  const AuthErrorCode(this.wire);
}

/// Error shown by a sign-in UI: a [code] to translate, a default English [message], and a
/// developer [hint] for setup mistakes.
class AuthUiError {
  final AuthErrorCode code;
  final String message;
  final String? hint;
  final String? requestId;

  const AuthUiError(this.code, this.message, {this.hint, this.requestId});

  static const _messages = {
    AuthErrorCode.invalidCredentials: 'Wrong email or password.',
    AuthErrorCode.emailTaken: 'An account with this email already exists.',
    AuthErrorCode.weakPassword: 'The password is too short.',
    AuthErrorCode.invalidEmail: 'Enter a valid email address.',
    AuthErrorCode.signupClosed: 'Sign-up is not available.',
    AuthErrorCode.network: 'Cannot reach the server. Check your connection.',
    AuthErrorCode.redirectNotAllowed: 'This sign-in redirect is not allowed.',
    AuthErrorCode.providerFailed: 'Sign-in with this provider failed.',
    AuthErrorCode.unknown: 'Something went wrong.',
  };

  /// Default English text for [code].
  static String defaultMessage(AuthErrorCode code) => _messages[code]!;

  /// Maps a thrown error (usually [MonolithException]) to a code (doc 24 §3).
  factory AuthUiError.from(Object e, {String? redirectUri}) {
    if (e is! MonolithException) {
      return AuthUiError(
          AuthErrorCode.unknown, defaultMessage(AuthErrorCode.unknown));
    }
    final rid = e.requestId;
    AuthUiError mk(AuthErrorCode c, {String? message, String? hint}) =>
        AuthUiError(c, message ?? defaultMessage(c),
            hint: hint, requestId: rid);

    if (e.type == 'oauth_failed' || e.type == 'launch_failed') {
      final d = e.detail;
      return mk(AuthErrorCode.providerFailed,
          message: (d != null && d.isNotEmpty) ? d : null);
    }
    if (e.status == 0) return mk(AuthErrorCode.network);
    switch (e.status) {
      case 401:
        return mk(AuthErrorCode.invalidCredentials);
      case 409:
        return mk(AuthErrorCode.emailTaken);
      case 403:
        return mk(AuthErrorCode.signupClosed);
      case 400:
        final text = '${e.detail ?? ''} ${e.title}'.toLowerCase();
        if (e.getFieldError('redirectUri') != null ||
            text.contains('redirect')) {
          return mk(AuthErrorCode.redirectNotAllowed,
              hint: 'Add ${redirectUri ?? 'the redirect URI'} to the app\'s '
                  '"Allowed redirect URLs" (Admin UI > Authentication).');
        }
        if (e.getFieldError('password') != null) {
          return mk(AuthErrorCode.weakPassword,
              message: e.getFieldError('password'));
        }
        if (e.getFieldError('email') != null) {
          return mk(AuthErrorCode.invalidEmail,
              message: e.getFieldError('email'));
        }
        break;
    }
    return mk(AuthErrorCode.unknown,
        message: (e.detail != null && e.detail!.isNotEmpty) ? e.detail : null);
  }

  @override
  String toString() => 'AuthUiError(${code.wire}: $message)';
}

/// Options shared by every UI kit and the headless controller.
class AuthUiOptions {
  final EmailMode email;
  final SignUpMode signUp;

  /// null = all providers the server enabled, in server order. A list = allow list and order.
  final List<String>? providers;

  /// Provider ids never shown.
  final Set<String> hideProviders;

  const AuthUiOptions({
    this.email = EmailMode.auto,
    this.signUp = SignUpMode.auto,
    this.providers,
    this.hideProviders = const {},
  });
}

/// Everything a sign-in UI needs to draw itself.
class AuthUiState {
  /// Restoring the session or fetching the methods.
  final bool loading;

  /// Email flags and providers; null until fetched (or when the fetch failed).
  final PublicAuth? methods;
  final AuthMode mode;

  /// An email request is running.
  final bool submitting;

  /// A provider sign-in was started and not finished yet.
  final String? submittingProvider;
  final AuthUiError? error;

  /// Set when signed in.
  final AppUser? user;

  const AuthUiState({
    this.loading = true,
    this.methods,
    this.mode = AuthMode.signIn,
    this.submitting = false,
    this.submittingProvider,
    this.error,
    this.user,
  });

  /// Any request or provider sign-in in progress.
  bool get busy => submitting || submittingProvider != null;

  AuthUiState copyWith({
    bool? loading,
    PublicAuth? methods,
    AuthMode? mode,
    bool? submitting,
    String? submittingProvider,
    bool clearProvider = false,
    AuthUiError? error,
    bool clearError = false,
    AppUser? user,
    bool clearUser = false,
  }) =>
      AuthUiState(
        loading: loading ?? this.loading,
        methods: methods ?? this.methods,
        mode: mode ?? this.mode,
        submitting: submitting ?? this.submitting,
        submittingProvider: clearProvider
            ? null
            : (submittingProvider ?? this.submittingProvider),
        error: clearError ? null : (error ?? this.error),
        user: clearUser ? null : (user ?? this.user),
      );
}

final _emailRe = RegExp(r'^[^@\s]+@[^@\s]+\.[^@\s]+$');

/// Headless sign-in state machine. UI kits draw [state]; custom UIs can do the same.
///
/// Listen to [stream]; [state] is always the latest value (the stream does not replay).
class AuthController {
  final MonolithClient client;
  final AuthUiOptions options;
  final StreamController<AuthUiState> _out = StreamController.broadcast();
  final List<StreamSubscription<Object?>> _subs = [];
  AuthUiState _state;
  bool _disposed = false;

  /// Completes when the session is restored and the methods were fetched (or failed).
  late final Future<void> ready;

  AuthController(this.client, {this.options = const AuthUiOptions()})
      : _state = AuthUiState(
          mode: options.signUp == SignUpMode.only
              ? AuthMode.signUp
              : AuthMode.signIn,
          user: client.auth().user,
        ) {
    ready = _start();
  }

  AuthService get _auth => client.auth();

  AuthUiState get state => _state;
  Stream<AuthUiState> get stream => _out.stream;

  void _set(AuthUiState s) {
    if (_disposed) return;
    _state = s;
    _out.add(s);
  }

  Future<void> _start() async {
    _subs.add(_auth.changes.listen(_onAuth));
    _subs.add(_auth.redirectErrors.listen(_onRedirectError));
    await _auth.restore();
    _onAuth(_auth.state);
    await refreshMethods();
    _set(_state.copyWith(loading: false));
  }

  void _onAuth(AuthState s) {
    if (s is AuthSignedIn) {
      _set(_state.copyWith(
          user: s.user,
          submitting: false,
          clearProvider: true,
          clearError: true));
    } else if (s is AuthSignedOut) {
      _set(_state.copyWith(clearUser: true));
    }
  }

  void _onRedirectError(MonolithException e) {
    _set(_state.copyWith(
        submitting: false,
        clearProvider: true,
        error: AuthUiError.from(e, redirectUri: _auth.redirectUri)));
  }

  // ---- derived view of the options ----

  /// Whether to draw the email/password form.
  bool get showEmailForm {
    switch (options.email) {
      case EmailMode.hide:
        return false;
      case EmailMode.force:
        return true;
      case EmailMode.auto:
        return _state.methods?.emailPassword.enabled ?? true;
    }
  }

  /// Whether the sign-up form can be reached.
  bool get signUpAvailable {
    switch (options.signUp) {
      case SignUpMode.hide:
        return false;
      case SignUpMode.only:
        return true;
      case SignUpMode.auto:
        return _state.methods?.emailPassword.allowSignup ?? true;
    }
  }

  /// Providers to draw, in order.
  List<AuthProviderInfo> get visibleProviders {
    final all = _state.methods?.providers ?? const <AuthProviderInfo>[];
    final hidden = options.hideProviders;
    final wanted = options.providers;
    if (wanted == null) {
      return all.where((p) => !hidden.contains(p.id)).toList();
    }
    final byId = {for (final p in all) p.id: p};
    return [
      for (final id in wanted)
        if (!hidden.contains(id) && byId[id] != null) byId[id]!,
    ];
  }

  int get minPasswordLength =>
      _state.methods?.emailPassword.minPasswordLength ?? 8;

  // ---- actions ----

  /// Fetches the methods (cached 60 s unless [force]). A failure sets [AuthUiState.error]
  /// and keeps the previous methods.
  Future<void> refreshMethods({bool force = false}) async {
    try {
      final m = await _auth.providers(force: force);
      _set(_state.copyWith(methods: m));
    } catch (e) {
      _set(_state.copyWith(error: AuthUiError.from(e)));
    }
  }

  void setMode(AuthMode mode) {
    if (mode == AuthMode.signUp && !signUpAvailable) return;
    _set(_state.copyWith(mode: mode, clearError: true));
  }

  void dismissError() => _set(_state.copyWith(clearError: true));

  Future<void> signInWithEmail(String email, String password) async {
    final e = email.trim();
    if (!_emailRe.hasMatch(e)) {
      return _fail(const AuthUiError(
          AuthErrorCode.invalidEmail, 'Enter a valid email address.'));
    }
    if (password.isEmpty) {
      return _fail(const AuthUiError(
          AuthErrorCode.invalidCredentials, 'Wrong email or password.'));
    }
    await _email(() => _auth.signIn(e, password));
  }

  Future<void> signUpWithEmail(String email, String password,
      [String? name]) async {
    final e = email.trim();
    if (!_emailRe.hasMatch(e)) {
      return _fail(const AuthUiError(
          AuthErrorCode.invalidEmail, 'Enter a valid email address.'));
    }
    if (password.length < minPasswordLength) {
      return _fail(AuthUiError(AuthErrorCode.weakPassword,
          'The password must have at least $minPasswordLength characters.'));
    }
    final n = name?.trim();
    await _email(
        () => _auth.signUp(e, password, name: (n?.isEmpty ?? true) ? null : n));
  }

  void _fail(AuthUiError err) => _set(_state.copyWith(error: err));

  Future<void> _email(Future<AppUser> Function() run) async {
    _set(_state.copyWith(submitting: true, clearError: true));
    try {
      final u = await run();
      _set(_state.copyWith(submitting: false, user: u, clearError: true));
    } catch (e) {
      _set(_state.copyWith(submitting: false, error: AuthUiError.from(e)));
    }
  }

  /// Opens the provider page (through the client's `urlLauncher`). The state stays
  /// `submittingProvider == id` until the redirect finishes it or [cancelProvider] is called.
  Future<void> signInWithProvider(String id) async {
    _set(_state.copyWith(submittingProvider: id, clearError: true));
    try {
      await _auth.signInWithProvider(id);
    } catch (e) {
      _set(_state.copyWith(
          clearProvider: true,
          error: AuthUiError.from(e, redirectUri: _auth.redirectUri)));
    }
  }

  /// Forgets a pending provider sign-in (the user came back without finishing).
  void cancelProvider() => _set(_state.copyWith(clearProvider: true));

  /// Completes a provider sign-in from the redirect URI; errors land in [AuthUiState.error].
  Future<void> handleRedirect(Uri uri) async {
    try {
      await _auth.handleRedirect(uri);
    } catch (e) {
      _onRedirectError(e is MonolithException
          ? e
          : MonolithException(Problem(
              type: 'oauth_failed',
              title: 'Sign-in failed',
              status: 0,
              detail: '$e')));
    }
  }

  Future<void> signOut() async {
    await _auth.signOut();
    _set(_state.copyWith(clearUser: true, clearProvider: true));
    await refreshMethods();
  }

  Future<void> dispose() async {
    _disposed = true;
    for (final s in _subs) {
      await s.cancel();
    }
    await _out.close();
  }
}
