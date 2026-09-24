package com.isper.mobile.recording

import android.Manifest
import android.content.pm.PackageManager
import android.os.StatFs
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.FilledIconButton
import androidx.compose.material3.IconButtonDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedIconButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.content.ContextCompat
import com.isper.mobile.R

/**
 * Bytes por hora do Ogg/Opus a 32 kbit/s no celular: o microfone entra a
 * 48 kHz e o VBR fica perto da meta (198 KB em 48 s num Galaxy Tab A9). O
 * corpus de 16 kHz dá 10,5 MB/h, mas não é o caso do aparelho (ADR 0016).
 */
private const val BYTES_PER_HOUR = 15_000_000.0

@Composable
fun RecordScreen(state: RecState, modifier: Modifier = Modifier) {
    val context = LocalContext.current
    var denied by remember { mutableStateOf(false) }
    val askPermissions = rememberLauncherForActivityResult(
        ActivityResultContracts.RequestMultiplePermissions(),
    ) { granted ->
        if (granted[Manifest.permission.RECORD_AUDIO] == true) {
            denied = false
            RecordingService.start(context)
        } else {
            denied = true
        }
    }
    val startRecording = {
        if (ContextCompat.checkSelfPermission(context, Manifest.permission.RECORD_AUDIO) ==
            PackageManager.PERMISSION_GRANTED
        ) {
            RecordingService.start(context)
        } else {
            askPermissions.launch(RecordShortcutActivity.permissionsToAsk())
        }
    }
    val hoursLeft = remember(state is RecState.Active) {
        runCatching {
            StatFs(Storage.recordingsDir(context).path).availableBytes / BYTES_PER_HOUR
        }.getOrNull()
    }

    Column(
        modifier = modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(horizontal = 20.dp, vertical = 16.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        when (state) {
            RecState.Idle -> IdleRecorder(onRecord = startRecording, denied = denied, hoursLeft = hoursLeft)
            is RecState.Active -> ActiveRecorder(state, hoursLeft)
        }
    }
}

@Composable
private fun IdleRecorder(onRecord: () -> Unit, denied: Boolean, hoursLeft: Double?) {
    Spacer(Modifier.height(24.dp))
    Text(
        stringResource(R.string.rec_idle_title),
        style = MaterialTheme.typography.headlineMedium,
        textAlign = TextAlign.Center,
    )
    Spacer(Modifier.height(8.dp))
    Text(
        stringResource(R.string.rec_idle_subtitle),
        style = MaterialTheme.typography.bodyMedium,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        textAlign = TextAlign.Center,
    )
    Spacer(Modifier.height(48.dp))
    val start = stringResource(R.string.rec_start)
    FilledIconButton(
        onClick = onRecord,
        modifier = Modifier
            .size(120.dp)
            .testTag("gravar")
            .semantics { contentDescription = start },
        shape = CircleShape,
        colors = IconButtonDefaults.filledIconButtonColors(containerColor = MaterialTheme.colorScheme.primary),
    ) {
        Box(
            Modifier
                .size(40.dp)
                .background(MaterialTheme.colorScheme.onPrimary, CircleShape),
        )
    }
    Spacer(Modifier.height(16.dp))
    Text(start, style = MaterialTheme.typography.titleMedium)
    hoursLeft?.let {
        Spacer(Modifier.height(6.dp))
        Text(
            stringResource(R.string.rec_space_left, it.toInt()),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
    if (denied) {
        Spacer(Modifier.height(16.dp))
        Text(
            stringResource(R.string.rec_permission_denied),
            color = MaterialTheme.colorScheme.error,
            textAlign = TextAlign.Center,
        )
    }
    Spacer(Modifier.height(40.dp))
    Surface(
        color = MaterialTheme.colorScheme.surfaceVariant,
        shape = RoundedCornerShape(12.dp),
        modifier = Modifier.fillMaxWidth(),
    ) {
        Text(
            stringResource(R.string.rec_tip_shortcuts),
            modifier = Modifier.padding(16.dp),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

@Composable
private fun ActiveRecorder(state: RecState.Active, hoursLeft: Double?) {
    val context = LocalContext.current
    StatusChip(state)
    Spacer(Modifier.height(28.dp))
    Text(
        formatDuration(state.elapsedSecs),
        fontFamily = FontFamily.Serif,
        fontSize = 64.sp,
        fontWeight = FontWeight.Normal,
        modifier = Modifier.semantics {
            contentDescription = formatDuration(state.elapsedSecs)
        },
    )
    hoursLeft?.let {
        Text(
            stringResource(R.string.rec_space_left, it.toInt()),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
    Spacer(Modifier.height(24.dp))
    Waveform(levels = state.levels, dimmed = state.paused || state.silenced)
    Spacer(Modifier.height(12.dp))
    if (state.moments.isNotEmpty()) {
        Text(
            state.moments.takeLast(4).joinToString("   ") { "★ ${formatDuration(it)}" },
            style = MaterialTheme.typography.labelMedium,
            fontFamily = FontFamily.Monospace,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
    Spacer(Modifier.height(40.dp))
    Row(
        horizontalArrangement = Arrangement.spacedBy(28.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        val mark = stringResource(R.string.rec_mark)
        OutlinedIconButton(
            onClick = { RecordingService.send(context, RecordingService.ACTION_MARK) },
            modifier = Modifier.size(64.dp).testTag("marcar").semantics { contentDescription = mark },
        ) { Text("★", fontSize = 24.sp) }

        val stop = stringResource(R.string.rec_stop)
        FilledIconButton(
            onClick = { RecordingService.send(context, RecordingService.ACTION_STOP) },
            modifier = Modifier.size(96.dp).testTag("parar").semantics { contentDescription = stop },
            shape = CircleShape,
            colors = IconButtonDefaults.filledIconButtonColors(containerColor = MaterialTheme.colorScheme.primary),
        ) {
            Box(
                Modifier
                    .size(30.dp)
                    .background(MaterialTheme.colorScheme.onPrimary, RoundedCornerShape(6.dp)),
            )
        }

        val pauseLabel = stringResource(if (state.paused) R.string.rec_resume else R.string.rec_pause)
        OutlinedIconButton(
            onClick = {
                RecordingService.send(
                    context,
                    if (state.paused) RecordingService.ACTION_RESUME else RecordingService.ACTION_PAUSE,
                )
            },
            modifier = Modifier.size(64.dp).testTag("pausar").semantics { contentDescription = pauseLabel },
        ) { Text(if (state.paused) "▶" else "❚❚", fontSize = 20.sp) }
    }
    Spacer(Modifier.height(12.dp))
    Text(
        stringResource(R.string.rec_screen_off_ok),
        style = MaterialTheme.typography.bodySmall,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        textAlign = TextAlign.Center,
    )
}

@Composable
private fun StatusChip(state: RecState.Active) {
    val (label, color) = when {
        state.silenced -> stringResource(R.string.rec_status_silenced) to MaterialTheme.colorScheme.error
        state.paused -> stringResource(R.string.rec_status_paused) to MaterialTheme.colorScheme.onSurfaceVariant
        else -> stringResource(R.string.rec_status_recording) to MaterialTheme.colorScheme.primary
    }
    val pulse = rememberInfiniteTransition(label = "pulso")
    val alpha by pulse.animateFloat(
        initialValue = 1f,
        targetValue = 0.3f,
        animationSpec = infiniteRepeatable(tween(800), RepeatMode.Reverse),
        label = "alfa",
    )
    Surface(color = color.copy(alpha = 0.14f), shape = RoundedCornerShape(50)) {
        Row(
            Modifier.padding(horizontal = 14.dp, vertical = 7.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Box(
                Modifier
                    .size(8.dp)
                    .alpha(if (state.paused) 1f else alpha)
                    .background(color, CircleShape),
            )
            Spacer(Modifier.width(8.dp))
            Text(label, color = color, style = MaterialTheme.typography.labelLarge)
        }
    }
}

@Composable
private fun Waveform(levels: List<Float>, dimmed: Boolean) {
    val color = MaterialTheme.colorScheme.onSurface.copy(alpha = if (dimmed) 0.25f else 0.8f)
    Canvas(
        Modifier
            .fillMaxWidth()
            .height(72.dp),
    ) {
        val bars = 48
        val gap = 3.dp.toPx()
        val w = (size.width - gap * (bars - 1)) / bars
        val recent = levels.takeLast(bars)
        for (i in 0 until bars) {
            val level = recent.getOrNull(i - (bars - recent.size)) ?: 0f
            val h = (size.height * (0.06f + 0.94f * level)).coerceAtMost(size.height)
            drawRoundRect(
                color = color,
                topLeft = Offset(i * (w + gap), (size.height - h) / 2),
                size = Size(w, h),
                cornerRadius = CornerRadius(w / 2, w / 2),
            )
        }
    }
}
