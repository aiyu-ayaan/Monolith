import 'dart:math' as math;

import 'package:flutter/material.dart';
import 'package:monolith_client/monolith_client.dart';

/// Look of one sign-in button: brand name, colors and whether the glyph keeps its own colors.
class ProviderBrand {
  const ProviderBrand({
    required this.name,
    required this.background,
    required this.foreground,
    this.outlined = false,
    this.coloredGlyph = false,
  });

  final String name;
  final Color background;
  final Color foreground;

  /// Draw a border (neutral surface buttons).
  final bool outlined;

  /// The glyph has its own colors (Google, Microsoft); otherwise it is drawn in [foreground].
  final bool coloredGlyph;

  static const knownIds = [
    'google',
    'github',
    'apple',
    'microsoft',
    'facebook',
    'discord',
    'gitlab',
    'x',
    'linkedin',
    'twitch',
    'oidc',
  ];

  /// Brand for [id]. Unknown ids get a neutral button (name comes from the server).
  static ProviderBrand of(String id, ThemeData theme) {
    final dark = theme.brightness == Brightness.dark;
    final surface = theme.colorScheme.surface;
    final onSurface = theme.colorScheme.onSurface;
    ProviderBrand neutral(String name, {bool colored = false}) => ProviderBrand(
      name: name,
      background: surface,
      foreground: onSurface,
      outlined: true,
      coloredGlyph: colored,
    );
    // near-black brands invert in dark mode so they stay visible
    ProviderBrand black(String name) => dark
        ? ProviderBrand(
            name: name,
            background: const Color(0xFFFFFFFF),
            foreground: const Color(0xFF000000),
          )
        : ProviderBrand(
            name: name,
            background: const Color(0xFF000000),
            foreground: const Color(0xFFFFFFFF),
          );
    switch (id) {
      case 'google':
        return neutral('Google', colored: true);
      case 'microsoft':
        return neutral('Microsoft', colored: true);
      case 'github':
        return dark
            ? const ProviderBrand(
                name: 'GitHub',
                background: Color(0xFFFFFFFF),
                foreground: Color(0xFF24292F),
              )
            : const ProviderBrand(
                name: 'GitHub',
                background: Color(0xFF24292F),
                foreground: Color(0xFFFFFFFF),
              );
      case 'apple':
        return black('Apple');
      case 'x':
        return black('X');
      case 'facebook':
        return const ProviderBrand(
          name: 'Facebook',
          background: Color(0xFF1877F2),
          foreground: Color(0xFFFFFFFF),
        );
      case 'discord':
        return const ProviderBrand(
          name: 'Discord',
          background: Color(0xFF5865F2),
          foreground: Color(0xFFFFFFFF),
        );
      case 'gitlab':
        return const ProviderBrand(
          name: 'GitLab',
          background: Color(0xFFFC6D26),
          foreground: Color(0xFFFFFFFF),
        );
      case 'linkedin':
        return const ProviderBrand(
          name: 'LinkedIn',
          background: Color(0xFF0A66C2),
          foreground: Color(0xFFFFFFFF),
        );
      case 'twitch':
        return const ProviderBrand(
          name: 'Twitch',
          background: Color(0xFF9146FF),
          foreground: Color(0xFFFFFFFF),
        );
      case 'oidc':
        return neutral('Single sign-on');
      default:
        return neutral(id);
    }
  }
}

/// Provider glyph drawn with a [CustomPainter] (no asset files).
///
/// These are simplified vector approximations, not the official artwork. Replace them through
/// the `providerButton` slot where a brand's rules require the original logo.
class ProviderIcon extends StatelessWidget {
  const ProviderIcon({
    super.key,
    required this.id,
    this.size = 20,
    this.color,
    this.background,
    this.colored = false,
  });

  final String id;
  final double size;

  /// Glyph color for single-color glyphs.
  final Color? color;

  /// Color used for cut-outs inside a glyph (the button background).
  final Color? background;

  /// Use the brand's own colors where the glyph has them (Google, Microsoft).
  final bool colored;

  @override
  Widget build(BuildContext context) {
    final fg = color ?? Theme.of(context).colorScheme.onSurface;
    final bg = background ?? Theme.of(context).colorScheme.surface;
    return ExcludeSemantics(
      child: SizedBox(
        width: size,
        height: size,
        child: CustomPaint(painter: ProviderGlyphPainter(id, fg, bg, colored)),
      ),
    );
  }
}

/// Paints the glyph of [id] in a 24 x 24 box scaled to the canvas.
class ProviderGlyphPainter extends CustomPainter {
  ProviderGlyphPainter(this.id, this.fg, this.bg, this.colored);

  final String id;
  final Color fg;
  final Color bg;
  final bool colored;

  @override
  void paint(Canvas canvas, Size size) {
    canvas.save();
    canvas.scale(size.width / 24, size.height / 24);
    final fill = Paint()..color = fg;
    final cut = Paint()..color = bg;
    switch (id) {
      case 'google':
        _google(canvas);
      case 'microsoft':
        _microsoft(canvas);
      case 'github':
        _github(canvas, fill);
      case 'apple':
        _apple(canvas, fill, cut);
      case 'facebook':
        _text(canvas, 'f', 22, fg);
      case 'x':
        _text(canvas, 'X', 19, fg);
      case 'linkedin':
        _text(canvas, 'in', 16, fg);
      case 'discord':
        canvas.drawRRect(
          RRect.fromRectAndRadius(
            const Rect.fromLTWH(2.5, 5.5, 19, 13),
            const Radius.circular(6),
          ),
          fill,
        );
        canvas.drawCircle(const Offset(9, 12), 2, cut);
        canvas.drawCircle(const Offset(15, 12), 2, cut);
      case 'gitlab':
        canvas.drawPath(
          Path()
            ..moveTo(12, 21)
            ..lineTo(2.5, 12)
            ..lineTo(5, 3.5)
            ..lineTo(8, 10)
            ..lineTo(16, 10)
            ..lineTo(19, 3.5)
            ..lineTo(21.5, 12)
            ..close(),
          fill,
        );
      case 'twitch':
        canvas.drawPath(
          Path()
            ..moveTo(4, 3)
            ..lineTo(20, 3)
            ..lineTo(20, 14)
            ..lineTo(15.5, 14)
            ..lineTo(12, 18)
            ..lineTo(12, 14)
            ..lineTo(4, 14)
            ..close(),
          fill,
        );
        canvas.drawRect(const Rect.fromLTWH(9, 6.5, 1.8, 4.5), cut);
        canvas.drawRect(const Rect.fromLTWH(13.5, 6.5, 1.8, 4.5), cut);
      default:
        _key(canvas);
    }
    canvas.restore();
  }

  void _google(Canvas canvas) {
    final rect = Rect.fromCircle(center: const Offset(12, 12), radius: 7.5);
    Paint stroke(Color c) => Paint()
      ..color = colored ? c : fg
      ..style = PaintingStyle.stroke
      ..strokeWidth = 4.2;
    double rad(double deg) => deg * math.pi / 180;
    canvas.drawArc(
      rect,
      rad(-150),
      rad(105),
      false,
      stroke(const Color(0xFFEA4335)),
    );
    canvas.drawArc(
      rect,
      rad(150),
      rad(60),
      false,
      stroke(const Color(0xFFFBBC05)),
    );
    canvas.drawArc(
      rect,
      rad(60),
      rad(90),
      false,
      stroke(const Color(0xFF34A853)),
    );
    canvas.drawArc(
      rect,
      rad(0),
      rad(60),
      false,
      stroke(const Color(0xFF4285F4)),
    );
    canvas.drawLine(
      const Offset(12, 12),
      const Offset(19.5, 12),
      stroke(const Color(0xFF4285F4)),
    );
  }

  void _microsoft(Canvas canvas) {
    Paint p(Color c) => Paint()..color = colored ? c : fg;
    canvas.drawRect(
      const Rect.fromLTWH(3, 3, 8.5, 8.5),
      p(const Color(0xFFF25022)),
    );
    canvas.drawRect(
      const Rect.fromLTWH(12.5, 3, 8.5, 8.5),
      p(const Color(0xFF7FBA00)),
    );
    canvas.drawRect(
      const Rect.fromLTWH(3, 12.5, 8.5, 8.5),
      p(const Color(0xFF00A4EF)),
    );
    canvas.drawRect(
      const Rect.fromLTWH(12.5, 12.5, 8.5, 8.5),
      p(const Color(0xFFFFB900)),
    );
  }

  void _github(Canvas canvas, Paint fill) {
    canvas.drawCircle(const Offset(12, 13.5), 6.5, fill);
    canvas.drawPath(
      Path()
        ..moveTo(5.8, 11)
        ..lineTo(6.3, 4)
        ..lineTo(11, 8)
        ..close()
        ..moveTo(18.2, 11)
        ..lineTo(17.7, 4)
        ..lineTo(13, 8)
        ..close(),
      fill,
    );
  }

  void _apple(Canvas canvas, Paint fill, Paint cut) {
    canvas.drawCircle(const Offset(9.3, 14.5), 5.3, fill);
    canvas.drawCircle(const Offset(14.7, 14.5), 5.3, fill);
    canvas.drawRect(const Rect.fromLTWH(9.3, 9.5, 5.4, 10), fill);
    canvas.drawCircle(const Offset(20, 12.5), 2.6, cut);
    canvas.drawPath(
      Path()
        ..moveTo(12, 9)
        ..quadraticBezierTo(11.5, 4.5, 15.5, 3)
        ..quadraticBezierTo(16, 7.5, 12, 9)
        ..close(),
      fill,
    );
  }

  void _key(Canvas canvas) {
    final s = Paint()
      ..color = fg
      ..style = PaintingStyle.stroke
      ..strokeWidth = 2.2
      ..strokeCap = StrokeCap.round;
    canvas.drawCircle(const Offset(8, 12), 4.2, s);
    canvas.drawLine(const Offset(12.2, 12), const Offset(21, 12), s);
    canvas.drawLine(const Offset(18, 12), const Offset(18, 15.5), s);
  }

  void _text(Canvas canvas, String t, double fontSize, Color c) {
    final tp = TextPainter(
      text: TextSpan(
        text: t,
        style: TextStyle(
          color: c,
          fontSize: fontSize,
          fontWeight: FontWeight.w800,
          height: 1,
        ),
      ),
      textDirection: TextDirection.ltr,
    )..layout();
    tp.paint(canvas, Offset((24 - tp.width) / 2, (24 - tp.height) / 2));
  }

  @override
  bool shouldRepaint(ProviderGlyphPainter old) =>
      old.id != id || old.fg != fg || old.bg != bg || old.colored != colored;
}

/// The built-in "Continue with {name}" button.
class ProviderButton extends StatelessWidget {
  const ProviderButton({
    super.key,
    required this.provider,
    required this.label,
    required this.onPressed,
    this.loading = false,
    this.radius = 12,
  });

  final AuthProviderInfo provider;

  /// Full text, e.g. "Continue with Google".
  final String label;
  final VoidCallback? onPressed;
  final bool loading;
  final double radius;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final brand = ProviderBrand.of(provider.id, theme);
    final border = BorderSide(
      color: brand.outlined
          ? theme.colorScheme.outlineVariant
          : Colors.transparent,
    );
    return Semantics(
      button: true,
      enabled: onPressed != null,
      label: label,
      excludeSemantics: true,
      onTap: onPressed,
      child: SizedBox(
        height: 48,
        width: double.infinity,
        child: TextButton(
          onPressed: onPressed,
          style: TextButton.styleFrom(
            backgroundColor: brand.background,
            foregroundColor: brand.foreground,
            disabledBackgroundColor: brand.background.withAlpha(120),
            disabledForegroundColor: brand.foreground.withAlpha(120),
            side: border,
            shape: RoundedRectangleBorder(
              borderRadius: BorderRadius.circular(radius),
            ),
          ),
          child: Row(
            mainAxisAlignment: MainAxisAlignment.center,
            children: [
              if (loading)
                SizedBox(
                  width: 20,
                  height: 20,
                  child: CircularProgressIndicator(
                    strokeWidth: 2,
                    color: brand.foreground,
                  ),
                )
              else
                ProviderIcon(
                  id: provider.id,
                  color: brand.foreground,
                  background: brand.background,
                  colored: brand.coloredGlyph,
                ),
              const SizedBox(width: 12),
              Flexible(
                child: Text(
                  label,
                  overflow: TextOverflow.ellipsis,
                  style: const TextStyle(fontWeight: FontWeight.w600),
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}
