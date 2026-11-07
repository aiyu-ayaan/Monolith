package dev.monolith.compose

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import dev.monolith.client.MonolithClient
import dev.monolith.client.auth.AppUser
import kotlinx.coroutines.launch
import androidx.compose.runtime.rememberCoroutineScope

/** First letter of the name, else of the email, else `?`. */
fun userInitial(user: AppUser): String =
    (user.name.trim().ifEmpty { user.email.orEmpty().trim() }).firstOrNull()?.uppercase() ?: "?"

/** Avatar initial with the name (or email) next to it. */
@Composable
fun UserChip(user: AppUser, modifier: Modifier = Modifier) {
    Row(modifier, verticalAlignment = Alignment.CenterVertically) {
        Box(Modifier.size(32.dp).background(MaterialTheme.colorScheme.primaryContainer, CircleShape), Alignment.Center) {
            Text(userInitial(user), color = MaterialTheme.colorScheme.onPrimaryContainer)
        }
        Spacer(Modifier.size(8.dp))
        Column {
            Text(user.name.ifBlank { user.email.orEmpty() }, style = MaterialTheme.typography.bodyMedium)
            if (user.name.isNotBlank() && !user.email.isNullOrBlank()) {
                Text(user.email.orEmpty(), style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }
    }
}

/** Signs the current user out. */
@Composable
fun SignOutButton(mono: MonolithClient, modifier: Modifier = Modifier, label: String = AuthLabels().signOut) {
    val scope = rememberCoroutineScope()
    OutlinedButton(onClick = { scope.launch { mono.auth.signOut() } }, modifier = modifier.padding(0.dp)) { Text(label) }
}
