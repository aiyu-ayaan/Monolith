import 'package:monolith_client/monolith_client.dart';

/// Every string of the sign-in UI. Override what you need (translation); the rest stays English.
///
/// `{name}` in [continueWith] is replaced by the provider name, `{min}` in
/// [weakPassword] by the minimum password length.
class AuthLabels {
  const AuthLabels({
    this.signInTitle = 'Sign in',
    this.signUpTitle = 'Create account',
    this.email = 'Email',
    this.password = 'Password',
    this.name = 'Name (optional)',
    this.signInButton = 'Sign in',
    this.signUpButton = 'Create account',
    this.noAccount = "Don't have an account?",
    this.goToSignUp = 'Sign up',
    this.haveAccount = 'Already have an account?',
    this.goToSignIn = 'Sign in',
    this.or = 'or',
    this.continueWith = 'Continue with {name}',
    this.showPassword = 'Show password',
    this.hidePassword = 'Hide password',
    this.loading = 'Loading',
    this.dismiss = 'Dismiss',
    this.termsPrefix = 'By continuing you agree to the',
    this.terms = 'Terms',
    this.and = 'and',
    this.privacy = 'Privacy Policy',
    this.signOut = 'Sign out',
    this.invalidCredentials = 'Wrong email or password.',
    this.emailTaken = 'An account with this email already exists.',
    this.weakPassword = 'The password must have at least {min} characters.',
    this.invalidEmail = 'Enter a valid email address.',
    this.signupClosed = 'Sign-up is not available.',
    this.network = 'Cannot reach the server. Check your connection.',
    this.redirectNotAllowed = 'This sign-in redirect is not allowed.',
    this.providerFailed = 'Sign-in with this provider failed.',
    this.unknownError = 'Something went wrong.',
  });

  final String signInTitle;
  final String signUpTitle;
  final String email;
  final String password;
  final String name;
  final String signInButton;
  final String signUpButton;
  final String noAccount;
  final String goToSignUp;
  final String haveAccount;
  final String goToSignIn;
  final String or;
  final String continueWith;
  final String showPassword;
  final String hidePassword;
  final String loading;
  final String dismiss;
  final String termsPrefix;
  final String terms;
  final String and;
  final String privacy;
  final String signOut;
  final String invalidCredentials;
  final String emailTaken;
  final String weakPassword;
  final String invalidEmail;
  final String signupClosed;
  final String network;
  final String redirectNotAllowed;
  final String providerFailed;
  final String unknownError;

  String continueWithName(String providerName) =>
      continueWith.replaceAll('{name}', providerName);

  /// Text for [error]. For unrecognised server problems the server's own message is kept.
  String errorText(AuthUiError error, int minPasswordLength) {
    switch (error.code) {
      case AuthErrorCode.invalidCredentials:
        return invalidCredentials;
      case AuthErrorCode.emailTaken:
        return emailTaken;
      case AuthErrorCode.weakPassword:
        return weakPassword.replaceAll('{min}', '$minPasswordLength');
      case AuthErrorCode.invalidEmail:
        return invalidEmail;
      case AuthErrorCode.signupClosed:
        return signupClosed;
      case AuthErrorCode.network:
        return network;
      case AuthErrorCode.redirectNotAllowed:
        return redirectNotAllowed;
      case AuthErrorCode.providerFailed:
        // the provider's own explanation (e.g. "access_denied") helps more than a fixed text
        return error.message ==
                AuthUiError.defaultMessage(AuthErrorCode.providerFailed)
            ? providerFailed
            : error.message;
      case AuthErrorCode.unknown:
        return error.message ==
                AuthUiError.defaultMessage(AuthErrorCode.unknown)
            ? unknownError
            : error.message;
    }
  }
}
