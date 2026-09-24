package com.isper.mobile.recording

import com.isper.mobile.core.RecordingInfo
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow

/** O que a tela Gravar, o bloco rápido e a notificação mostram. */
sealed interface RecState {
    data object Idle : RecState

    data class Active(
        val id: String,
        val elapsedSecs: Double,
        val paused: Boolean,
        val silenced: Boolean,
        val moments: List<Double>,
        /** Nível do microfone, de 0 a 1, dos últimos segundos (mais novo no fim). */
        val levels: List<Float>,
        val sampleRate: Int,
    ) : RecState
}

sealed interface RecEvent {
    data class Saved(val info: RecordingInfo) : RecEvent
    data class Failed(val message: String) : RecEvent
}

/**
 * Ponte entre o [RecordingService] e quem mostra o estado. O serviço é o
 * único que escreve; a interface só lê.
 */
object RecorderBus {
    private val _state = MutableStateFlow<RecState>(RecState.Idle)
    val state: StateFlow<RecState> = _state.asStateFlow()

    private val _events = MutableSharedFlow<RecEvent>(extraBufferCapacity = 8)
    val events: SharedFlow<RecEvent> = _events.asSharedFlow()

    internal fun update(state: RecState) {
        _state.value = state
    }

    internal fun emit(event: RecEvent) {
        _events.tryEmit(event)
    }

    /**
     * A gravação que o serviço está abrindo: o manifesto já existe, mas o
     * primeiro [RecState.Active] ainda não saiu. Sem isso, uma Biblioteca que
     * listasse nesse instante a trataria como caída.
     */
    @Volatile internal var opening: String? = null

    /** O id da gravação em andamento (a recuperação não mexe nela). */
    val activeId: String? get() = (state.value as? RecState.Active)?.id ?: opening
}
