package dev.monolith.compose

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalUriHandler
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.monolith.client.MonolithClient
import dev.monolith.client.auth.AuthController
import dev.monolith.client.auth.AuthMode
import dev.monolith.client.auth.AuthState
import dev.monolith.client.auth.AuthUiState

/** Creates an [AuthController] tied to the composition and applies [options] when they change. */
@Composable
fun rememberAuthController(mono: MonolithClient, options: AuthUiOptions = AuthUiOptions()): AuthController {
    val controller = remember(mono) { AuthController(mono, options.auth) }
    LaunchedEffect(controller, options.auth) { controller.setOptions(options.auth) }
    DisposableEffect(controller) { onDispose { controller.close() } }
    return controller
}

/**
 * The drop-in sign-in screen: providers the server enables, an email form with a sign-in/sign-up switch,
 * loading and error states. Pair it with [AuthGate] to show it until the user is signed in.
 */
@Composable
fun MonolithAuth(mono: MonolithClient, modifier: Modifier = Modifier, options: AuthUiOptions = AuthUiOptions()) {
    val controller = rememberAuthController(mono, options)
    val state by controller.state.collectAsStateCompat()
    val onSignedIn by rememberUpdatedState(options.onSignedIn)
    val onError by rememberUpdatedState(options.onError)
    var notified by remember { mutableStateOf<String?>(null) }

    LaunchedEffect(state.user?.id) {
        val u = state.user
        if (u != null && u.id != notified) {
            notified = u.id
            onSignedIn?.invoke(u)
        } else if (u == null) {
            notified = null
        }
    }
    LaunchedEffect(state.error) { state.error?.let { onError?.invoke(it) } }
    LifecycleResumeEffect { controller.onAppResumed() }

    MonolithAuthContent(state, controller, modifier, options, onRetry = { controller.reloadMethods() })
}

/** Stateless version of [MonolithAuth]: draws [state] and calls [actions]. */
@Composable
fun MonolithAuthContent(
    state: AuthUiState,
    actions: dev.monolith.client.auth.AuthActions,
    modifier: Modifier = Modifier,
    options: AuthUiOptions = AuthUiOptions(),
    onRetry: () -> Unit = {}
) {
    val labels = options.labels
    val radius = RoundedCornerShape(options.theme.cornerRadiusDp.dp)
    val body: @Composable ColumnScope.() -> Unit = {
        Header(state, options)
        val err = state.error
        if (err != null) {
            options.errorBanner?.invoke(err, actions::dismissError) ?: ErrorBanner(err.message, labels.dismiss, actions::dismissError)
        }
        when {
            state.loading && state.methods == null && state.providers.isEmpty() -> Box(Modifier.fillMaxWidth().padding(24.dp), Alignment.Center) {
                CircularProgressIndicator(Modifier.semantics { contentDescription = "Loading" })
            }
            state.methods == null -> TextButton(onClick = onRetry, modifier = Modifier.align(Alignment.CenterHorizontally)) { Text(labels.retry) }
            !state.showEmailForm && state.providers.isEmpty() -> Text(labels.nothingToOffer, style = MaterialTheme.typography.bodyMedium)
            else -> {
                state.providers.forEach { p ->
                    val ctx = ProviderButtonContext(p, state.activeProvider == p.id, !state.submitting) { actions.signInWithProvider(p.id) }
                    options.providerButton?.invoke(ctx) ?: ProviderButton(ctx, labels, radius)
                    Spacer(Modifier.height(8.dp))
                }
                if (state.providers.isNotEmpty() && state.showEmailForm) OrDivider(labels.or)
                if (state.showEmailForm) EmailForm(state, actions, options, radius)
            }
        }
        (options.footer ?: { t, p -> DefaultFooter(t, p, labels) })(this, options.termsUrl, options.privacyUrl)
    }
    when (options.layout) {
        AuthLayout.Card -> Box(modifier.fillMaxWidth().padding(16.dp), Alignment.Center) {
            Card(
                Modifier.widthIn(max = 420.dp).fillMaxWidth(),
                shape = radius,
                colors = CardDefaults.cardColors(containerColor = options.theme.surface ?: MaterialTheme.colorScheme.surface)
            ) {
                Column(Modifier.padding(24.dp), verticalArrangement = Arrangement.spacedBy(4.dp), content = body)
            }
        }
        AuthLayout.Fullscreen -> Surface(modifier.fillMaxSize(), color = options.theme.surface ?: MaterialTheme.colorScheme.background) {
            Box(Modifier.fillMaxSize().padding(24.dp), Alignment.Center) {
                Column(Modifier.widthIn(max = 420.dp).fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(4.dp), content = body)
            }
        }
        AuthLayout.Plain -> Column(modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(4.dp), content = body)
    }
}

@Composable
private fun ColumnScope.Header(state: AuthUiState, options: AuthUiOptions) {
    val l = options.labels
    val title = options.title ?: if (state.mode == AuthMode.SignUp) l.signUpTitle else l.signInTitle
    options.header?.let { it(this, title); return }
    Text(title, style = MaterialTheme.typography.headlineSmall)
    options.subtitle?.let { Text(it, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant) }
    Spacer(Modifier.height(12.dp))
}

@Composable
private fun ErrorBanner(message: String, dismissLabel: String, onDismiss: () -> Unit) {
    Surface(
        color = MaterialTheme.colorScheme.errorContainer,
        contentColor = MaterialTheme.colorScheme.onErrorContainer,
        shape = RoundedCornerShape(8.dp),
        modifier = Modifier.fillMaxWidth().semantics { contentDescription = "Error: $message" }
    ) {
        Row(Modifier.padding(start = 12.dp), verticalAlignment = Alignment.CenterVertically) {
            Text(message, Modifier.weight(1f), style = MaterialTheme.typography.bodyMedium)
            TextButton(onClick = onDismiss) { Text(dismissLabel) }
        }
    }
    Spacer(Modifier.height(8.dp))
}

@Composable
private fun OrDivider(text: String) {
    Row(Modifier.fillMaxWidth().padding(vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
        HorizontalDivider(Modifier.weight(1f))
        Text(text, Modifier.padding(horizontal = 12.dp), style = MaterialTheme.typography.labelMedium)
        HorizontalDivider(Modifier.weight(1f))
    }
}

@Composable
private fun ProviderButton(ctx: ProviderButtonContext, labels: AuthLabels, shape: RoundedCornerShape) {
    val brand = ProviderBrands.forProvider(ctx.provider.id, ctx.provider.name)
    val text = labels.continueWith(brand.name)
    OutlinedButton(
        onClick = ctx.onClick,
        enabled = ctx.enabled,
        shape = shape,
        border = BorderStroke(1.dp, MaterialTheme.colorScheme.outline),
        modifier = Modifier.fillMaxWidth().semantics { contentDescription = text }
    ) {
        if (ctx.busy) {
            CircularProgressIndicator(Modifier.size(20.dp), strokeWidth = 2.dp)
        } else {
            Box(Modifier.size(22.dp).background(brand.color, CircleShape), Alignment.Center) {
                Text(brand.letter, color = androidx.compose.ui.graphics.Color.White, fontSize = 11.sp)
            }
        }
        Spacer(Modifier.size(12.dp))
        Text(text)
    }
}

@Composable
private fun ColumnScope.EmailForm(
    state: AuthUiState,
    actions: dev.monolith.client.auth.AuthActions,
    options: AuthUiOptions,
    shape: RoundedCornerShape
) {
    val l = options.labels
    var email by rememberSaveable { mutableStateOf("") }
    var password by rememberSaveable { mutableStateOf("") }
    var name by rememberSaveable { mutableStateOf("") }
    var visible by rememberSaveable { mutableStateOf(false) }
    val signUp = state.mode == AuthMode.SignUp
    val enabled = !state.submitting

    OutlinedTextField(
        email, { email = it }, Modifier.fillMaxWidth(), enabled = enabled, singleLine = true, shape = shape,
        label = { Text(l.email) }, keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Email)
    )
    if (signUp) {
        OutlinedTextField(name, { name = it }, Modifier.fillMaxWidth(), enabled = enabled, singleLine = true, shape = shape, label = { Text(l.name) })
    }
    OutlinedTextField(
        password, { password = it }, Modifier.fillMaxWidth(), enabled = enabled, singleLine = true, shape = shape,
        label = { Text(l.password) },
        visualTransformation = if (visible) VisualTransformation.None else PasswordVisualTransformation(),
        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password),
        trailingIcon = {
            TextButton(onClick = { visible = !visible }) { Text(if (visible) l.hidePassword else l.showPassword, fontSize = 12.sp) }
        },
        supportingText = if (signUp) {
            { Text("Min. ${state.minPasswordLength} characters") }
        } else null
    )
    Spacer(Modifier.height(8.dp))
    val theme = options.theme
    Button(
        onClick = { if (signUp) actions.signUpWithEmail(email, password, name) else actions.signInWithEmail(email, password) },
        enabled = enabled,
        shape = shape,
        colors = ButtonDefaults.buttonColors(
            containerColor = theme.primary ?: MaterialTheme.colorScheme.primary,
            contentColor = theme.onPrimary ?: MaterialTheme.colorScheme.onPrimary
        ),
        modifier = Modifier.fillMaxWidth()
    ) {
        if (state.submitting && state.activeProvider == null) CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp)
        else Text(if (signUp) l.signUpButton else l.signInButton)
    }
    if (state.canSignUp) {
        TextButton(
            onClick = { actions.setMode(if (signUp) AuthMode.SignIn else AuthMode.SignUp) },
            modifier = Modifier.align(Alignment.CenterHorizontally)
        ) { Text(if (signUp) l.switchToSignIn else l.switchToSignUp) }
    }
}

@Composable
private fun ColumnScope.DefaultFooter(termsUrl: String?, privacyUrl: String?, labels: AuthLabels) {
    if (termsUrl == null && privacyUrl == null) return
    val uri = LocalUriHandler.current
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.Center) {
        termsUrl?.let { TextButton(onClick = { uri.openUri(it) }) { Text(labels.terms, fontSize = 12.sp, textAlign = TextAlign.Center) } }
        privacyUrl?.let { TextButton(onClick = { uri.openUri(it) }) { Text(labels.privacy, fontSize = 12.sp, textAlign = TextAlign.Center) } }
    }
}

/** Shows [MonolithAuth] until the user is signed in, then [content]. */
@Composable
fun AuthGate(
    mono: MonolithClient,
    modifier: Modifier = Modifier,
    options: AuthUiOptions = AuthUiOptions(),
    loading: @Composable () -> Unit = {},
    content: @Composable () -> Unit
) {
    val auth by mono.auth.state.collectAsStateCompat()
    when (auth) {
        AuthState.Unknown -> loading()
        AuthState.SignedOut -> MonolithAuth(mono, modifier, options)
        is AuthState.SignedIn -> content()
    }
}
