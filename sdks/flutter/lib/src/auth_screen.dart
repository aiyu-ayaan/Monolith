import 'dart:async';

import 'package:flutter/material.dart';
import 'package:monolith_client/monolith_client.dart';
import 'package:url_launcher/url_launcher.dart';

import 'auth_labels.dart';
import 'auth_theme.dart';
import 'auth_widgets.dart';
import 'provider_brand.dart';

/// How the sign-in screen is framed.
enum AuthLayout { card, fullscreen, plain }

/// Replaces the built-in provider button.
typedef ProviderButtonBuilder = Widget Function(
  BuildContext context,
  AuthProviderInfo provider,
  bool loading,
  VoidCallback? onPressed,
);

/// Replaces the error banner.
typedef AuthErrorBuilder = Widget Function(
  BuildContext context,
  AuthUiError error,
  String text,
  VoidCallback dismiss,
);

/// The drop-in sign-in screen: the providers the server enables, an email form with a
/// sign-in/sign-up switch, loading and error states. Follows the app theme and system dark mode.
///
/// Three levels: use it as is, replace parts through the `*Builder` slots, or skip it and use
/// [AuthBuilder] / [AuthController] to draw everything yourself.
class MonolithAuthScreen extends StatelessWidget {
  const MonolithAuthScreen({
    super.key,
    required this.client,
    this.options = const AuthUiOptions(),
    this.labels = const AuthLabels(),
    this.theme = const AuthTheme(),
    this.layout = AuthLayout.card,
    this.title,
    this.subtitle,
    this.logo,
    this.termsUrl,
    this.privacyUrl,
    this.headerBuilder,
    this.providerButtonBuilder,
    this.errorBuilder,
    this.footerBuilder,
    this.onSignedIn,
    this.onError,
  });

  final MonolithClient client;
  final AuthUiOptions options;
  final AuthLabels labels;
  final AuthTheme theme;
  final AuthLayout layout;
  final String? title;
  final String? subtitle;
  final Widget? logo;
  final String? termsUrl;
  final String? privacyUrl;
  final Widget Function(BuildContext context, String title)? headerBuilder;
  final ProviderButtonBuilder? providerButtonBuilder;
  final AuthErrorBuilder? errorBuilder;
  final Widget Function(
    BuildContext context,
    String? termsUrl,
    String? privacyUrl,
  )?
  footerBuilder;
  final void Function(AppUser user)? onSignedIn;
  final void Function(AuthUiError error)? onError;

  @override
  Widget build(BuildContext context) {
    return Theme(
      data: theme.apply(Theme.of(context)),
      child: AuthBuilder(
        client: client,
        options: options,
        builder: (context, c, s) => _Notifier(
          state: s,
          onSignedIn: onSignedIn,
          onError: onError,
          child: _frame(context, _Body(screen: this, controller: c, state: s)),
        ),
      ),
    );
  }

  Widget _frame(BuildContext context, Widget body) {
    final content = ConstrainedBox(
      constraints: BoxConstraints(maxWidth: theme.maxWidth),
      child: Padding(padding: theme.padding, child: body),
    );
    switch (layout) {
      case AuthLayout.card:
        return Center(
          child: SingleChildScrollView(
            child: Card(margin: const EdgeInsets.all(16), child: content),
          ),
        );
      case AuthLayout.fullscreen:
        return Scaffold(
          body: SafeArea(
            child: Center(child: SingleChildScrollView(child: content)),
          ),
        );
      case AuthLayout.plain:
        return content;
    }
  }
}

/// Calls the screen callbacks when the user appears or the error changes.
class _Notifier extends StatefulWidget {
  const _Notifier({
    required this.state,
    required this.onSignedIn,
    required this.onError,
    required this.child,
  });

  final AuthUiState state;
  final void Function(AppUser user)? onSignedIn;
  final void Function(AuthUiError error)? onError;
  final Widget child;

  @override
  State<_Notifier> createState() => _NotifierState();
}

class _NotifierState extends State<_Notifier> {
  String? _user;
  AuthUiError? _error;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    _check();
  }

  @override
  void didUpdateWidget(_Notifier old) {
    super.didUpdateWidget(old);
    _check();
  }

  // Callbacks run after the frame so they may navigate or call setState.
  void _check() {
    final u = widget.state.user;
    if (u != null && u.id != _user) {
      _user = u.id;
      final cb = widget.onSignedIn;
      if (cb != null)
        WidgetsBinding.instance.addPostFrameCallback((_) => cb(u));
    } else if (u == null) {
      _user = null;
    }
    final e = widget.state.error;
    final cb = widget.onError;
    if (e != null && !identical(e, _error) && cb != null) {
      WidgetsBinding.instance.addPostFrameCallback((_) => cb(e));
    }
    _error = e;
  }

  @override
  Widget build(BuildContext context) => widget.child;
}

class _Body extends StatefulWidget {
  const _Body({
    required this.screen,
    required this.controller,
    required this.state,
  });

  final MonolithAuthScreen screen;
  final AuthController controller;
  final AuthUiState state;

  @override
  State<_Body> createState() => _BodyState();
}

class _BodyState extends State<_Body> {
  final _email = TextEditingController();
  final _password = TextEditingController();
  final _name = TextEditingController();
  bool _show = false;

  @override
  void dispose() {
    _email.dispose();
    _password.dispose();
    _name.dispose();
    super.dispose();
  }

  AuthController get c => widget.controller;
  AuthUiState get s => widget.state;
  MonolithAuthScreen get w => widget.screen;
  AuthLabels get l => w.labels;

  void _submit() {
    if (s.busy) return;
    if (s.mode == AuthMode.signUp) {
      unawaited(c.signUpWithEmail(_email.text, _password.text, _name.text));
    } else {
      unawaited(c.signInWithEmail(_email.text, _password.text));
    }
  }

  String _providerLabel(AuthProviderInfo p) {
    final brand = ProviderBrand.of(p.id, Theme.of(context));
    final known = ProviderBrand.knownIds.contains(p.id) && p.id != 'oidc';
    return l.continueWithName(
      known ? brand.name : (p.name.isEmpty ? brand.name : p.name),
    );
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final signUp = s.mode == AuthMode.signUp;
    final title = w.title ?? (signUp ? l.signUpTitle : l.signInTitle);
    final providers = c.visibleProviders;
    final showEmail = c.showEmailForm;
    final err = s.error;

    final children = <Widget>[
      w.headerBuilder?.call(context, title) ??
          Column(
            children: [
              if (w.logo != null)
                Padding(
                  padding: const EdgeInsets.only(bottom: 12),
                  child: w.logo,
                ),
              Text(
                title,
                style: theme.textTheme.headlineSmall,
                textAlign: TextAlign.center,
              ),
              if (w.subtitle != null)
                Padding(
                  padding: const EdgeInsets.only(top: 4),
                  child: Text(
                    w.subtitle!,
                    style: theme.textTheme.bodyMedium,
                    textAlign: TextAlign.center,
                  ),
                ),
            ],
          ),
      const SizedBox(height: 16),
      if (err != null) ...[
        _errorWidget(context, err),
        const SizedBox(height: 12),
      ],
    ];

    if (s.loading && s.methods == null) {
      children.add(
        Center(
          child: Semantics(
            label: l.loading,
            child: const CircularProgressIndicator(),
          ),
        ),
      );
    } else {
      for (final p in providers) {
        final loading = s.submittingProvider == p.id;
        final onPressed = s.busy
            ? null
            : () => unawaited(c.signInWithProvider(p.id));
        children
          ..add(
            w.providerButtonBuilder?.call(context, p, loading, onPressed) ??
                ProviderButton(
                  provider: p,
                  label: _providerLabel(p),
                  onPressed: onPressed,
                  loading: loading,
                  radius: w.theme.radius,
                ),
          )
          ..add(const SizedBox(height: 8));
      }
      if (providers.isNotEmpty && showEmail) {
        children.add(
          Padding(
            padding: const EdgeInsets.symmetric(vertical: 8),
            child: Row(
              children: [
                const Expanded(child: Divider()),
                Padding(
                  padding: const EdgeInsets.symmetric(horizontal: 12),
                  child: Text(l.or),
                ),
                const Expanded(child: Divider()),
              ],
            ),
          ),
        );
      }
      if (showEmail) children.addAll(_emailForm(context, signUp));
      if (!showEmail && providers.isEmpty && s.methods != null) {
        children.add(Text(l.unknownError, textAlign: TextAlign.center));
      }
    }
    children.add(
      w.footerBuilder?.call(context, w.termsUrl, w.privacyUrl) ??
          _footer(context),
    );

    return Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: children,
    );
  }

  Widget _errorWidget(BuildContext context, AuthUiError err) {
    final text = l.errorText(err, c.minPasswordLength);
    void dismiss() => c.dismissError();
    final custom = w.errorBuilder;
    if (custom != null) return custom(context, err, text, dismiss);
    final cs = Theme.of(context).colorScheme;
    return Semantics(
      liveRegion: true,
      child: Container(
        padding: const EdgeInsets.fromLTRB(12, 4, 4, 4),
        decoration: BoxDecoration(
          color: cs.errorContainer,
          borderRadius: BorderRadius.circular(8),
        ),
        child: Row(
          children: [
            Expanded(
              child: Text(text, style: TextStyle(color: cs.onErrorContainer)),
            ),
            IconButton(
              tooltip: l.dismiss,
              icon: Icon(Icons.close, color: cs.onErrorContainer, size: 18),
              onPressed: dismiss,
            ),
          ],
        ),
      ),
    );
  }

  List<Widget> _emailForm(BuildContext context, bool signUp) {
    return [
      TextField(
        controller: _email,
        enabled: !s.busy,
        keyboardType: TextInputType.emailAddress,
        autofillHints: const [AutofillHints.email],
        textInputAction: TextInputAction.next,
        decoration: InputDecoration(labelText: l.email),
      ),
      const SizedBox(height: 12),
      if (signUp) ...[
        TextField(
          controller: _name,
          enabled: !s.busy,
          textInputAction: TextInputAction.next,
          autofillHints: const [AutofillHints.name],
          decoration: InputDecoration(labelText: l.name),
        ),
        const SizedBox(height: 12),
      ],
      TextField(
        controller: _password,
        enabled: !s.busy,
        obscureText: !_show,
        autofillHints: [
          signUp ? AutofillHints.newPassword : AutofillHints.password,
        ],
        textInputAction: TextInputAction.done,
        onSubmitted: (_) => _submit(),
        decoration: InputDecoration(
          labelText: l.password,
          suffixIcon: IconButton(
            tooltip: _show ? l.hidePassword : l.showPassword,
            icon: Icon(_show ? Icons.visibility_off : Icons.visibility),
            onPressed: () => setState(() => _show = !_show),
          ),
        ),
      ),
      const SizedBox(height: 16),
      FilledButton(
        onPressed: s.busy ? null : _submit,
        child: s.submitting
            ? const SizedBox(
                width: 20,
                height: 20,
                child: CircularProgressIndicator(strokeWidth: 2),
              )
            : Text(signUp ? l.signUpButton : l.signInButton),
      ),
      if (c.signUpAvailable)
        Row(
          mainAxisAlignment: MainAxisAlignment.center,
          children: [
            Text(signUp ? l.haveAccount : l.noAccount),
            TextButton(
              onPressed: s.busy
                  ? null
                  : () => c.setMode(signUp ? AuthMode.signIn : AuthMode.signUp),
              child: Text(signUp ? l.goToSignIn : l.goToSignUp),
            ),
          ],
        ),
    ];
  }

  Widget _footer(BuildContext context) {
    final terms = w.termsUrl;
    final privacy = w.privacyUrl;
    if (terms == null && privacy == null) return const SizedBox.shrink();
    Widget link(String text, String url) => TextButton(
      style: TextButton.styleFrom(
        minimumSize: Size.zero,
        padding: const EdgeInsets.symmetric(horizontal: 4, vertical: 2),
      ),
      onPressed: () => unawaited(launchUrl(Uri.parse(url))),
      child: Text(text, style: Theme.of(context).textTheme.labelSmall),
    );
    return Padding(
      padding: const EdgeInsets.only(top: 12),
      child: Wrap(
        alignment: WrapAlignment.center,
        crossAxisAlignment: WrapCrossAlignment.center,
        children: [
          Text(l.termsPrefix, style: Theme.of(context).textTheme.labelSmall),
          if (terms != null) link(l.terms, terms),
          if (terms != null && privacy != null)
            Text(l.and, style: Theme.of(context).textTheme.labelSmall),
          if (privacy != null) link(l.privacy, privacy),
        ],
      ),
    );
  }
}
