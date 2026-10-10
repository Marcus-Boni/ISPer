package com.isper.mobile.today

import android.annotation.SuppressLint
import android.media.AudioFormat
import android.media.AudioRecord
import android.media.MediaRecorder
import java.io.ByteArrayOutputStream
import kotlin.concurrent.thread

/**
 * O microfone para ditar uma tarefa (Fase 10.6): PCM 16 kHz mono, 16 bits,
 * na memória — uma frase curta não vira arquivo. Quem transcreve é o núcleo,
 * no próprio aparelho (`transcribeDictation`); o áudio não sai do celular.
 *
 * Para sozinho em [MAX_SECS]. A permissão do microfone é pedida pela tela
 * antes de [start].
 */
class Dictation(private val onLevel: (Float) -> Unit) {
    @Volatile private var running = false
    private var worker: Thread? = null
    private val pcm = ByteArrayOutputStream()

    val isRunning: Boolean get() = running

    /** Segundos gravados até agora. */
    val seconds: Float get() = pcm.size() / (RATE * 2f)

    @SuppressLint("MissingPermission")
    fun start() {
        if (running) return
        val min = AudioRecord.getMinBufferSize(RATE, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT)
        val audio = AudioRecord(
            MediaRecorder.AudioSource.VOICE_RECOGNITION,
            RATE,
            AudioFormat.CHANNEL_IN_MONO,
            AudioFormat.ENCODING_PCM_16BIT,
            maxOf(min, RATE / 5 * 2),
        )
        pcm.reset()
        running = true
        audio.startRecording()
        worker = thread(name = "isper-ditado") {
            val chunk = ByteArray(RATE / 10 * 2) // 100 ms
            try {
                while (running && seconds < MAX_SECS) {
                    val n = audio.read(chunk, 0, chunk.size)
                    if (n <= 0) continue
                    synchronized(pcm) { pcm.write(chunk, 0, n) }
                    onLevel(level(chunk, n))
                }
            } finally {
                running = false
                runCatching { audio.stop() }
                audio.release()
            }
        }
    }

    /** Para e devolve o áudio gravado. */
    fun stop(): ByteArray {
        running = false
        worker?.join(1_000)
        worker = null
        return synchronized(pcm) { pcm.toByteArray() }
    }

    private fun level(chunk: ByteArray, n: Int): Float {
        var peak = 0
        var i = 0
        while (i + 1 < n) {
            val s = (chunk[i].toInt() and 0xFF) or (chunk[i + 1].toInt() shl 8)
            peak = maxOf(peak, kotlin.math.abs(s.toShort().toInt()))
            i += 2
        }
        return (peak / 32_768f).coerceIn(0f, 1f)
    }

    companion object {
        const val RATE = 16_000
        const val MAX_SECS = 60f
    }
}
