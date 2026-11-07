import 'dart:async';

import 'package:flutter/material.dart';
import 'package:monolith_client/monolith_client.dart';

import 'auth_labels.dart';
import 'auth_screen.dart';

/// Headless sign-in: owns an [AuthController] (unless you pass one) and rebuilds on every
/// state change. Draw whatever you like in [builder].
class AuthBuilder extends StatefulWidget {
  const AuthBuilder({
    super.key,
    this.client,
    this.controller,
    this.options = const AuthUiOptions(),
    required this.builder,
  }) : assert(
         client != null || controller != null,
         'Pass a client or a controller',
       );

  final MonolithClient? client;

  /// Use an existing controller; the widget then does not dispose it.
  final AuthController? controller;
  final AuthUiOptions options;
  final Widget Function(
    BuildContext context,
    AuthController controller,
    AuthUiState state,
  )
  builder;

  @override
  State<AuthBuilder> createState() => _AuthBuilderState();
}

class _AuthBuilderState extends State<AuthBuilder> {
  late AuthController _controller;
  late bool _owns;

  @override
  void initState() {
    super.initState();
    _bind();
  }

  void _bind() {
    _owns = widget.controller == null;
    _controller =
        widget.controller ??
        AuthController(widget.client!, options: widget.options);
  }

  @override
  void didUpdateWidget(AuthBuilder old) {
    super.didUpdateWidget(old);
    if (old.controller != widget.controller || old.client != widget.client) {
      if (_owns) unawaited(_controller.dispose());
      _bind();
    }
  }

  @override
  void dispose() {
    if (_owns) unawaited(_controller.dispose());
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return StreamBuilder<AuthUiState>(
      stream: _controller.stream,
      initialData: _controller.state,
      builder: (context, snap) =>
          widget.builder(context, _controller, snap.data ?? _controller.state),
    );
  }
}

/// Shows [MonolithAuthScreen] until the user is signed in, then [child].
class AuthGate extends StatefulWidget {
  const AuthGate({
    super.key,
    required this.client,
    required this.child,
    this.options = const AuthUiOptions(),
    this.screen,
    this.loading,
  });

  final MonolithClient client;
  final Widget child;
  final AuthUiOptions options;

  /// Replaces the default [MonolithAuthScreen] (build your own, or configure the stock one).
  final WidgetBuilder? screen;

  /// Shown while the stored session is restored. Default: a centered progress indicator.
  final WidgetBuilder? loading;

  @override
  State<AuthGate> createState() => _AuthGateState();
}

class _AuthGateState extends State<AuthGate> {
  StreamSubscription<AuthState>? _sub;
  late AuthState _auth;

  @override
  void initState() {
    super.initState();
    final a = widget.client.auth();
    _auth = a.state;
    _sub = a.changes.listen((s) {
      if (mounted) setState(() => _auth = s);
    });
    unawaited(
      a.restore().then((_) {
        if (mounted) setState(() => _auth = a.state);
      }),
    );
  }

  @override
  void dispose() {
    unawaited(_sub?.cancel() ?? Future<void>.value());
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final s = _auth;
    if (s is AuthUnknown) {
      return widget.loading?.call(context) ??
          const Center(child: CircularProgressIndicator());
    }
    if (s is AuthSignedIn) return widget.child;
    return widget.screen?.call(context) ??
        MonolithAuthScreen(client: widget.client, options: widget.options);
  }
}

/// Avatar initial with the name (or email) next to it.
class UserChip extends StatelessWidget {
  const UserChip({super.key, required this.user});

  final AppUser user;

  /// First letter of the name, else of the email, else `?`.
  static String initialOf(AppUser user) {
    final s = user.name.trim().isNotEmpty
        ? user.name.trim()
        : (user.email ?? '').trim();
    return s.isEmpty ? '?' : s.substring(0, 1).toUpperCase();
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final title = user.name.trim().isNotEmpty
        ? user.name
        : (user.email ?? user.id);
    final showEmail =
        user.name.trim().isNotEmpty && (user.email ?? '').isNotEmpty;
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        CircleAvatar(
          radius: 16,
          backgroundColor: theme.colorScheme.primaryContainer,
          foregroundColor: theme.colorScheme.onPrimaryContainer,
          child: Text(initialOf(user)),
        ),
        const SizedBox(width: 8),
        Flexible(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(title, overflow: TextOverflow.ellipsis),
              if (showEmail)
                Text(
                  user.email!,
                  overflow: TextOverflow.ellipsis,
                  style: theme.textTheme.labelSmall,
                ),
            ],
          ),
        ),
      ],
    );
  }
}

/// Signs the current user out.
class SignOutButton extends StatelessWidget {
  const SignOutButton({
    super.key,
    required this.client,
    this.labels = const AuthLabels(),
  });

  final MonolithClient client;
  final AuthLabels labels;

  @override
  Widget build(BuildContext context) => OutlinedButton(
    onPressed: () => unawaited(client.auth().signOut()),
    child: Text(labels.signOut),
  );
}
