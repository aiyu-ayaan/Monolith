import 'dart:async';
import 'dart:convert';

import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:monolith_client/monolith_client.dart';
import 'package:test/test.dart';

Map<String, dynamic> userJson([String name = 'Ann']) => {
      'id': 'u1',
      'email': 'ann@example.com',
      'name': name,
      'created': '2026-01-01T00:00:00Z',
      'disabled': false,
      'providers': ['password'],
    };

http.Response jsonRes(Object body, [int status = 200]) =>
    http.Response(jsonEncode(body), status,
        headers: {'content-type': 'application/json'});

http.Response problem(int status, String title,
        {String? detail, List<Map<String, String>>? errors}) =>
    http.Response(
        jsonEncode({
          'type': 'about:blank',
          'title': title,
          'status': status,
          if (detail != null) 'detail': detail,
          if (errors != null) 'errors': errors,
        }),
        status,
        headers: {'content-type': 'application/problem+json'});

Map<String, dynamic> sessionJson(String token,
        {String refresh = 'r1', required DateTime exp, String name = 'Ann'}) =>
    {
      'token': token,
      'refreshToken': refresh,
      'expiresAt': exp.toUtc().toIso8601String(),
      'user': userJson(name),
    };

const publicAuth = {
  'app': 'shop',
  'emailPassword': {
    'enabled': true,
    'allowSignup': true,
    'minPasswordLength': 10
  },
  'providers': [
    {'id': 'google', 'name': 'Google', 'startPath': '/x'},
    {'id': 'github', 'name': 'GitHub', 'startPath': '/y'},
    {'id': 'acme', 'name': 'Acme SSO', 'startPath': '/z'},
  ],
};

MonolithClient mk(MockClient mc,
        {SessionStorage? storage, UrlLauncher? launcher}) =>
    MonolithClient(ClientOptions(
      url: 'https://api.example.com',
      app: 'shop',
      client: mc,
      storage: storage,
      urlLauncher: launcher,
    ));

void main() {
  group('app-user auth', () {
    test('signIn / signUp / signOut drive state, token and storage', () async {
      final calls = <String>[];
      final storage = MemorySessionStorage();
      final c = mk(MockClient((r) async {
        calls.add('${r.method} ${r.url.path}');
        final p = r.url.path;
        if (p.endsWith('/auth/login')) {
          return jsonRes(sessionJson('t1',
              exp: DateTime.now().add(const Duration(minutes: 15))));
        }
        if (p.endsWith('/auth/register')) {
          expect(jsonDecode(r.body)['name'], 'Ann');
          return jsonRes(
              sessionJson('t2',
                  exp: DateTime.now().add(const Duration(minutes: 15))),
              201);
        }
        if (p.endsWith('/auth/logout')) {
          expect(jsonDecode(r.body)['refreshToken'], 'r1');
          return http.Response('', 204);
        }
        return http.Response('', 404);
      }), storage: storage);
      final auth = c.auth();
      expect(auth.state, isA<AuthUnknown>());
      final states = <AuthState>[];
      auth.changes.listen(states.add);

      final u = await auth.signIn('ann@example.com', 'pw');
      expect(u.email, 'ann@example.com');
      expect(auth.user?.id, 'u1');
      expect(c.http.getToken(), 't1');
      expect(await auth.accessToken(), 't1');
      expect(
          await storage.read('monolith.session:https://api.example.com:shop'),
          contains('"refreshToken":"r1"'));

      await auth.signUp('ann@example.com', 'longpassword', name: 'Ann');
      expect(c.http.getToken(), 't2');

      // t2 session still has refresh token r1 (fixture), logout sends it
      await auth.signOut();
      expect(calls.last, 'POST /api/v1/apps/shop/auth/logout');
      expect(auth.state, isA<AuthSignedOut>());
      expect(auth.user, isNull);
      expect(c.http.getToken(), isNull);
      await Future<void>.delayed(Duration.zero);
      expect(
          await storage.read('monolith.session:https://api.example.com:shop'),
          isNull);
      expect(states.whereType<AuthSignedIn>(), isNotEmpty);
      expect(states.last, isA<AuthSignedOut>());
    });

    test('restore loads stored session, discards junk', () async {
      final storage = MemorySessionStorage();
      const key = 'monolith.session:https://api.example.com:shop';
      await storage.write(
          key,
          jsonEncode(sessionJson('stored',
              exp: DateTime.now().add(const Duration(minutes: 10)))));
      final c =
          mk(MockClient((r) async => http.Response('', 500)), storage: storage);
      await c.auth().restore();
      expect(c.auth().state, isA<AuthSignedIn>());
      expect(c.http.getToken(), 'stored');
      expect(c.auth().user?.name, 'Ann');

      final bad = MemorySessionStorage();
      await bad.write(key, 'not json{');
      final c2 =
          mk(MockClient((r) async => http.Response('', 500)), storage: bad);
      await c2.auth().restore();
      expect(c2.auth().state, isA<AuthSignedOut>());
      expect(await bad.read(key), isNull);
    });

    test('restore refreshes an expiring session in the background', () async {
      final storage = MemorySessionStorage();
      const key = 'monolith.session:https://api.example.com:shop';
      await storage.write(
          key,
          jsonEncode(sessionJson('old',
              exp: DateTime.now().add(const Duration(seconds: 5)))));
      var refreshes = 0;
      final done = Completer<void>();
      final c = mk(MockClient((r) async {
        if (r.url.path.endsWith('/auth/refresh')) {
          refreshes++;
          expect(jsonDecode(r.body)['refreshToken'], 'r1');
          done.complete();
          return jsonRes(sessionJson('new',
              refresh: 'r2',
              exp: DateTime.now().add(const Duration(minutes: 15))));
        }
        return http.Response('', 404);
      }), storage: storage);
      await c.auth().restore();
      await done.future;
      await Future<void>.delayed(const Duration(milliseconds: 20));
      expect(refreshes, 1);
      expect(c.http.getToken(), 'new');
      expect(await storage.read(key), contains('"refreshToken":"r2"'));
    });

    test('refresh is single-flight before expiry', () async {
      var refreshes = 0;
      final gate = Completer<void>();
      var now = DateTime.utc(2026, 1, 1, 12);
      final mc = MockClient((r) async {
        final p = r.url.path;
        if (p.endsWith('/auth/login')) {
          return jsonRes(
              sessionJson('a1', exp: now.add(const Duration(minutes: 15))));
        }
        if (p.endsWith('/auth/refresh')) {
          refreshes++;
          await gate.future;
          return jsonRes(sessionJson('a2',
              refresh: 'r2', exp: now.add(const Duration(minutes: 30))));
        }
        if (p.endsWith('/records')) {
          return jsonRes({'items': [], 'auth': r.headers['authorization']});
        }
        return http.Response('', 404);
      });
      final hc = HttpClient(ClientOptions(
          url: 'https://api.example.com', app: 'shop', client: mc));
      final auth = AuthService.configured(hc, now: () => now);
      await auth.signIn('a@b.co', 'pw');
      now =
          now.add(const Duration(minutes: 14, seconds: 45)); // within 30 s skew
      final results = Future.wait([
        auth.accessToken(),
        auth.accessToken(),
        hc.request<Map<String, dynamic>>(
            'GET', '/api/v1/apps/shop/collections/t/records'),
      ]);
      await Future<void>.delayed(const Duration(milliseconds: 20));
      gate.complete();
      final r = await results;
      expect(refreshes, 1);
      expect(r[0], 'a2');
      expect(r[1], 'a2');
      expect((r[2] as Map)['auth'], 'Bearer a2');
    });

    test('a 401 refreshes once and retries once; concurrent 401s share it',
        () async {
      var refreshes = 0;
      final seen = <String>[];
      final mc = MockClient((r) async {
        final p = r.url.path;
        if (p.endsWith('/auth/login')) {
          return jsonRes(sessionJson('old',
              exp: DateTime.now().add(const Duration(minutes: 15))));
        }
        if (p.endsWith('/auth/refresh')) {
          refreshes++;
          await Future<void>.delayed(const Duration(milliseconds: 30));
          return jsonRes(sessionJson('fresh',
              refresh: 'r2',
              exp: DateTime.now().add(const Duration(minutes: 15))));
        }
        final a = r.headers['authorization'] ?? '';
        seen.add(a);
        if (a == 'Bearer old') return problem(401, 'Unauthorized');
        return jsonRes({'ok': true});
      });
      final c = mk(mc);
      await c.auth().signIn('a@b.co', 'pw');
      final rs = await Future.wait([
        c.http.request<Map<String, dynamic>>('GET', '/x'),
        c.http.request<Map<String, dynamic>>('GET', '/y'),
      ]);
      expect(rs.every((m) => m['ok'] == true), isTrue);
      expect(refreshes, 1);
      expect(seen.where((s) => s == 'Bearer fresh'), hasLength(2));
    });

    test('rejected refresh token signs out; second 401 is not retried forever',
        () async {
      var refreshes = 0;
      var hits = 0;
      final c = mk(MockClient((r) async {
        final p = r.url.path;
        if (p.endsWith('/auth/login')) {
          return jsonRes(sessionJson('old',
              exp: DateTime.now().add(const Duration(minutes: 15))));
        }
        if (p.endsWith('/auth/refresh')) {
          refreshes++;
          return problem(401, 'Unauthorized');
        }
        hits++;
        return problem(401, 'Unauthorized');
      }));
      await c.auth().signIn('a@b.co', 'pw');
      await expectLater(c.http.request<Map<String, dynamic>>('GET', '/x'),
          throwsA(isA<MonolithException>()));
      expect(refreshes, 1);
      expect(hits, 1);
      expect(c.auth().state, isA<AuthSignedOut>());
      expect(c.http.getToken(), isNull);
    });

    test('401 retry that still fails throws after one retry', () async {
      var refreshes = 0;
      var hits = 0;
      final c = mk(MockClient((r) async {
        final p = r.url.path;
        if (p.endsWith('/auth/login')) {
          return jsonRes(sessionJson('old',
              exp: DateTime.now().add(const Duration(minutes: 15))));
        }
        if (p.endsWith('/auth/refresh')) {
          refreshes++;
          return jsonRes(sessionJson('fresh',
              exp: DateTime.now().add(const Duration(minutes: 15))));
        }
        hits++;
        return problem(401, 'Unauthorized');
      }));
      await c.auth().signIn('a@b.co', 'pw');
      await expectLater(c.http.request<Map<String, dynamic>>('GET', '/x'),
          throwsA(isA<MonolithException>()));
      expect(refreshes, 1);
      expect(hits, 2);
    });

    test('handleRedirect parses fragment, loads user, stores session',
        () async {
      final storage = MemorySessionStorage();
      final c = mk(MockClient((r) async {
        if (r.url.path.endsWith('/auth/me')) {
          expect(r.headers['authorization'], 'Bearer tok%en+1');
          return jsonRes(userJson('Gina'));
        }
        return http.Response('', 404);
      }), storage: storage);
      final exp = DateTime.now().add(const Duration(minutes: 15)).toUtc();
      final uri = Uri.parse('monolith-shop://auth#token='
          '${Uri.encodeQueryComponent('tok%en+1')}&refreshToken=rr'
          '&expiresAt=${Uri.encodeQueryComponent(exp.toIso8601String())}');
      final u = await c.auth().handleRedirect(uri);
      expect(u?.name, 'Gina');
      expect(c.auth().state, isA<AuthSignedIn>());
      expect(c.http.getToken(), 'tok%en+1');
      expect(
          await storage.read('monolith.session:https://api.example.com:shop'),
          contains('"refreshToken":"rr"'));
    });

    test('handleRedirect: error fragment throws, unrelated uri is null',
        () async {
      final c = mk(MockClient((r) async => http.Response('', 500)));
      await expectLater(
          c.auth().handleRedirect(Uri.parse(
              'monolith-shop://auth#error=oauth_failed&error_description=Denied+by+user')),
          throwsA(isA<MonolithException>()
              .having((e) => e.type, 'type', 'oauth_failed')
              .having((e) => e.detail, 'detail', 'Denied by user')));
      expect(await c.auth().handleRedirect(Uri.parse('monolith-shop://auth')),
          isNull);
      expect(await c.auth().handleRedirect(Uri.parse('https://x.y/#section')),
          isNull);
      // quiet variant pushes to the stream
      final errs = <MonolithException>[];
      c.auth().redirectErrors.listen(errs.add);
      await c
          .auth()
          .handleRedirectQuietly(Uri.parse('a://b#error=access_denied'));
      await Future<void>.delayed(Duration.zero);
      expect(errs, hasLength(1));
    });

    test('signInWithProvider builds the start URL and calls the launcher',
        () async {
      Uri? launched;
      final c = mk(MockClient((r) async => http.Response('', 500)),
          launcher: (u) async {
        launched = u;
        return true;
      });
      final url = await c.auth().signInWithProvider('google');
      expect(url, launched);
      expect(url.path, '/api/v1/apps/shop/auth/oauth/google/start');
      expect(url.queryParameters['redirectUri'], 'monolith-shop://auth');
      final custom = await c
          .auth()
          .signInWithProvider('github', redirectUri: 'https://app.test/cb');
      expect(custom.queryParameters['redirectUri'], 'https://app.test/cb');

      final failing = mk(MockClient((r) async => http.Response('', 500)),
          launcher: (u) async => false);
      await expectLater(failing.auth().signInWithProvider('google'),
          throwsA(isA<MonolithException>()));
    });

    test('providers cached 60 s, updateProfile, changePassword re-login',
        () async {
      var provCalls = 0;
      var now = DateTime.utc(2026, 1, 1);
      final logins = <String>[];
      final mc = MockClient((r) async {
        final p = r.url.path;
        if (p.endsWith('/auth/providers')) {
          provCalls++;
          return jsonRes(publicAuth);
        }
        if (p.endsWith('/auth/login')) {
          logins.add(jsonDecode(r.body)['password'] as String);
          return jsonRes(sessionJson('t${logins.length}',
              exp: now.add(const Duration(minutes: 15))));
        }
        if (p.endsWith('/auth/me') && r.method == 'PATCH') {
          return jsonRes(userJson(jsonDecode(r.body)['name'] as String));
        }
        if (p.endsWith('/auth/password')) return http.Response('', 204);
        return http.Response('', 404);
      });
      final hc = HttpClient(ClientOptions(
          url: 'https://api.example.com', app: 'shop', client: mc));
      final a = AuthService.configured(hc, now: () => now);
      final m = await a.providers();
      expect(m.providers.map((p) => p.id), ['google', 'github', 'acme']);
      expect(m.emailPassword.minPasswordLength, 10);
      await a.providers();
      expect(provCalls, 1);
      now = now.add(const Duration(seconds: 61));
      await a.providers();
      expect(provCalls, 2);
      await a.providers(force: true);
      expect(provCalls, 3);

      await a.signIn('ann@example.com', 'old-password');
      final u = await a.updateProfile(name: 'New');
      expect(u.name, 'New');
      expect(a.user?.name, 'New');
      await a.changePassword('old-password', 'new-password');
      expect(logins, ['old-password', 'new-password']);
      expect(hc.getToken(), 't2');
    });

    test('Monolith.init returns a restored client, same instance per url+app',
        () async {
      await Monolith.reset();
      final storage = MemorySessionStorage();
      await storage.write(
          'monolith.session:https://api.example.com:shop',
          jsonEncode(sessionJson('stored',
              exp: DateTime.now().add(const Duration(minutes: 10)))));
      final a = await Monolith.init(
          url: 'https://api.example.com/',
          app: 'shop',
          storage: storage,
          client: MockClient((r) async => http.Response('', 500)));
      final b =
          await Monolith.init(url: 'https://api.example.com', app: 'shop');
      final other = await Monolith.init(
          url: 'https://api.example.com',
          app: 'other',
          client: MockClient((r) async => http.Response('', 500)));
      expect(identical(a, b), isTrue);
      expect(identical(a, other), isFalse);
      expect(a.auth().state, isA<AuthSignedIn>());
      expect(other.auth().state, isA<AuthSignedOut>());
      await Monolith.reset();
    });
  });

  group('AuthController', () {
    MockClient server({
      Map<String, dynamic> methods = publicAuth,
      http.Response Function(http.Request)? onLogin,
      http.Response Function(http.Request)? onRegister,
      int? providersStatus,
    }) =>
        MockClient((r) async {
          final p = r.url.path;
          if (p.endsWith('/auth/providers')) {
            return providersStatus != null
                ? problem(providersStatus, 'x')
                : jsonRes(methods);
          }
          if (p.endsWith('/auth/login')) {
            return (onLogin ?? (_) => problem(401, 'Unauthorized'))(r);
          }
          if (p.endsWith('/auth/register')) {
            return (onRegister ?? (_) => problem(403, 'Forbidden'))(r);
          }
          return http.Response('', 404);
        });

    http.Response okSession(http.Request _) => jsonRes(
        sessionJson('t', exp: DateTime.now().add(const Duration(minutes: 15))));

    test('starts loading, ends with methods, signed out', () async {
      final ctl = AuthController(mk(server()));
      expect(ctl.state.loading, isTrue);
      await ctl.ready;
      expect(ctl.state.loading, isFalse);
      expect(ctl.state.methods?.providers, hasLength(3));
      expect(ctl.state.user, isNull);
      expect(ctl.state.mode, AuthMode.signIn);
      await ctl.dispose();
    });

    test(
        'email sign-in success sets user; wrong password -> invalid_credentials',
        () async {
      var ok = false;
      final ctl = AuthController(mk(server(
          onLogin: (r) => ok ? okSession(r) : problem(401, 'Unauthorized'))));
      await ctl.ready;
      await ctl.signInWithEmail('ann@example.com', 'bad');
      expect(ctl.state.error?.code, AuthErrorCode.invalidCredentials);
      expect(ctl.state.submitting, isFalse);
      ok = true;
      final seen = <AuthUiState>[];
      ctl.stream.listen(seen.add);
      await ctl.signInWithEmail('ann@example.com', 'good');
      expect(ctl.state.error, isNull);
      expect(ctl.state.user?.id, 'u1');
      expect(seen.first.submitting, isTrue);
      await ctl.dispose();
    });

    test('validation runs before any request', () async {
      var posts = 0;
      final ctl = AuthController(mk(server(onLogin: (r) {
        posts++;
        return okSession(r);
      }, onRegister: (r) {
        posts++;
        return okSession(r);
      })));
      await ctl.ready;
      await ctl.signInWithEmail('nope', 'pw');
      expect(ctl.state.error?.code, AuthErrorCode.invalidEmail);
      await ctl.signUpWithEmail('ann@example.com', 'short');
      expect(ctl.state.error?.code, AuthErrorCode.weakPassword); // min 10
      expect(ctl.state.error?.message, contains('10'));
      expect(posts, 0);
      await ctl.signUpWithEmail('ann@example.com', 'long-enough-pw', 'Ann');
      expect(posts, 1);
      expect(ctl.state.user, isNotNull);
      await ctl.dispose();
    });

    test('error code mapping', () {
      AuthErrorCode code(http.Response r) {
        final body = jsonDecode(r.body) as Map<String, dynamic>;
        return AuthUiError.from(MonolithException(Problem.fromJson(body))).code;
      }

      expect(code(problem(401, 'u')), AuthErrorCode.invalidCredentials);
      expect(code(problem(409, 'u')), AuthErrorCode.emailTaken);
      expect(code(problem(403, 'u')), AuthErrorCode.signupClosed);
      expect(code(problem(400, 'Bad', detail: 'redirectUri is not allowed')),
          AuthErrorCode.redirectNotAllowed);
      expect(
          code(problem(400, 'Bad', errors: [
            {'field': 'password', 'code': 'min', 'message': 'too short'}
          ])),
          AuthErrorCode.weakPassword);
      expect(
          code(problem(400, 'Bad', errors: [
            {'field': 'email', 'code': 'fmt', 'message': 'bad'}
          ])),
          AuthErrorCode.invalidEmail);
      expect(code(problem(500, 'boom')), AuthErrorCode.unknown);
      expect(
          AuthUiError.from(const MonolithException(
                  Problem(type: 'network_error', title: 'n', status: 0)))
              .code,
          AuthErrorCode.network);
      expect(AuthUiError.from(StateError('x')).code, AuthErrorCode.unknown);
    });

    test('email taken and network errors come through', () async {
      final ctl = AuthController(
          mk(server(onRegister: (r) => problem(409, 'Conflict'))));
      await ctl.ready;
      await ctl.signUpWithEmail('ann@example.com', 'long-enough-pw');
      expect(ctl.state.error?.code, AuthErrorCode.emailTaken);
      ctl.dismissError();
      expect(ctl.state.error, isNull);
      await ctl.dispose();

      final net = MockClient((r) async => throw Exception('offline'));
      final ctl2 = AuthController(mk(net));
      await ctl2.ready;
      expect(ctl2.state.error?.code, AuthErrorCode.network);
      expect(ctl2.state.methods, isNull);
      expect(ctl2.showEmailForm, isTrue); // falls back to the form
      await ctl2.dispose();
    });

    test('options: email, signUp, providers, hideProviders', () async {
      final c = mk(server());
      var ctl = AuthController(c);
      await ctl.ready;
      expect(
          ctl.visibleProviders.map((p) => p.id), ['google', 'github', 'acme']);
      expect(ctl.showEmailForm, isTrue);
      expect(ctl.signUpAvailable, isTrue);
      ctl.setMode(AuthMode.signUp);
      expect(ctl.state.mode, AuthMode.signUp);
      await ctl.dispose();

      ctl = AuthController(c,
          options: const AuthUiOptions(
              email: EmailMode.hide,
              signUp: SignUpMode.hide,
              providers: ['github', 'missing', 'google'],
              hideProviders: {'google'}));
      await ctl.ready;
      expect(ctl.visibleProviders.map((p) => p.id), ['github']);
      expect(ctl.showEmailForm, isFalse);
      ctl.setMode(AuthMode.signUp);
      expect(ctl.state.mode, AuthMode.signIn);
      await ctl.dispose();

      ctl = AuthController(c,
          options: const AuthUiOptions(signUp: SignUpMode.only));
      expect(ctl.state.mode, AuthMode.signUp);
      await ctl.ready;
      await ctl.dispose();

      final closed = mk(server(methods: {
        'app': 'shop',
        'emailPassword': {
          'enabled': false,
          'allowSignup': false,
          'minPasswordLength': 8
        },
        'providers': [],
      }));
      ctl = AuthController(closed);
      await ctl.ready;
      expect(ctl.showEmailForm, isFalse);
      expect(ctl.signUpAvailable, isFalse);
      await ctl.dispose();
      ctl = AuthController(closed,
          options: const AuthUiOptions(email: EmailMode.force));
      await ctl.ready;
      expect(ctl.showEmailForm, isTrue);
      await ctl.dispose();
    });

    test('provider sign-in: pending state, redirect completes, errors map',
        () async {
      Uri? launched;
      final mc = MockClient((r) async {
        final p = r.url.path;
        if (p.endsWith('/auth/providers')) return jsonRes(publicAuth);
        if (p.endsWith('/auth/me')) return jsonRes(userJson());
        return http.Response('', 404);
      });
      final c = MonolithClient(ClientOptions(
          url: 'https://api.example.com',
          app: 'shop',
          client: mc,
          urlLauncher: (u) async {
            launched = u;
            return true;
          }));
      final ctl = AuthController(c);
      await ctl.ready;
      await ctl.signInWithProvider('google');
      expect(launched?.path, endsWith('/oauth/google/start'));
      expect(ctl.state.submittingProvider, 'google');
      expect(ctl.state.busy, isTrue);

      await ctl
          .handleRedirect(Uri.parse('monolith-shop://auth#error=oauth_failed'));
      expect(ctl.state.error?.code, AuthErrorCode.providerFailed);
      expect(ctl.state.submittingProvider, isNull);

      await ctl.signInWithProvider('github');
      await ctl.handleRedirect(
          Uri.parse('monolith-shop://auth#token=abc&refreshToken=r'));
      expect(ctl.state.user?.id, 'u1');
      expect(ctl.state.submittingProvider, isNull);
      await ctl.dispose();
    });

    test('start 400 redirect problem becomes redirect_not_allowed with hint',
        () async {
      final c = mk(MockClient((r) async {
        if (r.url.path.endsWith('/auth/providers')) return jsonRes(publicAuth);
        return http.Response('', 404);
      }),
          launcher: (u) async => throw MonolithException(Problem.fromJson({
                'status': 400,
                'title': 'Bad Request',
                'detail': 'redirectUri not allowed'
              })));
      final ctl = AuthController(c);
      await ctl.ready;
      await ctl.signInWithProvider('google');
      expect(ctl.state.error?.code, AuthErrorCode.redirectNotAllowed);
      expect(ctl.state.error?.hint, contains('monolith-shop://auth'));
      expect(ctl.state.submittingProvider, isNull);
      await ctl.dispose();
    });

    test('signOut clears user', () async {
      final ctl = AuthController(mk(server(onLogin: okSession)));
      await ctl.ready;
      await ctl.signInWithEmail('ann@example.com', 'pw');
      expect(ctl.state.user, isNotNull);
      await ctl.signOut();
      expect(ctl.state.user, isNull);
      await ctl.dispose();
    });
  });
}
