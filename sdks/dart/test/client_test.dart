import 'dart:async';
import 'dart:convert';

import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:monolith_client/monolith_client.dart';
import 'package:test/test.dart';

class CustomTodo {
  final String id;
  final String title;
  final bool done;

  CustomTodo({
    required this.id,
    required this.title,
    required this.done,
  });

  factory CustomTodo.fromJson(Map<String, dynamic> json) {
    return CustomTodo(
      id: json['id'] as String? ?? '',
      title: json['title'] as String? ?? '',
      done: json['done'] as bool? ?? false,
    );
  }
}

void main() {
  group('MonolithClient', () {
    test('initializes with default options', () {
      final client = createClient(const ClientOptions(
        url: 'https://api.example.com',
        app: 'testapp',
      ));

      expect(client, isA<MonolithClient>());
      expect(client.http.baseUrl, equals('https://api.example.com'));
      expect(client.http.app, equals('testapp'));
      expect(client.http.getToken(), isNull);
      expect(client.http.getApiKey(), isNull);
    });

    test('builds correct URLs with query parameters', () {
      final client = createClient(const ClientOptions(
        url: 'https://api.example.com/',
        app: 'testapp',
      ));

      final uri = client.http.buildUri('/test', {
        'filter': "status = 'active'",
        'limit': 10,
        'tags': ['a', 'b'],
        'empty': null,
      });

      expect(uri.path, equals('/test'));
      expect(uri.queryParameters['filter'], equals("status = 'active'"));
      expect(uri.queryParameters['limit'], equals('10'));
      expect(uri.queryParametersAll['tags'], equals(['a', 'b']));
      expect(uri.queryParameters.containsKey('empty'), isFalse);
    });

    test('handles collection CRUD requests', () async {
      final mockClient = MockClient((request) async {
        final url = request.url.toString();
        final method = request.method;

        if (url.contains('/collections/todos/records:count')) {
          return http.Response(
            jsonEncode({'count': 42}),
            200,
            headers: {'content-type': 'application/json'},
          );
        }

        if (url.contains('/collections/todos/records/rec_123') &&
            method == 'GET') {
          return http.Response(
            jsonEncode({
              'id': 'rec_123',
              'title': 'Task 1',
              'created': '2026-01-01',
              'updated': '2026-01-01',
              '_v': 1,
            }),
            200,
            headers: {'content-type': 'application/json'},
          );
        }

        if (url.contains('/collections/todos/records') && method == 'POST') {
          final body = jsonDecode(request.body) as Map<String, dynamic>;
          return http.Response(
            jsonEncode({
              'id': 'rec_new',
              ...body,
              'created': '2026-01-01',
              'updated': '2026-01-01',
              '_v': 1,
            }),
            201,
            headers: {'content-type': 'application/json'},
          );
        }

        if (url.contains('/collections/todos/records/rec_123') &&
            method == 'PATCH') {
          final body = jsonDecode(request.body) as Map<String, dynamic>;
          return http.Response(
            jsonEncode({
              'id': 'rec_123',
              ...body,
              'updated': '2026-01-02',
              '_v': 2,
            }),
            200,
            headers: {'content-type': 'application/json'},
          );
        }

        if (url.contains('/collections/todos/records/rec_123') &&
            method == 'PUT') {
          final body = jsonDecode(request.body) as Map<String, dynamic>;
          return http.Response(
            jsonEncode({
              'id': 'rec_123',
              ...body,
              'updated': '2026-01-03',
              '_v': 3,
            }),
            200,
            headers: {'content-type': 'application/json'},
          );
        }

        if (url.contains('/collections/todos/records/rec_123') &&
            method == 'DELETE') {
          return http.Response('', 204);
        }

        if (url.contains('/collections/todos/records') && method == 'GET') {
          return http.Response(
            jsonEncode({
              'items': [
                {'id': 'rec_1', 'title': 'Item 1'},
              ],
              'total': 1,
              'cursor': 'cur_1',
            }),
            200,
            headers: {'content-type': 'application/json'},
          );
        }

        return http.Response('Not found', 404);
      });

      final client = createClient(ClientOptions(
        url: 'https://api.example.com',
        app: 'testapp',
        client: mockClient,
      ));

      final todos = client.collection<RecordModel>('todos');

      // List
      final listRes = await todos.list(filter: 'done = false', limit: 5);
      expect(listRes.items, hasLength(1));
      expect(listRes.items.first.id, equals('rec_1'));
      expect(listRes.items.first['title'], equals('Item 1'));
      expect(listRes.total, equals(1));
      expect(listRes.cursor, equals('cur_1'));

      // Count
      final count = await todos.count();
      expect(count, equals(42));

      // GetOne
      final item = await todos.getOne('rec_123');
      expect(item.id, equals('rec_123'));
      expect(item['title'], equals('Task 1'));
      expect(item.v, equals(1));

      // Create
      final created = await todos.create({'title': 'New Task'});
      expect(created.id, equals('rec_new'));
      expect(created['title'], equals('New Task'));

      // Update
      final updated = await todos.update('rec_123', {'title': 'Updated Task'});
      expect(updated.id, equals('rec_123'));
      expect(updated['title'], equals('Updated Task'));
      expect(updated.v, equals(2));

      // Replace
      final replaced =
          await todos.replace('rec_123', {'title': 'Replaced Task'});
      expect(replaced.id, equals('rec_123'));
      expect(replaced['title'], equals('Replaced Task'));
      expect(replaced.v, equals(3));

      // Delete
      final deleted = await todos.delete('rec_123');
      expect(deleted, isTrue);
    });

    test('supports custom model decoder for collections', () async {
      final mockClient = MockClient((request) async {
        return http.Response(
          jsonEncode({
            'items': [
              {'id': 't_1', 'title': 'Grocery', 'done': false},
              {'id': 't_2', 'title': 'Clean room', 'done': true},
            ],
            'total': 2,
          }),
          200,
          headers: {'content-type': 'application/json'},
        );
      });

      final client = createClient(ClientOptions(
        url: 'https://api.example.com',
        app: 'testapp',
        client: mockClient,
      ));

      final todos = client.collection<CustomTodo>(
        'todos',
        CustomTodo.fromJson,
      );

      final res = await todos.list();
      expect(res.items, hasLength(2));
      expect(res.items[0], isA<CustomTodo>());
      expect(res.items[0].id, equals('t_1'));
      expect(res.items[0].title, equals('Grocery'));
      expect(res.items[0].done, isFalse);
      expect(res.items[1].done, isTrue);
    });

    test('handles problem+json errors with MonolithException', () async {
      final mockClient = MockClient((request) async {
        return http.Response(
          jsonEncode({
            'type': 'validation_error',
            'title': 'Validation Failed',
            'status': 400,
            'detail': 'The title field is required',
            'requestId': 'req_xyz',
            'errors': [
              {
                'field': 'title',
                'code': 'required',
                'message': 'must not be empty',
              }
            ],
          }),
          400,
          headers: {
            'content-type': 'application/problem+json',
            'x-request-id': 'req_xyz',
          },
        );
      });

      final client = createClient(ClientOptions(
        url: 'https://api.example.com',
        app: 'testapp',
        client: mockClient,
      ));

      try {
        await client.collection('todos').create({});
        fail('should have thrown MonolithException');
      } on MonolithException catch (e) {
        expect(e.status, equals(400));
        expect(e.type, equals('validation_error'));
        expect(e.title, equals('Validation Failed'));
        expect(e.detail, equals('The title field is required'));
        expect(e.requestId, equals('req_xyz'));
        expect(e.errors, hasLength(1));
        expect(e.getFieldError('title'), equals('must not be empty'));
        expect(e.getFieldError('nonexistent'), isNull);
      }
    });

    test('manages auth sessions and notifies listeners', () async {
      final mockClient = MockClient((request) async {
        final url = request.url.toString();
        if (url.contains('/auth/login')) {
          final body = jsonDecode(request.body) as Map<String, dynamic>;
          expect(body['email'], equals('test@example.com'));
          expect(body['password'], equals('secret123'));
          return http.Response(
            jsonEncode({
              'token': 'jwt_token_123',
              'user': {'id': 'u_1', 'email': 'test@example.com'},
            }),
            200,
            headers: {'content-type': 'application/json'},
          );
        }

        if (url.contains('/system/install')) {
          return http.Response(
            jsonEncode({'token': 'jwt_superuser_token'}),
            200,
            headers: {'content-type': 'application/json'},
          );
        }

        return http.Response('Not found', 404);
      });

      final client = createClient(ClientOptions(
        url: 'https://api.example.com',
        app: 'testapp',
        client: mockClient,
      ));

      final authNotifications = <String?>[];
      final unsub = client.auth('users').onChange((token) {
        authNotifications.add(token);
      });

      final session = await client
          .auth('users')
          .signInWithPassword('test@example.com', 'secret123');

      expect(session.token, equals('jwt_token_123'));
      expect(session.user?['email'], equals('test@example.com'));
      expect(client.http.getToken(), equals('jwt_token_123'));
      expect(authNotifications, equals(['jwt_token_123']));

      // Sign out
      client.auth('users').signOut();
      expect(client.http.getToken(), isNull);
      expect(authNotifications, equals(['jwt_token_123', null]));

      // Unsubscribe
      unsub();
      client.auth('users').setToken('new_token');
      expect(
          authNotifications, hasLength(2)); // Did not receive new notification

      // Install
      final installSession = await client
          .auth('_superusers')
          .install('setup-token', 'admin@example.com', 'admin-pass');
      expect(installSession.token, equals('jwt_superuser_token'));
      expect(client.http.getToken(), equals('jwt_superuser_token'));
    });

    test('generates file URLs and handles storage file operations', () async {
      final mockClient = MockClient((request) async {
        final url = request.url.toString();
        final method = request.method;

        if (url.contains('/buckets/avatars/objects/user1.png') &&
            method == 'PUT') {
          expect(request.headers['content-type'], equals('image/png'));
          return http.Response(
            jsonEncode({
              'bucket': 'avatars',
              'key': 'user1.png',
              'size': request.bodyBytes.length,
              'contentType': 'image/png',
              'blake3': 'hash123',
              'created': '2026-01-01',
              'updated': '2026-01-01',
            }),
            200,
            headers: {'content-type': 'application/json'},
          );
        }

        if (url.contains('/buckets/avatars/objects/user1.png') &&
            method == 'DELETE') {
          return http.Response('', 204);
        }

        if (url.contains('/buckets/avatars/objects') && method == 'GET') {
          return http.Response(
            jsonEncode({
              'items': [
                {
                  'bucket': 'avatars',
                  'key': 'user1.png',
                  'size': 1234,
                  'contentType': 'image/png',
                  'blake3': 'hash123',
                  'created': '2026-01-01',
                  'updated': '2026-01-01',
                }
              ],
              'nextCursor': 'cursor_abc',
            }),
            200,
            headers: {'content-type': 'application/json'},
          );
        }

        return http.Response('Not found', 404);
      });

      final client = createClient(ClientOptions(
        url: 'https://api.example.com',
        app: 'testapp',
        client: mockClient,
      ));

      final fileUrl = client.files.getURL(
        'avatars',
        'user1.png',
        thumb: '100x100',
      );
      expect(
        fileUrl,
        equals(
          'https://api.example.com/api/v1/apps/testapp/storage/buckets/avatars/objects/user1.png?thumb=100x100',
        ),
      );

      final uploaded = await client.files.upload(
        'avatars',
        'user1.png',
        [1, 2, 3, 4],
        contentType: 'image/png',
      );
      expect(uploaded.bucket, equals('avatars'));
      expect(uploaded.key, equals('user1.png'));
      expect(uploaded.size, equals(4));
      expect(uploaded.contentType, equals('image/png'));

      final list = await client.files.list('avatars');
      expect(list.items, hasLength(1));
      expect(list.items.first.key, equals('user1.png'));
      expect(list.nextCursor, equals('cursor_abc'));

      final deleted = await client.files.delete('avatars', 'user1.png');
      expect(deleted, isTrue);
    });

    test('executes batch operations', () async {
      final mockClient = MockClient((request) async {
        expect(
          request.url.path,
          equals('/api/v1/apps/testapp/batch'),
        );
        final body = jsonDecode(request.body) as Map<String, dynamic>;
        final ops = body['operations'] as List;
        expect(ops, hasLength(2));

        return http.Response(
          jsonEncode([
            {
              'status': 201,
              'body': {'id': '1', 'title': 'First'},
            },
            {
              'status': 204,
              'body': null,
            },
          ]),
          200,
          headers: {'content-type': 'application/json'},
        );
      });

      final client = createClient(ClientOptions(
        url: 'https://api.example.com',
        app: 'testapp',
        client: mockClient,
      ));

      final results = await client.batch([
        const BatchOperation(
          method: 'POST',
          path: '/api/v1/apps/testapp/collections/todos/records',
          body: {'title': 'First'},
        ),
        const BatchOperation(
          method: 'DELETE',
          path: '/api/v1/apps/testapp/collections/todos/records/2',
        ),
      ]);

      expect(results, hasLength(2));
      expect(results[0].status, equals(201));
      expect(results[0].body['id'], equals('1'));
      expect(results[1].status, equals(204));
    });

    test('handles realtime SSE subscriptions and event dispatching', () async {
      final streamController = StreamController<List<int>>();

      final mockClient = MockClient.streaming((request, bodyStream) async {
        expect(request.url.path, equals('/api/v1/apps/testapp/realtime'));
        expect(request.url.queryParameters['collections'], equals('todos'));
        expect(request.headers['accept'], equals('text/event-stream'));

        return http.StreamedResponse(
          streamController.stream,
          200,
          headers: {'content-type': 'text/event-stream'},
        );
      });

      final client = createClient(ClientOptions(
        url: 'https://api.example.com',
        app: 'testapp',
        clientFactory: () => mockClient,
      ));

      final events = <RealtimeEvent<RecordModel>>[];
      final unsub = client.collection<RecordModel>('todos').subscribe((ev) {
        events.add(ev);
      });

      // Allow connect to initiate
      await Future<void>.delayed(const Duration(milliseconds: 50));

      // Emit SSE events including comment/ping
      streamController.add(utf8.encode(':ping\n\n'));
      streamController.add(utf8.encode(
        'event: create\n'
        'data: {"collection":"todos","action":"create","record":{"id":"rec_1","title":"RT Item","_v":1}}\n\n',
      ));

      await Future<void>.delayed(const Duration(milliseconds: 50));

      expect(events, hasLength(1));
      expect(events.first.action, equals('create'));
      expect(events.first.collection, equals('todos'));
      expect(events.first.record.id, equals('rec_1'));
      expect(events.first.record['title'], equals('RT Item'));

      unsub();
      client.close();
      await streamController.close();
    });
  });
}
