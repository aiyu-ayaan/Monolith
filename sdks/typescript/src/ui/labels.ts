export interface AuthLabels {
  signInTitle: string;
  signUpTitle: string;
  email: string;
  password: string;
  name: string;
  signIn: string;
  signUp: string;
  signingIn: string;
  redirecting: string;
  switchToSignUp: string;
  switchToSignIn: string;
  continueWith: string;
  or: string;
  terms: string;
  privacy: string;
  loading: string;
  signedInAs: string;
  signOut: string;
  dismiss: string;
  noMethods: string;
  retry: string;
  error_invalid_credentials: string;
  error_email_taken: string;
  error_weak_password: string;
  error_invalid_email: string;
  error_signup_closed: string;
  error_network: string;
  error_redirect_not_allowed: string;
  error_provider_failed: string;
  error_unknown: string;
}

/** `{app}` and `{name}` are replaced where they appear. */
export const defaultLabels: AuthLabels = {
  signInTitle: "Sign in to {app}",
  signUpTitle: "Create your {app} account",
  email: "Email",
  password: "Password",
  name: "Name (optional)",
  signIn: "Sign in",
  signUp: "Create account",
  signingIn: "Please wait...",
  redirecting: "Redirecting...",
  switchToSignUp: "No account? Create one",
  switchToSignIn: "Already have an account? Sign in",
  continueWith: "Continue with {name}",
  or: "or",
  terms: "Terms of service",
  privacy: "Privacy policy",
  loading: "Loading...",
  signedInAs: "Signed in as {name}",
  signOut: "Sign out",
  dismiss: "Dismiss",
  noMethods: "No sign-in method is enabled for this app.",
  retry: "Try again",
  error_invalid_credentials: "Wrong email or password.",
  error_email_taken: "An account with this email already exists.",
  error_weak_password: "Choose a longer password.",
  error_invalid_email: "Enter a valid email address.",
  error_signup_closed: "Sign-up is closed for this app.",
  error_network: "Cannot reach the server. Check your connection and try again.",
  error_redirect_not_allowed: "This page is not an allowed redirect URL for sign-in.",
  error_provider_failed: "Sign-in with the provider failed. Try again.",
  error_unknown: "Something went wrong. Try again.",
};

export function resolveLabels(overrides?: Partial<AuthLabels>): AuthLabels {
  return { ...defaultLabels, ...overrides };
}

export function fill(template: string, vars: Record<string, string>): string {
  return template.replace(/\{(\w+)\}/g, (m, k: string) => vars[k] ?? m);
}
