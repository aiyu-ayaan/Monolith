import 'package:flutter/material.dart';

/// Overrides on top of the app's [ThemeData]. Every field is optional; null keeps the app's
/// value, so the screen follows system dark mode unless you force colors.
class AuthTheme {
  const AuthTheme({
    this.primary,
    this.onPrimary,
    this.background,
    this.surface,
    this.foreground,
    this.error,
    this.radius = 12,
    this.fontFamily,
    this.buttonStyle,
    this.maxWidth = 400,
    this.padding = const EdgeInsets.all(24),
  });

  final Color? primary;
  final Color? onPrimary;
  final Color? background;
  final Color? surface;
  final Color? foreground;
  final Color? error;

  /// Corner radius of fields, buttons and the card.
  final double radius;
  final String? fontFamily;

  /// Merged over the filled (email submit) button style.
  final ButtonStyle? buttonStyle;
  final double maxWidth;
  final EdgeInsets padding;

  /// The app theme with these overrides applied.
  ThemeData apply(ThemeData base) {
    final cs = base.colorScheme.copyWith(
      primary: primary,
      onPrimary: onPrimary,
      surface: surface,
      onSurface: foreground,
      error: error,
    );
    final shape = RoundedRectangleBorder(
      borderRadius: BorderRadius.circular(radius),
    );
    var t = base.copyWith(
      colorScheme: cs,
      scaffoldBackgroundColor: background ?? base.scaffoldBackgroundColor,
      inputDecorationTheme: base.inputDecorationTheme.copyWith(
        border: OutlineInputBorder(borderRadius: BorderRadius.circular(radius)),
      ),
      cardTheme: base.cardTheme.copyWith(shape: shape),
      filledButtonTheme: FilledButtonThemeData(
        style: FilledButton.styleFrom(
          minimumSize: const Size.fromHeight(48),
          shape: shape,
        ).merge(buttonStyle),
      ),
    );
    if (fontFamily != null) {
      t = t.copyWith(textTheme: t.textTheme.apply(fontFamily: fontFamily));
    }
    return t;
  }
}
