package com.isper.mobile.transcribe

import android.Manifest
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import androidx.core.app.NotificationCompat
import androidx.core.content.ContextCompat
import com.isper.mobile.MainActivity
import com.isper.mobile.R

/** As notificações da transcrição no celular (Fase 9.4). */
internal object Notifications {
    /** O andamento (baixar modelos, transcrever): discreto, sem som. */
    private const val CHANNEL_WORK = "transcricao"
    /** O mesmo canal da "Ata pronta" do PC (Fase 9.3). */
    private const val CHANNEL_MINUTES = "atas"
    const val DOWNLOAD_ID = 9401
    const val TRANSCRIBE_ID = 9402

    private fun channels(ctx: Context): NotificationManager {
        val nm = ctx.getSystemService(NotificationManager::class.java)
        nm.createNotificationChannel(
            NotificationChannel(CHANNEL_WORK, ctx.getString(R.string.channel_transcribing), NotificationManager.IMPORTANCE_LOW),
        )
        nm.createNotificationChannel(
            NotificationChannel(CHANNEL_MINUTES, ctx.getString(R.string.channel_minutes), NotificationManager.IMPORTANCE_DEFAULT),
        )
        return nm
    }

    private fun openLibrary(ctx: Context, minutesId: String? = null): PendingIntent {
        val intent = Intent(ctx, MainActivity::class.java)
            .putExtra(MainActivity.EXTRA_TAB, MainActivity.TAB_LIBRARY)
            .addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP)
        minutesId?.let { intent.putExtra(MainActivity.EXTRA_MINUTES, it) }
        return PendingIntent.getActivity(
            ctx,
            minutesId?.hashCode() ?: 0,
            intent,
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
    }

    /** A notificação do trabalho em andamento, com a barra de progresso. */
    fun progress(ctx: Context, title: String, text: String, pct: Int): Notification {
        channels(ctx)
        return NotificationCompat.Builder(ctx, CHANNEL_WORK)
            .setSmallIcon(R.drawable.ic_stat_minutes)
            .setContentTitle(title)
            .setContentText(text)
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .setSilent(true)
            .setProgress(100, pct.coerceIn(0, 100), pct <= 0)
            .setContentIntent(openLibrary(ctx))
            .build()
    }

    /** "Ata pronta", para a ata feita no celular: abre a tela da ata. */
    fun minutesReady(ctx: Context, recordingId: String, title: String) {
        if (ContextCompat.checkSelfPermission(ctx, Manifest.permission.POST_NOTIFICATIONS) !=
            PackageManager.PERMISSION_GRANTED
        ) {
            return
        }
        val n = NotificationCompat.Builder(ctx, CHANNEL_MINUTES)
            .setSmallIcon(R.drawable.ic_stat_minutes)
            .setContentTitle(ctx.getString(R.string.local_minutes_ready))
            .setContentText(title)
            .setContentIntent(openLibrary(ctx, recordingId))
            .setAutoCancel(true)
            .build()
        channels(ctx).notify(recordingId.hashCode(), n)
    }
}
