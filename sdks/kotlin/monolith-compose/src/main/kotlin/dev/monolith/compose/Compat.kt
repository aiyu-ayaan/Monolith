package dev.monolith.compose

import androidx.compose.runtime.Composable
import androidx.compose.runtime.State
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.platform.LocalLifecycleOwner
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import kotlinx.coroutines.flow.StateFlow

@Composable
internal fun <T> StateFlow<T>.collectAsStateCompat(): State<T> = collectAsState()

/** Runs [onResume] each time the host lifecycle resumes. */
@Composable
internal fun LifecycleResumeEffect(onResume: () -> Unit) {
    val owner = LocalLifecycleOwner.current
    val latest = rememberUpdatedState(onResume)
    DisposableEffect(owner) {
        val observer = LifecycleEventObserver { _, event -> if (event == Lifecycle.Event.ON_RESUME) latest.value() }
        owner.lifecycle.addObserver(observer)
        onDispose { owner.lifecycle.removeObserver(observer) }
    }
}
