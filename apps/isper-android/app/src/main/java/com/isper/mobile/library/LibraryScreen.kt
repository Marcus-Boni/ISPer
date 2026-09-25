package com.isper.mobile.library

import android.content.Intent
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.core.content.FileProvider
import com.isper.mobile.R
import com.isper.mobile.core.GapKind
import com.isper.mobile.core.RecordingInfo
import com.isper.mobile.core.RecordingState
import com.isper.mobile.core.RemoteStage
import com.isper.mobile.sync.PcSync
import com.isper.mobile.recording.formatDuration
import java.io.File
import java.time.OffsetDateTime
import java.time.format.DateTimeFormatter
import java.util.Locale

private val dateFormat = DateTimeFormatter.ofPattern("EEE, d 'de' MMM · HH:mm", Locale.forLanguageTag("pt-BR"))

private fun title(info: RecordingInfo): String =
    runCatching { OffsetDateTime.parse(info.startedAt).format(dateFormat) }
        .getOrDefault(info.id)
        .replaceFirstChar { it.uppercase() }

private fun size(bytes: ULong): String {
    val mb = bytes.toDouble() / 1_000_000.0
    return if (mb >= 1) String.format(Locale.forLanguageTag("pt-BR"), "%.1f MB", mb)
    else String.format(Locale.forLanguageTag("pt-BR"), "%.0f KB", bytes.toDouble() / 1000.0)
}

@Composable
fun LibraryScreen(
    state: LibraryState,
    actions: LibraryViewModel,
    onMeasure: (RecordingInfo) -> Unit,
    onOpenMinutes: (RecordingInfo) -> Unit,
    modifier: Modifier = Modifier,
) {
    val visible = state.recordings.filterNot { it.id in state.pendingDelete }
    var expanded by rememberSaveable { mutableStateOf<String?>(null) }
    val pc by actions.pc.collectAsState()
    val sending by actions.sending.collectAsState()
    val lastError by actions.lastError.collectAsState()
    val finished = visible.filter { it.state != RecordingState.RECORDING }
    val waiting = finished.count { it.remote == RemoteStage.QUEUED || it.remote == RemoteStage.PROCESSING }
    val unsent = finished.count { it.remote == RemoteStage.NOT_SENT || it.remote == RemoteStage.SENDING }

    PairDialogs(state.pair, actions)

    LazyColumn(
        modifier = modifier.fillMaxSize(),
        contentPadding = PaddingValues(16.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        item {
            Column(Modifier.padding(top = 8.dp, bottom = 8.dp)) {
                Text(stringResource(R.string.lib_title), style = MaterialTheme.typography.headlineMedium)
                Text(
                    stringResource(R.string.lib_subtitle),
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        item {
            PcCard(
                pc = pc,
                sending = sending,
                waiting = waiting,
                unsent = unsent,
                lastError = lastError,
                actions = actions,
            )
        }
        if (!state.loading && visible.isEmpty()) {
            item {
                Surface(
                    color = MaterialTheme.colorScheme.surfaceVariant,
                    shape = RoundedCornerShape(12.dp),
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Text(
                        stringResource(R.string.lib_empty),
                        modifier = Modifier.padding(20.dp),
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        }
        items(visible, key = { it.id }) { info ->
            RecordingCard(
                info = info,
                expanded = expanded == info.id,
                playing = state.playingId == info.id,
                playingPositionMs = state.playingPosition,
                onToggle = { expanded = if (expanded == info.id) null else info.id },
                actions = actions,
                onMeasure = { onMeasure(info) },
                paired = pc != null,
                sendingFraction = sending?.takeIf { it.id == info.id }?.fraction,
                onOpenMinutes = { onOpenMinutes(info) },
            )
        }
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun RecordingCard(
    info: RecordingInfo,
    expanded: Boolean,
    playing: Boolean,
    playingPositionMs: Int,
    onToggle: () -> Unit,
    actions: LibraryViewModel,
    onMeasure: () -> Unit,
    paired: Boolean,
    sendingFraction: Float?,
    onOpenMinutes: () -> Unit,
) {
    val context = LocalContext.current
    Card(
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
        modifier = Modifier
            .fillMaxWidth()
            .clickable(onClick = onToggle),
    ) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Text(title(info), style = MaterialTheme.typography.titleMedium)
            Text(
                listOf(info.durationSecs?.let(::formatDuration) ?: "—", size(info.sizeBytes)).joinToString(" · "),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            val silenced = info.gaps.count { it.kind == GapKind.SILENCED }
            FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                when (info.state) {
                    RecordingState.RECOVERED -> Chip(stringResource(R.string.lib_chip_recovered), MaterialTheme.colorScheme.error)
                    RecordingState.IMPORTED -> Chip(
                        info.sourceName?.let { stringResource(R.string.lib_chip_imported_from, it) }
                            ?: stringResource(R.string.lib_chip_imported),
                        MaterialTheme.colorScheme.tertiary,
                    )
                    RecordingState.RECORDING -> Chip(stringResource(R.string.lib_chip_recording), MaterialTheme.colorScheme.primary)
                    RecordingState.FINISHED -> {}
                }
                if (info.moments.isNotEmpty()) Chip("★ ${info.moments.size}", MaterialTheme.colorScheme.onSurfaceVariant)
                if (silenced > 0) {
                    Chip(stringResource(R.string.lib_chip_silenced, silenced), MaterialTheme.colorScheme.onSurfaceVariant)
                }
                if (info.state != RecordingState.RECORDING) RemoteChip(info, paired, sendingFraction)
            }
            if (info.remote == RemoteStage.FAILED && !info.remoteError.isNullOrBlank()) {
                Text(
                    info.remoteError.orEmpty(),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.error,
                )
            }
            if (info.minutesPath != null || info.remote == RemoteStage.FAILED) {
                Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                    if (info.minutesPath != null) {
                        TextButton(onClick = onOpenMinutes, modifier = Modifier.testTag("ver-ata-${info.id}")) {
                            Text(stringResource(R.string.lib_open_minutes))
                        }
                    }
                    if (info.remote == RemoteStage.FAILED) {
                        TextButton(onClick = { actions.retry(info) }) { Text(stringResource(R.string.lib_retry)) }
                    }
                }
            }
            if (playing) {
                val total = ((info.durationSecs ?: 1.0) * 1000).coerceAtLeast(1.0)
                LinearProgressIndicator(
                    progress = { (playingPositionMs / total).toFloat().coerceIn(0f, 1f) },
                    modifier = Modifier.fillMaxWidth().padding(top = 4.dp),
                )
            }
            if (expanded && info.state != RecordingState.RECORDING) {
                Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                    TextButton(onClick = { actions.togglePlay(info) }) {
                        Text(stringResource(if (playing) R.string.lib_stop_playing else R.string.lib_play))
                    }
                    TextButton(onClick = { share(context, info) }) { Text(stringResource(R.string.lib_share)) }
                    TextButton(onClick = onMeasure) { Text(stringResource(R.string.lib_measure)) }
                    TextButton(onClick = { actions.delete(info) }) {
                        Text(stringResource(R.string.lib_delete), color = MaterialTheme.colorScheme.error)
                    }
                }
            }
        }
    }
}

/** Em que pé a gravação está no PC. */
@Composable
private fun RemoteChip(info: RecordingInfo, paired: Boolean, sendingFraction: Float?) {
    val (text, color) = when (info.remote) {
        RemoteStage.NOT_SENT ->
            if (paired) stringResource(R.string.lib_remote_not_sent) to MaterialTheme.colorScheme.onSurfaceVariant
            else return
        RemoteStage.SENDING ->
            (sendingFraction?.let { stringResource(R.string.lib_remote_sending, (it * 100).toInt()) }
                ?: stringResource(R.string.lib_remote_partial)) to MaterialTheme.colorScheme.primary
        RemoteStage.QUEUED -> stringResource(R.string.lib_remote_queued) to MaterialTheme.colorScheme.primary
        RemoteStage.PROCESSING -> stringResource(R.string.lib_remote_processing) to MaterialTheme.colorScheme.primary
        RemoteStage.READY -> stringResource(R.string.lib_remote_ready) to MaterialTheme.colorScheme.tertiary
        RemoteStage.FAILED -> stringResource(R.string.lib_remote_failed) to MaterialTheme.colorScheme.error
    }
    Chip(text, color, textModifier = Modifier.testTag("remoto-${info.id}"))
}

@Composable
private fun Chip(text: String, color: Color, textModifier: Modifier = Modifier) {
    Surface(color = color.copy(alpha = 0.12f), shape = RoundedCornerShape(50)) {
        Text(
            text,
            color = color,
            style = MaterialTheme.typography.labelMedium,
            // A etiqueta de teste vai no texto: é ele que o uiautomator lê.
            modifier = Modifier.padding(horizontal = 10.dp, vertical = 4.dp).then(textModifier),
        )
    }
}

private fun share(context: android.content.Context, info: RecordingInfo) {
    val file = File(info.audioPath)
    val uri = FileProvider.getUriForFile(context, "${context.packageName}.arquivos", file)
    val type = when (file.extension.lowercase()) {
        "opus", "ogg", "oga" -> "audio/ogg"
        "mp3" -> "audio/mpeg"
        "m4a", "mp4", "aac" -> "audio/mp4"
        "wav" -> "audio/wav"
        "flac" -> "audio/flac"
        else -> "audio/*"
    }
    val send = Intent(Intent.ACTION_SEND)
        .setType(type)
        .putExtra(Intent.EXTRA_STREAM, uri)
        .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
    context.startActivity(Intent.createChooser(send, context.getString(R.string.lib_share_title)))
}
