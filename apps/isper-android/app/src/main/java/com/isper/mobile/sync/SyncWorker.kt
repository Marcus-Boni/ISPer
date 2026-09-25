package com.isper.mobile.sync

import android.Manifest
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.wifi.WifiManager
import android.util.Log
import androidx.core.app.NotificationCompat
import androidx.core.content.ContextCompat
import androidx.work.CoroutineWorker
import androidx.work.WorkerParameters
import com.isper.mobile.MainActivity
import com.isper.mobile.R
import com.isper.mobile.core.MobileException
import com.isper.mobile.core.ReadyMinutes
import com.isper.mobile.core.SyncListener
import com.isper.mobile.core.SyncSummary
import com.isper.mobile.recording.RecorderBus
import com.isper.mobile.recording.Storage
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext

/**
 * Uma rodada de sincronia com o PC, no WorkManager: manda o que falta,
 * pergunta pelo que está no PC e traz as atas prontas.
 *
 * - PC fora de alcance: tenta de novo mais tarde (1, 2, 4, 8, 15 min), por
 *   um agendamento à parte — a rodada sempre termina com sucesso, para
 *   nenhuma espera travar a fila do "Enviar agora";
 * - PC ainda transcrevendo: outra rodada em 90 s;
 * - ata que chegou: notificação "Ata pronta".
 */
class SyncWorker(context: Context, params: WorkerParameters) : CoroutineWorker(context, params) {

    override suspend fun doWork(): Result = withContext(Dispatchers.IO) {
        val ctx = applicationContext
        val link = PcSync.link(ctx)
        if (link.pairedPc() == null) return@withContext Result.success()
        PcSync.round.withLock {
            // O mDNS (achar o PC se o IP dele mudou) precisa receber multicast.
            val multicast = runCatching {
                ctx.getSystemService(WifiManager::class.java)
                    ?.createMulticastLock("isper-sync")
                    ?.apply { setReferenceCounted(false); acquire() }
            }.getOrNull()
            var summary: SyncSummary? = null
            try {
                summary = link.sync(
                    Storage.recordingsDir(ctx).path,
                    PcSync.deviceName(ctx),
                    PcSync.appVersion(ctx),
                    RecorderBus.activeId,
                    object : SyncListener {
                        override fun sending(recordingId: String, sent: ULong, total: ULong) {
                            // O WorkManager parou esta rodada (outra a substituiu):
                            // o envio para no ponto em que está e continua na próxima.
                            if (isStopped) link.cancel()
                            PcSync.progress(recordingId, sent, total)
                        }
                    },
                )
                summary.ready.forEach { notifyReady(ctx, it) }
                Log.i(TAG, "rodada: ${summary.sent} enviada(s), ${summary.ready.size} ata(s), ${summary.waiting} no PC, ${summary.unsent} pendente(s)" +
                    (summary.error?.let { " — $it" } ?: ""))
                when {
                    summary.error != null && summary.unsent > 0u -> PcSync.retryLater(ctx)
                    summary.waiting > 0u -> { PcSync.reached(); PcSync.syncLater(ctx, 90) }
                    else -> PcSync.reached()
                }
                Result.success()
            } catch (e: MobileException.NotPaired) {
                Log.w(TAG, "o PC esqueceu este celular")
                PcSync.forgetLocally(ctx)
                Result.success()
            } catch (e: MobileException) {
                Log.w(TAG, "rodada falhou", e)
                PcSync.retryLater(ctx)
                Result.success()
            } finally {
                runCatching { multicast?.release() }
                PcSync.roundFinished(ctx, summary)
            }
        }
    }

    private fun notifyReady(ctx: Context, ready: ReadyMinutes) {
        if (ContextCompat.checkSelfPermission(ctx, Manifest.permission.POST_NOTIFICATIONS) !=
            PackageManager.PERMISSION_GRANTED
        ) {
            return
        }
        val nm = ctx.getSystemService(NotificationManager::class.java)
        nm.createNotificationChannel(
            NotificationChannel(CHANNEL, ctx.getString(R.string.channel_minutes), NotificationManager.IMPORTANCE_DEFAULT),
        )
        val open = PendingIntent.getActivity(
            ctx,
            ready.recordingId.hashCode(),
            Intent(ctx, MainActivity::class.java)
                .putExtra(MainActivity.EXTRA_TAB, MainActivity.TAB_LIBRARY)
                .putExtra(MainActivity.EXTRA_MINUTES, ready.recordingId)
                .addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val n = NotificationCompat.Builder(ctx, CHANNEL)
            .setSmallIcon(R.drawable.ic_stat_rec)
            .setContentTitle(ctx.getString(R.string.sync_minutes_ready))
            .setContentText(ready.title)
            .setContentIntent(open)
            .setAutoCancel(true)
            .build()
        nm.notify(ready.recordingId.hashCode(), n)
    }

    companion object {
        private const val TAG = "ISPerSincronia"
        private const val CHANNEL = "atas"
    }
}
