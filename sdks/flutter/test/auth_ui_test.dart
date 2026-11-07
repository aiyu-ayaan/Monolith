import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:monolith_flutter/monolith_flutter.dart';

void main() {
  test('matchesRedirect compares scheme, host and path', () {
    const r = 'monolith-shop://auth';
    expect(
      MonolithFlutter.matchesRedirect(
        Uri.parse('monolith-shop://auth#token=a'),
        r,
      ),
      isTrue,
    );
    expect(
      MonolithFlutter.matchesRedirect(Uri.parse('monolith-shop://other'), r),
      isFalse,
    );
    expect(
      MonolithFlutter.matchesRedirect(Uri.parse('https://auth'), r),
      isFalse,
    );
    expect(
      MonolithFlutter.matchesRedirect(
        Uri.parse('https://a.io/cb'),
        'https://a.io/cb',
      ),
      isTrue,
    );
    expect(
      MonolithFlutter.matchesRedirect(
        Uri.parse('https://a.io/x'),
        'https://a.io/cb',
      ),
      isFalse,
    );
  });

  test('UserChip.initialOf prefers name, then email', () {
    expect(
      UserChip.initialOf(const AppUser(id: '1', email: 'z@x.io', name: 'ann')),
      'A',
    );
    expect(
      UserChip.initialOf(const AppUser(id: '1', email: 'z@x.io', name: ' ')),
      'Z',
    );
  });

  test('AuthLabels translates error codes and fills placeholders', () {
    const l = AuthLabels(
      weakPassword: 'Mind. {min} Zeichen',
      continueWith: 'Weiter mit {name}',
    );
    expect(l.continueWithName('Google'), 'Weiter mit Google');
    final e = AuthUiError(
      AuthErrorCode.weakPassword,
      AuthUiError.defaultMessage(AuthErrorCode.weakPassword),
    );
    expect(l.errorText(e, 10), 'Mind. 10 Zeichen');
  });

  test('ProviderBrand falls back to a neutral button for unknown ids', () {
    final b = ProviderBrand.of('zed', ThemeData.light());
    expect(b.outlined, isTrue);
    expect(b.name, 'zed');
  });
}
