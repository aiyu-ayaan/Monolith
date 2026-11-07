import 'package:flutter/material.dart';
import 'package:monolith_flutter/monolith_flutter.dart';

Future<void> main() async {
  final mono = await MonolithFlutter.init(
    url: 'https://api.example.com',
    app: 'shop',
  );
  runApp(
    MaterialApp(
      home: Scaffold(
        body: AuthGate(
          client: mono,
          child: Center(child: SignOutButton(client: mono)),
        ),
      ),
    ),
  );
}
