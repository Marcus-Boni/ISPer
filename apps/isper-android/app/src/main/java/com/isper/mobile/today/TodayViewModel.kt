package com.isper.mobile.today

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import com.isper.mobile.core.MobileDay
import com.isper.mobile.core.MobileException
import com.isper.mobile.core.ParsedTask
import com.isper.mobile.core.TaskBook
import com.isper.mobile.core.transcribeDictation
import com.isper.mobile.sync.PcSync
import com.isper.mobile.transcribe.LocalTranscribe
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/** O ditado, como a tela mostra. */
sealed interface DictationUi {
    data object Idle : DictationUi
    data class Listening(val seconds: Float, val level: Float) : DictationUi
    data object Understanding : DictationUi
}

/**
 * A tela Hoje do celular (Fase 10.6): o dia (o retrato do PC com a fila
 * daqui por cima), criar digitando ou ditando, concluir, adiar e aceitar da
 * caixa de entrada. Cada mudança vai para a fila do núcleo na hora; a
 * sincronia leva ao PC quando ele estiver por perto.
 */
class TodayViewModel(app: Application) : AndroidViewModel(app) {
    private val ctx get() = getApplication<Application>()

    private val _day = MutableStateFlow<MobileDay?>(null)
    val day: StateFlow<MobileDay?> = _day.asStateFlow()

    private val _notices = MutableSharedFlow<String>(extraBufferCapacity = 4)
    /** Avisos curtos para a tela (o que deu errado). */
    val notices: SharedFlow<String> = _notices.asSharedFlow()

    private val _dictation = MutableStateFlow<DictationUi>(DictationUi.Idle)
    val dictation: StateFlow<DictationUi> = _dictation.asStateFlow()

    /** O texto que o ditado entendeu, para a tela pôr no campo. */
    private val _heard = MutableSharedFlow<String>(extraBufferCapacity = 1)
    val heard: SharedFlow<String> = _heard.asSharedFlow()

    private var recorder: Dictation? = null
    private var ticker: Job? = null
    @Volatile private var level = 0f

    init {
        refresh()
        viewModelScope.launch { Tasks.changed.collect { refresh() } }
        viewModelScope.launch { PcSync.changed.collect { refresh() } }
    }

    fun refresh() {
        viewModelScope.launch(Dispatchers.IO) {
            runCatching { Tasks.day(ctx) }
                .onSuccess { _day.value = it }
                .onFailure { _notices.tryEmit(it.message ?: "erro") }
        }
    }

    /** O que a frase vira (título, dia, hora), para a prévia. */
    fun parse(text: String): ParsedTask? =
        text.takeIf { it.isNotBlank() }?.let { runCatching { Tasks.book(ctx).parse(it, Tasks.today()) }.getOrNull() }

    private fun act(block: TaskBook.() -> Unit) {
        viewModelScope.launch(Dispatchers.IO) {
            try {
                Tasks.book(ctx).block()
                Tasks.changedHere(ctx)
            } catch (e: MobileException) {
                _notices.tryEmit(e.message ?: "erro")
            }
        }
    }

    /** Cria a tarefa da frase (as datas saem do texto, como no PC). */
    fun add(text: String, voice: Boolean) = act {
        val p = parse(text, Tasks.today())
        add(p.title, p.plannedOn, p.plannedTime, p.dueOn, voice, Tasks.today(), Tasks.now())
    }

    fun complete(id: String) = act { setStatus(id, "done", Tasks.now()) }
    fun reopen(id: String) = act { setStatus(id, "open", Tasks.now()) }
    fun drop(id: String) = act { setStatus(id, "dropped", Tasks.now()) }
    fun moveTo(id: String, day: String?) = act { setDay(id, day, Tasks.now()) }
    fun rename(id: String, title: String) = act { rename(id, title, Tasks.now()) }

    /** O modelo do Whisper está no aparelho? Sem ele, não dá para ditar (baixa nos Ajustes). */
    fun canDictate(): Boolean =
        LocalTranscribe.modelFile(ctx, LocalTranscribe.setup(ctx).modelFile).isFile

    fun startDictation() {
        if (recorder?.isRunning == true) return
        val r = Dictation { level = it }
        try {
            r.start()
        } catch (e: Exception) {
            _notices.tryEmit(e.message ?: "microfone indisponível")
            return
        }
        recorder = r
        ticker = viewModelScope.launch {
            while (r.isRunning) {
                _dictation.value = DictationUi.Listening(r.seconds, level)
                delay(100)
            }
            // Parou sozinho no limite: transcreve o que ouviu.
            if (recorder === r) stopDictation()
        }
    }

    fun stopDictation() {
        val r = recorder ?: return
        recorder = null
        ticker?.cancel()
        val pcm = r.stop()
        _dictation.value = DictationUi.Understanding
        viewModelScope.launch {
            try {
                val setup = LocalTranscribe.setup(ctx)
                val model = LocalTranscribe.modelFile(ctx, setup.modelFile).path
                val text = withContext(Dispatchers.Default) { transcribeDictation(pcm, model, "pt") }
                _heard.tryEmit(text)
            } catch (e: MobileException) {
                _notices.tryEmit(e.message ?: "não entendi")
            } finally {
                _dictation.value = DictationUi.Idle
            }
        }
    }

    override fun onCleared() {
        recorder?.stop()
        super.onCleared()
    }
}
