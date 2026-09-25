package com.isper.mobile

import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Icon
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarDuration
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.SnackbarResult
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.testTagsAsResourceId
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.isper.mobile.library.LibraryScreen
import com.isper.mobile.library.LibraryViewModel
import com.isper.mobile.library.MinutesScreen
import com.isper.mobile.recording.RecordScreen
import com.isper.mobile.recording.RecorderBus

/** Os três destinos do app, na ordem da barra de baixo. */
private data class Destination(val label: Int, val icon: Int)

private val destinations = listOf(
    Destination(R.string.tab_record, R.drawable.ic_mic),
    Destination(R.string.tab_library, R.drawable.ic_library),
    Destination(R.string.tab_lab, R.drawable.ic_lab),
)

@OptIn(ExperimentalComposeUiApi::class)
@Composable
fun IsperApp(
    tab: Int,
    onTab: (Int) -> Unit,
    library: LibraryViewModel,
    lab: SpikeViewModel,
    minutesId: String?,
    onMinutes: (String?) -> Unit,
) {
    val snackbars = remember { SnackbarHostState() }
    val undoLabel = stringResource(R.string.undo)
    LaunchedEffect(Unit) {
        library.notices.collect { notice ->
            val result = snackbars.showSnackbar(
                message = notice.text,
                actionLabel = notice.undoId?.let { undoLabel },
                duration = if (notice.undoId != null) SnackbarDuration.Long else SnackbarDuration.Short,
            )
            if (result == SnackbarResult.ActionPerformed) notice.undoId?.let(library::undoDelete)
        }
    }

    Scaffold(
        // As etiquetas de teste (testTag) viram resource-id para o uiautomator
        // do e2e (tools/e2e/android-recorder.ps1).
        modifier = Modifier.semantics { testTagsAsResourceId = true },
        snackbarHost = { SnackbarHost(snackbars) },
        bottomBar = {
            NavigationBar {
                destinations.forEachIndexed { index, d ->
                    NavigationBarItem(
                        selected = tab == index,
                        onClick = { onTab(index) },
                        icon = { Icon(painterResource(d.icon), contentDescription = null) },
                        label = { Text(stringResource(d.label)) },
                    )
                }
            }
        },
    ) { padding ->
        val modifier = Modifier.padding(padding)
        when (tab) {
            MainActivity.TAB_RECORD -> {
                val rec by RecorderBus.state.collectAsStateWithLifecycle()
                RecordScreen(state = rec, modifier = modifier)
            }
            MainActivity.TAB_LIBRARY -> {
                val state by library.state.collectAsStateWithLifecycle()
                // A ata aberta (pela Biblioteca ou pela notificação "Ata pronta").
                val open = minutesId?.let { id -> state.recordings.firstOrNull { it.id == id && it.minutesPath != null } }
                if (open != null) {
                    MinutesScreen(info = open, onBack = { onMinutes(null) }, modifier = modifier)
                } else {
                    LibraryScreen(
                        state = state,
                        actions = library,
                        onMeasure = { info ->
                            lab.useRecording(info.audioPath, info.sourceName ?: info.id)
                            onTab(MainActivity.TAB_LAB)
                        },
                        onOpenMinutes = { info -> onMinutes(info.id) },
                        modifier = modifier,
                    )
                }
            }
            else -> {
                val state by lab.state.collectAsStateWithLifecycle()
                SpikeScreen(state = state, actions = lab, modifier = modifier)
            }
        }
    }
}
