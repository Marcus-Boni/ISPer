package com.isper.mobile.library

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.testTagsAsResourceId
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import com.google.mlkit.vision.barcode.common.Barcode
import com.google.mlkit.vision.codescanner.GmsBarcodeScannerOptions
import com.google.mlkit.vision.codescanner.GmsBarcodeScanning
import com.isper.mobile.R
import com.isper.mobile.core.PcInfo
import com.isper.mobile.sync.PcSync

/**
 * Um diálogo abre numa janela própria: as etiquetas de teste (testTag) só
 * viram resource-id para o uiautomator se isso for ligado nele também (o do
 * Scaffold não chega aqui).
 */
@OptIn(ExperimentalComposeUiApi::class)
private val dialogTags = Modifier.semantics { testTagsAsResourceId = true }

/** O pareamento em andamento, como a tela o mostra. */
sealed interface PairUi {
    data object Idle : PairUi
    /** O código foi lido: "Parear com …?". */
    data class Confirm(val code: String, val pcName: String) : PairUi
    /** Esperando alguém tocar em "Permitir" no PC. */
    data class Waiting(val pcName: String) : PairUi
}

/**
 * O topo da Biblioteca: sem PC, o convite para parear; com PC, o estado da
 * sincronia e "Enviar agora".
 */
@Composable
fun PcCard(
    pc: PcInfo?,
    sending: PcSync.Sending?,
    waiting: Int,
    unsent: Int,
    lastError: String?,
    actions: LibraryViewModel,
) {
    val context = LocalContext.current
    var pasting by remember { mutableStateOf(false) }
    var confirmUnpair by remember { mutableStateOf(false) }

    Card(
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceVariant),
        modifier = Modifier.fillMaxWidth(),
    ) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            if (pc == null) {
                Text(stringResource(R.string.sync_card_title), style = MaterialTheme.typography.titleMedium)
                Text(
                    stringResource(R.string.sync_card_pitch),
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Button(
                        onClick = {
                            scanQr(context, onCode = actions::startPair, onError = actions::notice)
                        },
                        modifier = Modifier.testTag("ler-qr"),
                    ) { Text(stringResource(R.string.sync_scan)) }
                    OutlinedButton(
                        onClick = { pasting = true },
                        modifier = Modifier.testTag("colar-codigo"),
                    ) { Text(stringResource(R.string.sync_paste)) }
                }
            } else {
                Text(
                    stringResource(R.string.sync_connected, pc.name),
                    style = MaterialTheme.typography.titleMedium,
                    modifier = Modifier.testTag("pc-conectado"),
                )
                val line = when {
                    sending != null -> stringResource(R.string.sync_sending, (sending.fraction * 100).toInt())
                    lastError != null && unsent > 0 -> stringResource(R.string.sync_unreachable, unsent)
                    waiting > 0 -> plural(R.plurals.sync_waiting, waiting)
                    unsent > 0 -> plural(R.plurals.sync_unsent, unsent)
                    else -> stringResource(R.string.sync_all_sent)
                }
                Text(
                    line,
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.testTag("pc-status"),
                )
                if (sending != null) {
                    LinearProgressIndicator(progress = { sending.fraction }, modifier = Modifier.fillMaxWidth())
                }
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Button(onClick = actions::sendNow, modifier = Modifier.testTag("enviar-agora")) {
                        Text(stringResource(R.string.sync_send_now))
                    }
                    TextButton(onClick = { confirmUnpair = true }, modifier = Modifier.testTag("desconectar")) {
                        Text(stringResource(R.string.sync_unpair))
                    }
                }
            }
        }
    }

    if (pasting) {
        var text by remember { mutableStateOf("") }
        AlertDialog(
            modifier = dialogTags,
            onDismissRequest = { pasting = false },
            title = { Text(stringResource(R.string.sync_paste_title)) },
            text = {
                Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    Text(stringResource(R.string.sync_paste_hint), style = MaterialTheme.typography.bodyMedium)
                    OutlinedTextField(
                        value = text,
                        onValueChange = { text = it },
                        placeholder = { Text("isper://parear?…") },
                        singleLine = false,
                        maxLines = 4,
                        // Um código não se corrige: sem autocorreção nem sugestões.
                        keyboardOptions = KeyboardOptions(
                            keyboardType = KeyboardType.Uri,
                            autoCorrectEnabled = false,
                            imeAction = ImeAction.Done,
                        ),
                        modifier = Modifier.fillMaxWidth().testTag("codigo"),
                    )
                }
            },
            confirmButton = {
                TextButton(
                    onClick = { pasting = false; actions.startPair(text) },
                    enabled = text.isNotBlank(),
                    modifier = Modifier.testTag("usar-codigo"),
                ) { Text(stringResource(R.string.sync_use_code)) }
            },
            dismissButton = { TextButton(onClick = { pasting = false }) { Text(stringResource(R.string.cancel)) } },
        )
    }

    if (confirmUnpair && pc != null) {
        AlertDialog(
            modifier = dialogTags,
            onDismissRequest = { confirmUnpair = false },
            title = { Text(stringResource(R.string.sync_unpair_title, pc.name)) },
            text = { Text(stringResource(R.string.sync_unpair_text)) },
            confirmButton = {
                TextButton(
                    onClick = { confirmUnpair = false; actions.unpair() },
                    modifier = Modifier.testTag("confirmar-desconectar"),
                ) {
                    Text(stringResource(R.string.sync_unpair), color = MaterialTheme.colorScheme.error)
                }
            },
            dismissButton = { TextButton(onClick = { confirmUnpair = false }) { Text(stringResource(R.string.cancel)) } },
        )
    }
}

/** A pergunta "Parear com …?" e a espera pelo "Permitir" do PC. */
@Composable
fun PairDialogs(pair: PairUi, actions: LibraryViewModel) {
    when (pair) {
        PairUi.Idle -> {}
        is PairUi.Confirm -> AlertDialog(
            modifier = dialogTags,
            onDismissRequest = actions::cancelPair,
            title = { Text(stringResource(R.string.sync_confirm_title, pair.pcName)) },
            text = { Text(stringResource(R.string.sync_confirm_text)) },
            confirmButton = {
                TextButton(onClick = actions::confirmPair, modifier = Modifier.testTag("confirmar-pareamento")) {
                    Text(stringResource(R.string.sync_pair))
                }
            },
            dismissButton = { TextButton(onClick = actions::cancelPair) { Text(stringResource(R.string.cancel)) } },
        )
        is PairUi.Waiting -> AlertDialog(
            modifier = dialogTags,
            onDismissRequest = {},
            title = { Text(stringResource(R.string.sync_waiting_title)) },
            text = {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(16.dp)) {
                    CircularProgressIndicator(Modifier.size(28.dp))
                    Text(stringResource(R.string.sync_waiting_text, pair.pcName))
                }
            },
            confirmButton = {},
        )
    }
}

@Composable
private fun plural(id: Int, count: Int): String = pluralStringResource(id, count, count)

/**
 * O leitor de QR do Google (Play services): lê no aparelho, sem pedir a
 * permissão da câmera ao app. Sem ele, sobra "Colar o código".
 */
private fun scanQr(context: android.content.Context, onCode: (String) -> Unit, onError: (String) -> Unit) {
    val options = GmsBarcodeScannerOptions.Builder()
        .setBarcodeFormats(Barcode.FORMAT_QR_CODE)
        .build()
    GmsBarcodeScanning.getClient(context, options)
        .startScan()
        .addOnSuccessListener { barcode -> barcode.rawValue?.let(onCode) }
        .addOnFailureListener { e ->
            onError(context.getString(R.string.sync_scan_failed, e.localizedMessage ?: e.javaClass.simpleName))
        }
}
