package com.isper.mobile

import android.content.Intent
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.toggleable
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import com.isper.mobile.core.Stage
import java.util.Locale

private val ptBR: Locale = Locale.forLanguageTag("pt-BR")

private fun Float.fmt(digits: Int = 1) = String.format(ptBR, "%.${digits}f", this)

@Composable
fun SpikeScreen(state: SpikeUiState, actions: SpikeViewModel, modifier: Modifier = Modifier) {
    // A tela não apaga no meio de uma medição (o Android pode derrubar o
    // processo em segundo plano; o gravador da 9.2 é que roda num serviço).
    val view = LocalView.current
    DisposableEffect(state.busy) {
        view.keepScreenOn = state.busy
        onDispose { view.keepScreenOn = false }
    }
    val picker = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        uri?.let(actions::pickAudio)
    }

    run {
        LazyColumn(
            modifier = modifier.fillMaxSize(),
            contentPadding = androidx.compose.foundation.layout.PaddingValues(16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            item { Header() }
            when {
                state.missingCpu.isNotEmpty() -> item {
                    Notice(stringResource(R.string.device_unsupported, state.missingCpu.joinToString(", ")))
                }
                state.loadError != null -> item {
                    Notice(stringResource(R.string.device_load_failed, state.loadError))
                }
            }
            item { DeviceCard(state) }
            if (state.engine != null) {
                item { ModelCard(state, actions) }
                item {
                    AudioCard(state, actions) {
                        picker.launch(arrayOf("audio/*", "video/mp4"))
                    }
                }
                item { RunCard(state, actions) }
            }
            state.message?.let { msg -> item { Notice(msg) } }
            state.result?.let { result -> item { ResultCard(result) } }
        }
    }
}

@Composable
private fun Header() {
    Column(Modifier.padding(top = 8.dp, bottom = 4.dp)) {
        Text(
            stringResource(R.string.app_name).uppercase(),
            style = MaterialTheme.typography.labelMedium,
            color = MaterialTheme.colorScheme.primary,
        )
        Text(stringResource(R.string.lab_title), style = MaterialTheme.typography.headlineMedium)
        Spacer(Modifier.padding(top = 4.dp))
        Text(
            stringResource(R.string.lab_subtitle),
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

@Composable
private fun Section(title: String, content: @Composable () -> Unit) {
    Card(
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
        modifier = Modifier.fillMaxWidth(),
    ) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text(
                title.uppercase(),
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            content()
        }
    }
}

@Composable
private fun Notice(text: String) {
    Card(
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceVariant),
        modifier = Modifier.fillMaxWidth(),
    ) {
        Text(text, Modifier.padding(16.dp), color = MaterialTheme.colorScheme.error)
    }
}

@Composable
private fun Line(label: String, value: String) {
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
        Text(label, color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.bodyMedium)
        Spacer(Modifier.width(12.dp))
        Text(value, style = MaterialTheme.typography.bodyMedium, fontWeight = FontWeight.Medium)
    }
}

@Composable
private fun DeviceCard(state: SpikeUiState) {
    val d = state.device ?: return
    Section(stringResource(R.string.section_device)) {
        Text("${d.manufacturer} ${d.model}", style = MaterialTheme.typography.titleMedium)
        d.soc?.let { Line(stringResource(R.string.device_soc), it) }
        Line(stringResource(R.string.device_cores), "${state.engine?.cpuThreads ?: "?"} · ${d.abi}")
        Line(stringResource(R.string.device_ram), "${d.totalRamMb} MB")
        Line(stringResource(R.string.device_system), d.system)
        state.power?.let { p ->
            Line(
                stringResource(R.string.device_battery),
                listOfNotNull(
                    p.batteryPercent?.let { "$it%" },
                    p.temperatureC?.let { "${it.fmt()} °C" },
                    p.thermal,
                ).joinToString(" · "),
            )
        }
    }
}

@Composable
private fun ModelCard(state: SpikeUiState, actions: SpikeViewModel) {
    Section(stringResource(R.string.section_model)) {
        state.models.forEach { m ->
            val selected = state.selectedModel == m.option.file
            Row(
                verticalAlignment = Alignment.CenterVertically,
                modifier = Modifier
                    .fillMaxWidth()
                    .selectable(
                        selected = selected,
                        enabled = !state.busy,
                        role = Role.RadioButton,
                        onClick = { actions.selectModel(m.option.file) },
                    ),
            ) {
                RadioButton(selected = selected, onClick = null, enabled = !state.busy)
                Column(Modifier.weight(1f).padding(start = 8.dp)) {
                    Text(m.option.label, style = MaterialTheme.typography.bodyLarge)
                    Text(
                        m.option.note,
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                if (m.installed) {
                    Text(
                        stringResource(R.string.model_installed),
                        style = MaterialTheme.typography.labelMedium,
                        color = MaterialTheme.colorScheme.secondary,
                    )
                } else {
                    TextButton(onClick = { actions.download(m.option.file) }, enabled = !state.busy) {
                        Text(stringResource(R.string.model_size, m.option.approxMb.toInt()))
                    }
                }
            }
        }
    }
}

@Composable
private fun AudioCard(state: SpikeUiState, actions: SpikeViewModel, onPick: () -> Unit) {
    Section(stringResource(R.string.section_audio)) {
        val sample = state.audio is AudioChoice.Sample
        Row(
            verticalAlignment = Alignment.CenterVertically,
            modifier = Modifier
                .fillMaxWidth()
                .selectable(selected = sample, enabled = !state.busy, role = Role.RadioButton, onClick = actions::useSample),
        ) {
            RadioButton(selected = sample, onClick = null, enabled = !state.busy)
            Column(Modifier.padding(start = 8.dp)) {
                Text(stringResource(R.string.audio_sample))
                Text(
                    stringResource(if (state.sampleHasReference) R.string.audio_sample_reference else R.string.audio_sample_plain),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        (state.audio as? AudioChoice.Picked)?.let {
            Text(stringResource(R.string.audio_picked, it.name), style = MaterialTheme.typography.bodyMedium)
        }
        OutlinedButton(onClick = onPick, enabled = !state.busy) { Text(stringResource(R.string.audio_pick)) }
        HorizontalDivider(Modifier.padding(vertical = 4.dp))
        Row(
            verticalAlignment = Alignment.CenterVertically,
            modifier = Modifier
                .fillMaxWidth()
                .toggleable(value = state.diarize, enabled = !state.busy, role = Role.Switch, onValueChange = actions::setDiarize),
        ) {
            Column(Modifier.weight(1f)) {
                Text(stringResource(R.string.diarize))
                Text(
                    stringResource(R.string.diarize_note),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            Switch(checked = state.diarize, onCheckedChange = null, enabled = !state.busy)
        }
    }
}

@Composable
private fun RunCard(state: SpikeUiState, actions: SpikeViewModel) {
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        if (state.busy) {
            val p = state.progress
            val label = when (p?.stage) {
                Stage.DOWNLOAD -> stringResource(R.string.stage_download)
                Stage.DECODE -> stringResource(R.string.stage_decode)
                Stage.TRANSCRIBE -> stringResource(R.string.stage_transcribe)
                Stage.DIARIZE -> stringResource(R.string.stage_diarize)
                null -> ""
            }
            val fraction = p?.fraction
            Text(
                if (fraction != null && p.stage != Stage.DIARIZE) "$label · ${(fraction * 100).toInt()}%" else label,
                style = MaterialTheme.typography.bodyMedium,
            )
            if (fraction != null && p.stage != Stage.DIARIZE) {
                LinearProgressIndicator(progress = { fraction }, modifier = Modifier.fillMaxWidth())
            } else {
                LinearProgressIndicator(modifier = Modifier.fillMaxWidth())
            }
            OutlinedButton(onClick = actions::cancel, modifier = Modifier.fillMaxWidth()) {
                Text(stringResource(R.string.cancel))
            }
        } else {
            val ready = state.models.any { it.option.file == state.selectedModel && it.installed }
            Button(
                onClick = actions::run,
                enabled = state.selectedModel != null,
                modifier = Modifier.fillMaxWidth(),
            ) { Text(stringResource(R.string.run)) }
            if (!ready) {
                Text(
                    stringResource(R.string.need_model),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}

@Composable
private fun ResultCard(result: SpikeResult) {
    val r = result.report
    val context = LocalContext.current
    Section(stringResource(R.string.section_result)) {
        Text(
            stringResource(R.string.result_rtf, r.realtimeFactor.fmt(2)),
            style = MaterialTheme.typography.headlineSmall,
            color = MaterialTheme.colorScheme.primary,
        )
        Text(
            stringResource(if (r.realtimeFactor < 1f) R.string.result_faster else R.string.result_slower),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Text(r.model, style = MaterialTheme.typography.bodySmall, fontFamily = FontFamily.Monospace)
        HorizontalDivider(Modifier.padding(vertical = 4.dp))
        Line(stringResource(R.string.result_total), "${r.totalSecs.fmt()} s de ${r.audioSecs.fmt()} s")
        Line(stringResource(R.string.result_transcribe), "${r.transcribeSecs.fmt()} s")
        if (r.diarizeSecs > 0f) Line(stringResource(R.string.result_diarize), "${r.diarizeSecs.fmt()} s")
        Line(stringResource(R.string.result_load), "${r.loadSecs.fmt()} s")
        r.peakRssMb?.let { Line(stringResource(R.string.result_memory), "${it.fmt(0)} MB") }
        val b = result.before
        val a = result.after
        if (b.batteryPercent != null && a.batteryPercent != null) {
            Line(stringResource(R.string.result_battery), "${b.batteryPercent}% → ${a.batteryPercent}%")
        }
        if (b.temperatureC != null && a.temperatureC != null) {
            Line(stringResource(R.string.result_temperature), "${b.temperatureC.fmt()} → ${a.temperatureC.fmt()} °C · ${a.thermal}")
        }
        if (r.speakers > 0u) Line(stringResource(R.string.result_speakers), "${r.speakers}")
        if (r.wer != null) {
            val quality = listOfNotNull(r.wer, r.cer, r.der).joinToString(" · ") { "${(it * 100).fmt()}%" }
            Line(stringResource(R.string.result_quality), quality)
        }
        HorizontalDivider(Modifier.padding(vertical = 4.dp))
        Text(stringResource(R.string.result_transcript), style = MaterialTheme.typography.titleSmall)
        r.utterances.take(12).forEach { u ->
            val who = u.speaker?.let { stringResource(R.string.speaker_label, it.toInt() + 1) }
                ?: stringResource(R.string.speaker_unknown)
            Column {
                Text(
                    "$who · ${u.startSecs.fmt(0)} s",
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.tertiary,
                )
                Text(u.text, style = MaterialTheme.typography.bodyMedium)
            }
        }
        val shareTitle = stringResource(R.string.result_share_title)
        Button(
            onClick = {
                val send = Intent(Intent.ACTION_SEND)
                    .setType("text/plain")
                    .putExtra(Intent.EXTRA_SUBJECT, shareTitle)
                    .putExtra(Intent.EXTRA_TEXT, result.json)
                context.startActivity(Intent.createChooser(send, shareTitle))
            },
            modifier = Modifier.fillMaxWidth(),
        ) { Text(stringResource(R.string.result_share)) }
    }
}
