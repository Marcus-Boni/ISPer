package com.isper.mobile.recording

import android.Manifest
import android.annotation.SuppressLint
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.content.pm.ServiceInfo
import android.media.AudioFormat
import android.media.AudioRecord
import android.media.AudioRecordingConfiguration
import android.media.AudioManager
import android.media.MediaRecorder
import android.os.IBinder
import android.os.PowerManager
import android.service.quicksettings.TileService
import android.util.Log
import androidx.core.app.NotificationCompat
import androidx.core.app.ServiceCompat
import androidx.core.content.ContextCompat
import com.isper.mobile.MainActivity
import com.isper.mobile.R
import com.isper.mobile.core.Recorder
import java.util.concurrent.Executors
import kotlin.math.sqrt

/**
 * O gravador (Fase 9.2): um serviço em primeiro plano do tipo microfone, que
 * continua gravando com a tela apagada e o app fechado.
 *
 * O áudio vai do `AudioRecord` direto para o núcleo Rust ([Recorder]), que
 * escreve o Ogg/Opus em páginas de 1 s e o manifesto ao lado. Se o processo
 * morrer, o que já foi escrito fica; a próxima abertura do app recupera a
 * gravação (ver `list_recordings`).
 *
 * Três situações que um gravador de verdade precisa aguentar:
 * - **ligação**: o Android dá o microfone à chamada e passa a entregar
 *   silêncio; o [AudioRecordingConfiguration.isClientSilenced] avisa, e o
 *   trecho fica marcado no manifesto, sem parar a gravação;
 * - **microfone que cai** (o servidor de áudio reinicia): o `read` devolve
 *   erro; o microfone é reaberto e a reabertura fica marcada;
 * - **tela apagada**: um *wake lock* parcial mantém a CPU acordada enquanto
 *   houver gravação.
 */
class RecordingService : Service() {

    private var recorder: Recorder? = null
    private var record: AudioRecord? = null
    private var worker: Thread? = null
    private var wakeLock: PowerManager.WakeLock? = null
    private val callbacks = Executors.newSingleThreadExecutor()

    @Volatile private var running = false
    @Volatile private var paused = false
    @Volatile private var silenced = false
    @Volatile private var bytesWritten = 0L
    private var sampleRate = 48_000
    private var id = ""
    private val moments = mutableListOf<Double>()
    private val levels = ArrayDeque<Float>()

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_START -> start()
            ACTION_STOP -> stop()
            ACTION_MARK -> mark()
            ACTION_PAUSE -> setPaused(true)
            ACTION_RESUME -> setPaused(false)
        }
        // Se o sistema matar o serviço, não recomeçar sozinho: uma gravação
        // nova sem ninguém saber seria pior; a antiga é recuperada ao abrir.
        return START_NOT_STICKY
    }

    private fun start() {
        if (running) return
        if (ContextCompat.checkSelfPermission(this, Manifest.permission.RECORD_AUDIO) !=
            PackageManager.PERMISSION_GRANTED
        ) {
            RecorderBus.emit(RecEvent.Failed(getString(R.string.rec_no_permission)))
            stopSelf()
            return
        }
        ensureChannels()
        // O startForeground precisa vir logo (o sistema dá 5 s).
        ServiceCompat.startForeground(
            this, NOTIFICATION_ID, notification(), ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE,
        )
        try {
            val (newId, startedAt) = Storage.newId()
            id = newId
            RecorderBus.opening = id
            val audio = openMic()
            record = audio
            sampleRate = audio.sampleRate
            recorder = Recorder.start(
                Storage.recordingsDir(this).path, id, startedAt, sampleRate.toUInt(),
            )
            moments.clear()
            levels.clear()
            bytesWritten = 0
            paused = false
            silenced = false
            wakeLock = getSystemService(PowerManager::class.java)
                .newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "isper:gravacao")
                .apply { acquire(MAX_RECORDING_MS) }
            audio.registerAudioRecordingCallback(callbacks, silenceWatcher)
            audio.startRecording()
            running = true
            worker = Thread(::loop, "isper-gravador").apply { start() }
            publish()
            refreshTile()
            Log.i(TAG, "gravando $id a $sampleRate Hz")
        } catch (e: Exception) {
            Log.e(TAG, "não consegui começar a gravar", e)
            RecorderBus.emit(RecEvent.Failed(e.message ?: e.javaClass.simpleName))
            cleanup()
        } finally {
            RecorderBus.opening = null
        }
    }

    /** 48 kHz (o nativo de quase todo celular) ou, se não der, 16 kHz. */
    @SuppressLint("MissingPermission") // conferida em start()
    private fun openMic(): AudioRecord {
        for (rate in intArrayOf(48_000, 16_000)) {
            val min = AudioRecord.getMinBufferSize(
                rate, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT,
            )
            if (min <= 0) continue
            val audio = AudioRecord(
                MediaRecorder.AudioSource.MIC,
                rate,
                AudioFormat.CHANNEL_IN_MONO,
                AudioFormat.ENCODING_PCM_16BIT,
                maxOf(min, rate / 2 * 2), // meio segundo de folga
            )
            if (audio.state == AudioRecord.STATE_INITIALIZED) return audio
            audio.release()
        }
        error(getString(R.string.rec_mic_unavailable))
    }

    private fun loop() {
        val chunk = ByteArray(sampleRate / 10 * 2) // 100 ms de PCM 16 bits
        while (running) {
            val audio = record ?: break
            val n = audio.read(chunk, 0, chunk.size)
            if (n > 0) {
                if (!paused) {
                    try {
                        recorder?.write(chunk.copyOf(n))
                        bytesWritten += n
                    } catch (e: Exception) {
                        Log.e(TAG, "falha ao gravar o áudio", e)
                    }
                }
                pushLevel(chunk, n)
            } else if (running) {
                Log.w(TAG, "o microfone parou ($n); reabrindo")
                reopenMic()
            }
        }
    }

    private fun reopenMic() {
        try {
            record?.let {
                it.unregisterAudioRecordingCallback(silenceWatcher)
                it.release()
            }
            Thread.sleep(200)
            val audio = openMic()
            if (audio.sampleRate != sampleRate) {
                audio.release()
                error("o microfone voltou com outra taxa (${audio.sampleRate} Hz)")
            }
            audio.registerAudioRecordingCallback(callbacks, silenceWatcher)
            audio.startRecording()
            record = audio
            recorder?.markReopened()
        } catch (e: Exception) {
            Log.e(TAG, "não consegui reabrir o microfone", e)
            RecorderBus.emit(RecEvent.Failed(getString(R.string.rec_mic_lost)))
            stop()
        }
    }

    private val silenceWatcher = object : AudioManager.AudioRecordingCallback() {
        override fun onRecordingConfigChanged(configs: MutableList<AudioRecordingConfiguration>) {
            val mine = record?.activeRecordingConfiguration ?: return
            val now = mine.isClientSilenced
            if (now != silenced) {
                silenced = now
                runCatching { recorder?.setSilenced(now) }
                publish()
                notifyUpdate()
            }
        }
    }

    private fun pushLevel(chunk: ByteArray, n: Int) {
        var sum = 0.0
        var i = 0
        while (i + 1 < n) {
            val s = ((chunk[i + 1].toInt() shl 8) or (chunk[i].toInt() and 0xff)).toShort() / 32768.0
            sum += s * s
            i += 2
        }
        val rms = sqrt(sum / maxOf(1, n / 2)).toFloat()
        synchronized(levels) {
            levels.addLast((rms * 4f).coerceIn(0f, 1f))
            while (levels.size > LEVELS_KEPT) levels.removeFirst()
        }
        publish()
    }

    private fun elapsedSecs() = bytesWritten / 2.0 / sampleRate

    private fun publish() {
        if (!running) return
        val snapshot = synchronized(levels) { levels.toList() }
        RecorderBus.update(
            RecState.Active(
                id = id,
                elapsedSecs = elapsedSecs(),
                paused = paused,
                silenced = silenced,
                moments = moments.toList(),
                levels = snapshot,
                sampleRate = sampleRate,
            ),
        )
    }

    private fun mark() {
        val r = recorder ?: return
        runCatching { r.markMoment() }.onSuccess { at ->
            moments += at
            publish()
            notifyUpdate()
        }
    }

    private fun setPaused(value: Boolean) {
        if (!running || paused == value) return
        paused = value
        if (value) runCatching { recorder?.markPause() }
        publish()
        notifyUpdate()
    }

    private fun stop() {
        if (!running) {
            cleanup()
            return
        }
        running = false
        worker?.join(1_000)
        record?.let {
            runCatching { it.stop() }
            it.unregisterAudioRecordingCallback(silenceWatcher)
            it.release()
        }
        record = null
        try {
            recorder?.finish()?.let { info ->
                RecorderBus.emit(RecEvent.Saved(info))
                notifySaved(info.durationSecs ?: elapsedSecs())
            }
        } catch (e: Exception) {
            Log.e(TAG, "falha ao fechar a gravação", e)
            RecorderBus.emit(RecEvent.Failed(e.message ?: e.javaClass.simpleName))
        }
        cleanup()
    }

    private fun cleanup() {
        running = false
        recorder?.close()
        recorder = null
        record?.release()
        record = null
        wakeLock?.let { if (it.isHeld) it.release() }
        wakeLock = null
        RecorderBus.update(RecState.Idle)
        refreshTile()
        ServiceCompat.stopForeground(this, ServiceCompat.STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    override fun onDestroy() {
        // O sistema encerrando o serviço com gravação aberta: fecha direito.
        if (running) stop()
        callbacks.shutdown()
        super.onDestroy()
    }

    // ---------------------------------------------------------- notificação

    private fun ensureChannels() {
        val nm = getSystemService(NotificationManager::class.java)
        nm.createNotificationChannel(
            NotificationChannel(CHANNEL_REC, getString(R.string.channel_recording), NotificationManager.IMPORTANCE_LOW)
                .apply { setShowBadge(false) },
        )
        nm.createNotificationChannel(
            NotificationChannel(CHANNEL_DONE, getString(R.string.channel_saved), NotificationManager.IMPORTANCE_DEFAULT),
        )
    }

    private fun action(action: String, code: Int): PendingIntent = PendingIntent.getService(
        this, code, Intent(this, RecordingService::class.java).setAction(action),
        PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
    )

    private fun openApp(): PendingIntent = PendingIntent.getActivity(
        this, 0, Intent(this, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
        PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
    )

    private fun notification(): Notification {
        val text = when {
            silenced -> getString(R.string.rec_notif_silenced)
            paused -> getString(R.string.rec_notif_paused)
            moments.isNotEmpty() -> resources.getQuantityString(R.plurals.rec_notif_moments, moments.size, moments.size)
            else -> getString(R.string.rec_notif_running)
        }
        val b = NotificationCompat.Builder(this, CHANNEL_REC)
            .setSmallIcon(R.drawable.ic_stat_rec)
            .setContentTitle(getString(if (paused) R.string.rec_notif_title_paused else R.string.rec_notif_title))
            .setContentText(text)
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .setCategory(NotificationCompat.CATEGORY_STOPWATCH)
            .setForegroundServiceBehavior(NotificationCompat.FOREGROUND_SERVICE_IMMEDIATE)
            .setContentIntent(openApp())
            .setShowWhen(true)
            .setUsesChronometer(!paused)
            .setWhen(System.currentTimeMillis() - (elapsedSecs() * 1000).toLong())
            .addAction(0, getString(R.string.rec_mark), action(ACTION_MARK, 1))
        if (paused) {
            b.addAction(0, getString(R.string.rec_resume), action(ACTION_RESUME, 2))
        } else {
            b.addAction(0, getString(R.string.rec_pause), action(ACTION_PAUSE, 2))
        }
        b.addAction(0, getString(R.string.rec_stop), action(ACTION_STOP, 3))
        return b.build()
    }

    private fun notifyUpdate() {
        if (!running) return
        getSystemService(NotificationManager::class.java).notify(NOTIFICATION_ID, notification())
    }

    private fun notifySaved(secs: Double) {
        val n = NotificationCompat.Builder(this, CHANNEL_DONE)
            .setSmallIcon(R.drawable.ic_stat_rec)
            .setContentTitle(getString(R.string.rec_saved_title))
            .setContentText(getString(R.string.rec_saved_text, formatDuration(secs)))
            .setContentIntent(openApp())
            .setAutoCancel(true)
            .build()
        getSystemService(NotificationManager::class.java).notify(SAVED_ID, n)
    }

    private fun refreshTile() {
        runCatching {
            TileService.requestListeningState(this, ComponentName(this, RecordTileService::class.java))
        }
    }

    companion object {
        const val TAG = "ISPerGravador"
        const val ACTION_START = "com.isper.mobile.GRAVAR"
        const val ACTION_STOP = "com.isper.mobile.PARAR"
        const val ACTION_MARK = "com.isper.mobile.MARCAR"
        const val ACTION_PAUSE = "com.isper.mobile.PAUSAR"
        const val ACTION_RESUME = "com.isper.mobile.CONTINUAR"
        private const val CHANNEL_REC = "gravacao"
        private const val CHANNEL_DONE = "gravacao-salva"
        private const val NOTIFICATION_ID = 42
        private const val SAVED_ID = 43
        private const val LEVELS_KEPT = 64
        /** O wake lock se solta sozinho depois de 12 h, se tudo mais falhar. */
        private const val MAX_RECORDING_MS = 12L * 60 * 60 * 1000

        /** Começa a gravar. Precisa ser chamado com o app em primeiro plano. */
        fun start(context: Context) {
            ContextCompat.startForegroundService(
                context, Intent(context, RecordingService::class.java).setAction(ACTION_START),
            )
        }

        fun send(context: Context, action: String) {
            context.startService(Intent(context, RecordingService::class.java).setAction(action))
        }
    }
}

/** 1:05:09 ou 12:34. */
fun formatDuration(secs: Double): String {
    val total = secs.toLong()
    val h = total / 3600
    val m = (total % 3600) / 60
    val s = total % 60
    return if (h > 0) "%d:%02d:%02d".format(h, m, s) else "%d:%02d".format(m, s)
}

