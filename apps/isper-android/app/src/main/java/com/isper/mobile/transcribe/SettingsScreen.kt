package com.isper.mobile.transcribe

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.dp
import com.isper.mobile.DeviceProbe
import com.isper.mobile.R
import com.isper.mobile.core.mobileModels
import java.util.Locale

private fun megabytes(bytes: Long): String =
    String.format(Locale.forLanguageTag("pt-BR"), "%.0f MB", bytes / 1_000_000.0)

private fun gigabytes(mb: Long): String =
    String.format(Locale.forLanguageTag("pt-BR"), "%.1f GB", mb / 1024.0)

/**
 * Ajustes da transcrição no celular (Fase 9.4): quando transcrever, com que
 * modelo, se separa os falantes aqui, e os modelos que estão no aparelho.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun SettingsScreen(onBack: () -> Unit, modifier: Modifier = Modifier) {
    val context = LocalContext.current
    var setup by remember { mutableStateOf(LocalTranscribe.setup(context)) }
    var bytes by remember { mutableLongStateOf(LocalTranscribe.modelsBytes(context)) }
    val models = remember { mobileModels() }
    val ramMb = remember { DeviceProbe.info(context).totalRamMb }
    fun reload() {
        setup = LocalTranscribe.setup(context)
        bytes = LocalTranscribe.modelsBytes(context)
    }

    Column(
        modifier = modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        TextButton(onClick = onBack, modifier = Modifier.testTag("ajustes-voltar")) {
            Text("← " + stringResource(R.string.settings_back))
        }
        Text(stringResource(R.string.settings_title), style = MaterialTheme.typography.headlineMedium)

        Card(colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface)) {
            Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(stringResource(R.string.settings_local_title), style = MaterialTheme.typography.titleMedium)
                Text(
                    stringResource(R.string.settings_local_intro),
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Text(
                    stringResource(R.string.settings_device, gigabytes(ramMb), setup.plan.reason),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )

                HorizontalDivider(Modifier.padding(vertical = 4.dp))
                Text(stringResource(R.string.settings_when), style = MaterialTheme.typography.titleSmall)
                ModeOption(
                    TranscribeMode.CHARGING, setup.mode, R.string.settings_mode_charging, R.string.settings_mode_charging_desc,
                ) { LocalTranscribe.setMode(context, it); reload() }
                ModeOption(
                    TranscribeMode.NOW, setup.mode, R.string.settings_mode_now, R.string.settings_mode_now_desc,
                ) { LocalTranscribe.setMode(context, it); reload() }
                ModeOption(
                    TranscribeMode.PC_ONLY, setup.mode, R.string.settings_mode_pc, R.string.settings_mode_pc_desc,
                ) { LocalTranscribe.setMode(context, it); reload() }

                HorizontalDivider(Modifier.padding(vertical = 4.dp))
                Text(stringResource(R.string.settings_model), style = MaterialTheme.typography.titleSmall)
                val auto = models.firstOrNull { it.file == setup.plan.modelFile }
                Choice(
                    selected = setup.modelChoice == null,
                    title = stringResource(R.string.settings_model_auto, auto?.label ?: setup.plan.modelFile),
                    detail = auto?.let { stringResource(R.string.settings_model_size, it.approxMb.toInt()) },
                    tag = "modelo-auto",
                ) { LocalTranscribe.setModel(context, null); reload() }
                models.forEach { m ->
                    Choice(
                        selected = setup.modelChoice == m.file,
                        title = m.label,
                        detail = stringResource(R.string.settings_model_size, m.approxMb.toInt()) + " · " + m.note,
                        tag = "modelo-${m.file}",
                    ) { LocalTranscribe.setModel(context, m.file); reload() }
                }

                HorizontalDivider(Modifier.padding(vertical = 4.dp))
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Column(Modifier.weight(1f)) {
                        Text(stringResource(R.string.settings_speakers), style = MaterialTheme.typography.titleSmall)
                        Text(
                            stringResource(R.string.settings_speakers_desc),
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                    Switch(
                        checked = setup.diarize,
                        onCheckedChange = { on ->
                            // Igual ao plano: volta ao automático.
                            LocalTranscribe.setDiarize(context, if (on == setup.plan.diarize) null else on)
                            reload()
                        },
                        modifier = Modifier.testTag("falantes-no-celular"),
                    )
                }

                HorizontalDivider(Modifier.padding(vertical = 4.dp))
                Text(
                    stringResource(R.string.settings_models_on_device, megabytes(bytes)),
                    style = MaterialTheme.typography.bodyMedium,
                )
                FlowRow(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                    TextButton(
                        onClick = { LocalTranscribe.downloadNow(context) },
                        modifier = Modifier.testTag("baixar-modelos"),
                    ) { Text(stringResource(R.string.settings_download_now)) }
                    TextButton(
                        onClick = { LocalTranscribe.deleteModels(context); reload() },
                        enabled = bytes > 0,
                    ) { Text(stringResource(R.string.settings_delete_models), color = MaterialTheme.colorScheme.error) }
                }
            }
        }
    }
}

@Composable
private fun ModeOption(mode: TranscribeMode, current: TranscribeMode, title: Int, detail: Int, onPick: (TranscribeMode) -> Unit) {
    Choice(
        selected = mode == current,
        title = stringResource(title),
        detail = stringResource(detail),
        tag = "modo-${mode.key}",
    ) { onPick(mode) }
}

@Composable
private fun Choice(selected: Boolean, title: String, detail: String?, tag: String, onClick: () -> Unit) {
    Row(
        verticalAlignment = Alignment.CenterVertically,
        modifier = Modifier
            .fillMaxWidth()
            .selectable(selected = selected, onClick = onClick, role = Role.RadioButton)
            .testTag(tag),
    ) {
        RadioButton(selected = selected, onClick = null)
        Column(Modifier.padding(start = 8.dp).clickable(onClick = onClick)) {
            Text(title, style = MaterialTheme.typography.bodyLarge)
            if (detail != null) {
                Text(detail, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }
    }
}
