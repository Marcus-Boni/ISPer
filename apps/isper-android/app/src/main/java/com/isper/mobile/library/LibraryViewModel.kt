package com.isper.mobile.library

import android.app.Application
import android.media.MediaPlayer
import android.net.Uri
import android.provider.OpenableColumns
import android.util.Log
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import com.isper.mobile.R
import com.isper.mobile.core.MobileException
import com.isper.mobile.core.RecordingInfo
import com.isper.mobile.core.deleteRecording
import com.isper.mobile.core.importRecording
import com.isper.mobile.core.listRecordings
import com.isper.mobile.core.retryRecording
import com.isper.mobile.recording.RecEvent
import com.isper.mobile.recording.RecorderBus
import com.isper.mobile.recording.Storage
import com.isper.mobile.recording.formatDuration
import com.isper.mobile.sync.PcSync
import java.io.File
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

data class LibraryState(
    val recordings: List<RecordingInfo> = emptyList(),
    val loading: Boolean = true,
    val playingId: String? = null,
    val playingPosition: Int = 0,
    /** Apagadas há pouco e ainda desfazíveis: somem da lista na hora. */
    val pendingDelete: Set<String> = emptySet(),
    /** O pareamento com o PC em andamento (Fase 9.3). */
    val pair: PairUi = PairUi.Idle,
)

/** Um aviso de rodapé, com "Desfazer" quando é uma exclusão. */
data class LibraryNotice(val text: String, val undoId: String? = null)

/**
 * A Biblioteca do celular. A lista sai da pasta de gravações (o par
 * `.opus` + manifesto), e montá-la fecha as que uma queda deixou abertas.
 */
class LibraryViewModel(app: Application) : AndroidViewModel(app) {
    private val context = app.applicationContext
    private val dir = Storage.recordingsDir(context)

    private val _state = MutableStateFlow(LibraryState())
    val state: StateFlow<LibraryState> = _state.asStateFlow()

    private val _notices = MutableSharedFlow<LibraryNotice>(extraBufferCapacity = 4)
    val notices: SharedFlow<LibraryNotice> = _notices.asSharedFlow()

    private var player: MediaPlayer? = null
    private var ticker: Job? = null
    private val deleteJobs = mutableMapOf<String, Job>()

    /** O PC pareado, o envio em andamento e o último erro (Fase 9.3). */
    val pc = PcSync.pc
    val sending = PcSync.sending
    val lastError = PcSync.lastError

    init {
        PcSync.link(context)
        refresh()
        // Uma rodada de sincronia terminou: os estados "no PC" mudaram.
        viewModelScope.launch { PcSync.changed.collect { refresh() } }
        viewModelScope.launch {
            RecorderBus.events.collect { event ->
                when (event) {
                    is RecEvent.Saved -> {
                        refresh()
                        notice(context.getString(R.string.lib_saved, formatDuration(event.info.durationSecs ?: 0.0)))
                    }
                    is RecEvent.Failed -> notice(event.message)
                }
            }
        }
    }

    fun refresh() {
        viewModelScope.launch {
            val result = withContext(Dispatchers.IO) {
                runCatching { listRecordings(dir.path, RecorderBus.activeId) }
            }
            result.onSuccess { list ->
                _state.update { it.copy(recordings = list.recordings, loading = false) }
                list.recovered.forEach { r ->
                    notice(context.getString(R.string.lib_recovered, formatDuration(r.durationSecs ?: 0.0)))
                }
            }.onFailure { e ->
                Log.e(TAG, "não consegui listar as gravações", e)
                _state.update { it.copy(loading = false) }
                notice(e.message ?: e.javaClass.simpleName)
            }
        }
    }

    /** Um áudio que outro app compartilhou com o ISPer. */
    fun importShared(uri: Uri) {
        viewModelScope.launch {
            val result = withContext(Dispatchers.IO) {
                runCatching {
                    val name = displayName(uri) ?: "audio"
                    val temp = File(context.cacheDir, "importando").apply { mkdirs() }
                        .resolve(name.replace(Regex("[^\\w.\\- ]"), "_"))
                    context.contentResolver.openInputStream(uri)?.use { input ->
                        temp.outputStream().use { input.copyTo(it) }
                    } ?: error(context.getString(R.string.lib_import_unreadable, name))
                    try {
                        val (id, startedAt) = Storage.newId()
                        importRecording(dir.path, temp.path, name, id, startedAt)
                    } finally {
                        temp.delete()
                    }
                }
            }
            result.onSuccess { info ->
                refresh()
                notice(context.getString(R.string.lib_imported, info.sourceName ?: info.id))
                PcSync.syncSoon(context)
            }.onFailure { e -> notice(e.message ?: e.javaClass.simpleName) }
        }
    }

    private fun displayName(uri: Uri): String? =
        context.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)
            ?.use { c -> if (c.moveToFirst()) c.getString(0) else null }

    fun togglePlay(info: RecordingInfo) {
        if (_state.value.playingId == info.id) {
            stopPlayer()
            return
        }
        stopPlayer()
        try {
            player = MediaPlayer().apply {
                setDataSource(info.audioPath)
                setOnCompletionListener { stopPlayer() }
                prepare()
                start()
            }
            _state.update { it.copy(playingId = info.id, playingPosition = 0) }
            ticker = viewModelScope.launch {
                while (true) {
                    delay(250)
                    val p = player ?: break
                    _state.update { it.copy(playingPosition = runCatching { p.currentPosition }.getOrDefault(0)) }
                }
            }
        } catch (e: Exception) {
            Log.e(TAG, "não consegui tocar ${info.audioPath}", e)
            stopPlayer()
            notice(context.getString(R.string.lib_play_failed))
        }
    }

    private fun stopPlayer() {
        ticker?.cancel()
        ticker = null
        player?.run {
            runCatching { stop() }
            release()
        }
        player = null
        _state.update { it.copy(playingId = null, playingPosition = 0) }
    }

    /**
     * Some da lista na hora e só é apagada de verdade depois de alguns
     * segundos — tempo de tocar em "Desfazer" (ADR 0009).
     */
    fun delete(info: RecordingInfo) {
        if (_state.value.playingId == info.id) stopPlayer()
        _state.update { it.copy(pendingDelete = it.pendingDelete + info.id) }
        deleteJobs[info.id] = viewModelScope.launch {
            delay(UNDO_WINDOW_MS)
            withContext(Dispatchers.IO) { runCatching { deleteRecording(dir.path, info.id) } }
                .onFailure { e -> notice(e.message ?: e.javaClass.simpleName) }
            deleteJobs.remove(info.id)
            _state.update { it.copy(pendingDelete = it.pendingDelete - info.id) }
            refresh()
        }
        notice(context.getString(R.string.lib_deleted), undoId = info.id)
    }

    fun undoDelete(id: String) {
        deleteJobs.remove(id)?.cancel()
        _state.update { it.copy(pendingDelete = it.pendingDelete - id) }
    }

    fun notice(text: String) = notice(text, null)

    private fun notice(text: String, undoId: String?) {
        _notices.tryEmit(LibraryNotice(text, undoId))
    }

    // ------------------------------------------------ PC (Fase 9.3)

    /** Um código lido (QR) ou colado: pergunta "Parear com …?". */
    fun startPair(code: String) {
        viewModelScope.launch {
            runCatching { PcSync.readCode(context, code.trim()) }
                .onSuccess { name -> _state.update { it.copy(pair = PairUi.Confirm(code.trim(), name)) } }
                .onFailure { e -> notice(e.message ?: e.javaClass.simpleName) }
        }
    }

    fun cancelPair() = _state.update { it.copy(pair = PairUi.Idle) }

    /** "Parear": espera o "Permitir" no PC. */
    fun confirmPair() {
        val confirm = _state.value.pair as? PairUi.Confirm ?: return
        _state.update { it.copy(pair = PairUi.Waiting(confirm.pcName)) }
        viewModelScope.launch {
            runCatching { PcSync.pair(context, confirm.code) }
                .onSuccess { pc -> notice(context.getString(R.string.sync_paired, pc.name)) }
                .onFailure { e -> notice(pairError(e)) }
            _state.update { it.copy(pair = PairUi.Idle) }
        }
    }

    private fun pairError(e: Throwable): String = when (e) {
        is MobileException.Sync -> context.getString(R.string.sync_pair_failed, e.message ?: "")
        else -> e.message ?: e.javaClass.simpleName
    }

    fun unpair() {
        viewModelScope.launch {
            val name = pc.value?.name.orEmpty()
            runCatching { PcSync.unpair(context) }
            notice(context.getString(R.string.sync_unpaired, name))
            refresh()
        }
    }

    fun sendNow() {
        PcSync.syncSoon(context)
        notice(context.getString(R.string.sync_sending_now))
    }

    /** A gravação que falhou volta para a fila do envio. */
    fun retry(info: RecordingInfo) {
        retryRecording(dir.path, info.id)
        refresh()
        PcSync.syncSoon(context)
    }

    override fun onCleared() {
        stopPlayer()
        // Exclusões ainda pendentes valem: quem apagou não desfez.
        deleteJobs.keys.toList().forEach { id -> runCatching { deleteRecording(dir.path, id) } }
        super.onCleared()
    }

    companion object {
        private const val TAG = "ISPerBiblioteca"
        const val UNDO_WINDOW_MS = 5_000L
    }
}
