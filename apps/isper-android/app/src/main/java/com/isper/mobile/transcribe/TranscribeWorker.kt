package com.isper.mobile.transcribe

import android.content.Context
import android.content.pm.ServiceInfo
import android.os.Build
import android.util.Log
import androidx.work.CoroutineWorker
import androidx.work.ForegroundInfo
import androidx.work.WorkerParameters
import com.isper.mobile.R
import com.isper.mobile.core.LocalOptions
import com.isper.mobile.core.LocalTranscription
import com.isper.mobile.core.MobileException
import com.isper.mobile.core.ProgressListener
import com.isper.mobile.core.Stage
import com.isper.mobile.core.markLocalFailed
import com.isper.mobile.core.pendingLocal
import com.isper.mobile.recording.RecorderBus
import com.isper.mobile.recording.Storage
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * Transcreve no celular as gravações que ainda não têm ata (Fase 9.4), uma
 * por vez, da mais nova para a mais antiga.
 *
 * - Roda em primeiro plano, com a notificação do andamento: uma reunião de
 *   1 h leva mais de uma hora num intermediário, bem mais que os 10 min de um
 *   trabalho comum.
 * - Se o Android parar o trabalho (a tomada saiu, faltou memória), a
 *   transcrição para na janela em que está e o que foi feito fica guardado. O
 *   WorkManager roda de novo quando as condições voltam, e ela continua dali.
 * - Uma gravação que falha tem a falha anotada e sai da fila: não é tentada
 *   de novo em toda rodada. "Tentar de novo", na Biblioteca, a devolve.
 */
class TranscribeWorker(context: Context, params: WorkerParameters) : CoroutineWorker(context, params) {

    override suspend fun doWork(): Result = withContext(Dispatchers.IO) {
        val ctx = applicationContext
        val setup = LocalTranscribe.setup(ctx)
        val dir = Storage.recordingsDir(ctx).path
        val options = LocalOptions(
            modelPath = LocalTranscribe.modelFile(ctx, setup.modelFile).path,
            vadPath = LocalTranscribe.vadFile(ctx).path,
            lang = "pt",
            diarizeModelsDir = if (setup.diarize) LocalTranscribe.diarizeDir(ctx).path else null,
        )
        if (!LocalTranscribe.modelsReady(ctx, setup)) {
            // Os modelos sumiram (apagados nos Ajustes) entre o download e aqui.
            Log.w(TAG, "modelos ausentes: a transcrição fica para o próximo agendamento")
            return@withContext Result.success()
        }
        val tried = mutableSetOf<String>()
        try {
            while (!isStopped) {
                val id = pendingLocal(dir, RecorderBus.activeId).firstOrNull { it !in tried } ?: break
                tried += id
                transcribeOne(ctx, dir, id, options)
            }
        } finally {
            LocalTranscribe.finished()
        }
        Result.success()
    }

    private suspend fun transcribeOne(ctx: Context, dir: String, id: String, options: LocalOptions) {
        val job = LocalTranscription()
        val title = ctx.getString(R.string.local_transcribing_title)
        setForeground(foregroundInfo(ctx, title, ctx.getString(R.string.local_stage_reading), 0))
        val listener = object : ProgressListener {
            private var last = -1
            override fun onProgress(stage: Stage, done: ULong, total: ULong) {
                // O WorkManager parou o trabalho: para na próxima janela, e o
                // que já foi transcrito fica guardado para a próxima rodada.
                if (isStopped) job.cancel()
                val fraction = if (total == 0UL) 0f else (done.toDouble() / total.toDouble()).toFloat()
                val (text, pct) = when (stage) {
                    Stage.DECODE -> ctx.getString(R.string.local_stage_reading) to 0
                    Stage.TRANSCRIBE -> {
                        val p = (fraction * 100).toInt()
                        ctx.getString(R.string.local_stage_transcribing, p) to p
                    }
                    Stage.DIARIZE -> ctx.getString(R.string.local_stage_speakers) to 100
                    Stage.DOWNLOAD -> return
                }
                LocalTranscribe.progress(id, if (stage == Stage.TRANSCRIBE) fraction else if (stage == Stage.DIARIZE) 1f else 0f, text)
                if (pct != last || stage != Stage.TRANSCRIBE) {
                    last = pct
                    runCatching { setForegroundAsync(foregroundInfo(ctx, title, text, pct)) }
                }
            }
        }
        try {
            val minutes = job.run(dir, id, options, listener)
            Log.i(
                TAG,
                "ata feita no celular: $id, ${minutes.audioSecs.toInt()} s de áudio em ${minutes.totalSecs.toInt()} s" +
                    (if (minutes.resumedWindows > 0u) ", continuando ${minutes.resumedWindows} janelas" else ""),
            )
            Notifications.minutesReady(ctx, id, minutes.title)
            LocalTranscribe.notifyChanged()
        } catch (e: MobileException.Cancelled) {
            Log.i(TAG, "transcrição de $id parada: continua da próxima vez")
        } catch (e: MobileException.SupersededByPc) {
            Log.i(TAG, "a ata do PC de $id chegou antes: a do celular não é gravada")
            LocalTranscribe.notifyChanged()
        } catch (e: MobileException) {
            Log.w(TAG, "a transcrição de $id falhou", e)
            runCatching { markLocalFailed(dir, id, e.message ?: e.javaClass.simpleName) }
            LocalTranscribe.notifyChanged()
        }
    }

    private fun foregroundInfo(ctx: Context, title: String, text: String, pct: Int): ForegroundInfo {
        val n = Notifications.progress(ctx, title, text, pct)
        // Android 15+: "processamento de mídia" é o tipo certo para um
        // trabalho longo sobre um arquivo de áudio; antes dele, dataSync
        // cobre "processar arquivos locais".
        val type = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.VANILLA_ICE_CREAM) {
            ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PROCESSING
        } else {
            ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC
        }
        return ForegroundInfo(Notifications.TRANSCRIBE_ID, n, type)
    }

    companion object {
        private const val TAG = "ISPerTranscricao"
    }
}
