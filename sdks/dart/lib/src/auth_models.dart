/// Models for app-user authentication (doc 08 "Implemented today", doc 24 §2).

/// A signed-in app user (`AppUser` in the OpenAPI contract).
class AppUser {
  final String id;
  final String? email;
  final String name;
  final String? created;
  final bool disabled;
  final String? lastLogin;

  /// `password` and the ids of linked providers.
  final List<String> providers;

  const AppUser({
    required this.id,
    this.email,
    this.name = '',
    this.created,
    this.disabled = false,
    this.lastLogin,
    this.providers = const [],
  });

  factory AppUser.fromJson(Map<String, dynamic> json) => AppUser(
        id: json['id'] as String? ?? '',
        email: json['email'] as String?,
        name: json['name'] as String? ?? '',
        created: json['created'] as String?,
        disabled: json['disabled'] as bool? ?? false,
        lastLogin: json['lastLogin'] as String?,
        providers: (json['providers'] as List?)?.whereType<String>().toList() ??
            const [],
      );

  Map<String, dynamic> toJson() => {
        'id': id,
        'email': email,
        'name': name,
        if (created != null) 'created': created,
        'disabled': disabled,
        'lastLogin': lastLogin,
        'providers': providers,
      };

  /// Name, else email, else id: what a UI shows for this user.
  String get displayName =>
      name.isNotEmpty ? name : (email?.isNotEmpty == true ? email! : id);

  /// Upper-case first letter of [displayName] (avatar placeholder).
  String get initial {
    final n = displayName.trim();
    return n.isEmpty ? '?' : String.fromCharCode(n.runes.first).toUpperCase();
  }

  AppUser copyWith({String? name}) => AppUser(
        id: id,
        email: email,
        name: name ?? this.name,
        created: created,
        disabled: disabled,
        lastLogin: lastLogin,
        providers: providers,
      );

  @override
  bool operator ==(Object other) =>
      other is AppUser &&
      other.id == id &&
      other.email == email &&
      other.name == name &&
      other.disabled == disabled &&
      other.lastLogin == lastLogin &&
      other.providers.join(',') == providers.join(',');

  @override
  int get hashCode => Object.hash(id, email, name, disabled, lastLogin);

  @override
  String toString() => 'AppUser($id, $email)';
}

/// Tokens plus user. Stored by [SessionStorage] as JSON.
class UserSession {
  final String token;
  final String? refreshToken;
  final DateTime? expiresAt;
  final AppUser user;

  const UserSession({
    required this.token,
    required this.user,
    this.refreshToken,
    this.expiresAt,
  });

  /// Tolerant: `refreshToken`, `expiresAt` and `user` may be missing.
  factory UserSession.fromJson(Map<String, dynamic> json) {
    final u = json['user'];
    return UserSession(
      token: json['token'] as String? ?? '',
      refreshToken: json['refreshToken'] as String?,
      expiresAt: json['expiresAt'] is String
          ? DateTime.tryParse(json['expiresAt'] as String)
          : null,
      user: u is Map<String, dynamic>
          ? AppUser.fromJson(u)
          : const AppUser(id: ''),
    );
  }

  Map<String, dynamic> toJson() => {
        'token': token,
        if (refreshToken != null) 'refreshToken': refreshToken,
        if (expiresAt != null)
          'expiresAt': expiresAt!.toUtc().toIso8601String(),
        'user': user.toJson(),
      };

  UserSession copyWith({AppUser? user}) => UserSession(
        token: token,
        refreshToken: refreshToken,
        expiresAt: expiresAt,
        user: user ?? this.user,
      );
}

/// One sign-in provider from `GET auth/providers`.
class AuthProviderInfo {
  final String id;
  final String name;
  final String? startPath;

  const AuthProviderInfo(
      {required this.id, required this.name, this.startPath});

  factory AuthProviderInfo.fromJson(Map<String, dynamic> json) =>
      AuthProviderInfo(
        id: json['id'] as String? ?? '',
        name: json['name'] as String? ?? (json['id'] as String? ?? ''),
        startPath: json['startPath'] as String?,
      );
}

/// Email/password flags of the app.
class EmailPasswordInfo {
  final bool enabled;
  final bool allowSignup;
  final int minPasswordLength;

  const EmailPasswordInfo({
    this.enabled = true,
    this.allowSignup = true,
    this.minPasswordLength = 8,
  });

  factory EmailPasswordInfo.fromJson(Map<String, dynamic> json) =>
      EmailPasswordInfo(
        enabled: json['enabled'] as bool? ?? true,
        allowSignup: json['allowSignup'] as bool? ?? true,
        minPasswordLength: (json['minPasswordLength'] as num?)?.toInt() ?? 8,
      );
}

/// What a sign-in screen can offer (`PublicAuth`).
class PublicAuth {
  final String app;
  final EmailPasswordInfo emailPassword;
  final List<AuthProviderInfo> providers;

  const PublicAuth({
    this.app = '',
    this.emailPassword = const EmailPasswordInfo(),
    this.providers = const [],
  });

  factory PublicAuth.fromJson(Map<String, dynamic> json) => PublicAuth(
        app: json['app'] as String? ?? '',
        emailPassword: json['emailPassword'] is Map<String, dynamic>
            ? EmailPasswordInfo.fromJson(
                json['emailPassword'] as Map<String, dynamic>)
            : const EmailPasswordInfo(),
        providers: (json['providers'] as List? ?? const [])
            .whereType<Map<String, dynamic>>()
            .map(AuthProviderInfo.fromJson)
            .toList(),
      );
}

/// Auth state: [AuthUnknown] (restoring), [AuthSignedOut], [AuthSignedIn].
sealed class AuthState {
  const AuthState();

  AppUser? get user => null;
}

final class AuthUnknown extends AuthState {
  const AuthUnknown();
}

final class AuthSignedOut extends AuthState {
  const AuthSignedOut();
}

final class AuthSignedIn extends AuthState {
  @override
  final AppUser user;
  const AuthSignedIn(this.user);

  @override
  bool operator ==(Object other) => other is AuthSignedIn && other.user == user;

  @override
  int get hashCode => user.hashCode;
}
