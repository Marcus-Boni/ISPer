package com.isper.mobile.transcribe

import android.content.Context
import android.content.pm.ServiceInfo
import android.util.Log
import androidx.work.CoroutineWorker
import androidx.work.ForegroundInfo
import androidx.work.WorkerParameters
import com.isper.mobile.R
import com.isper.mobile.core.MobileException
import com.isper.mobile.core.ProgressListener
import com.isper.mobile.core.Stage
import com.isper.mobile.core.diarizeModelsInstalled
import com.isper.mobile.core.downloadDiarizeModels
import com.isper.mobile.core.downloadModel
import com.isper.mobile.core.downloadVad
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * Baixa o que a transcrição no celular precisa e ainda não está no aparelho: o
 * modelo Whisper do plano, o VAD e, com os falantes ligados, os dois modelos
 * de diarização. O núcleo confere o SHA-256 de cada um, como no PC.
 *
 * Vem antes do [TranscribeWorker] na mesma fila. Sem rede, o WorkManager
 * espera; um download que caiu tenta de novo com espera crescente.
 */
class ModelsWorker(context: Context, params: WorkerParameters) : CoroutineWorker(context, params) {

    override suspend fun doWork(): Result = withContext(Dispatchers.IO) {
        val ctx = applicationContext
        val setup = LocalTranscribe.setup(ctx)
        if (LocalTranscribe.modelsReady(ctx, setup)) return@withContext Result.success()
        // Na primeira vez são ~230 MB: pode passar dos 10 min que o Android
        // dá a um trabalho comum.
        runCatching { setForeground(foregroundInfo(ctx, 0)) }
        val listener = object : ProgressListener {
            private var last = -1
            override fun onProgress(stage: Stage, done: ULong, total: ULong) {
                if (total == 0UL) return
                val pct = (done.toDouble() * 100 / total.toDouble()).toInt()
                if (pct != last) {
                    last = pct
                    runCatching { setForegroundAsync(foregroundInfo(ctx, pct)) }
                }
            }
        }
        val dir = LocalTranscribe.modelsDir(ctx).path
        try {
            if (!LocalTranscribe.modelFile(ctx, setup.modelFile).isFile) downloadModel(setup.modelFile, dir, listener)
            if (!LocalTranscribe.vadFile(ctx).isFile) downloadVad(dir, listener)
            val diarize = LocalTranscribe.diarizeDir(ctx).path
            if (setup.diarize && !diarizeModelsInstalled(diarize)) downloadDiarizeModels(diarize, listener)
            Result.success()
        } catch (e: MobileException) {
            Log.w(TAG, "não consegui baixar os modelos: ${e.message}")
            if (runAttemptCount < MAX_ATTEMPTS) Result.retry() else Result.failure()
        }
    }

    private fun foregroundInfo(ctx: Context, pct: Int): ForegroundInfo {
        val n = Notifications.progress(
            ctx,
            ctx.getString(R.string.local_downloading_title),
            ctx.getString(R.string.local_downloading_text, pct),
            pct,
        )
        return ForegroundInfo(Notifications.DOWNLOAD_ID, n, ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC)
    }

    companion object {
        private const val TAG = "ISPerModelos"
        private const val MAX_ATTEMPTS = 5
    }
}
