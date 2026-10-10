package com.isper.mobile.today

import android.Manifest
import android.app.AlarmManager
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.util.Log
import androidx.core.app.NotificationCompat
import androidx.core.content.ContextCompat
import com.isper.mobile.MainActivity
import com.isper.mobile.R
import com.isper.mobile.core.MobileTask
import java.time.LocalDate
import java.time.LocalDateTime
import java.time.LocalTime
import java.time.ZoneId
import java.util.concurrent.TimeUnit

/**
 * Os avisos das tarefas com hora (Fase 10.6): na hora marcada, uma
 * notificação com "Concluir". Valem também para as rotinas, cuja tarefa de
 * cada dia vem do PC com a hora da rotina.
 *
 * O alarme é o inexato que dispara mesmo com o aparelho em repouso
 * ([AlarmManager.setAndAllowWhileIdle]): não pede a permissão de alarme
 * exato, e o Android pode atrasá-lo alguns minutos para poupar bateria.
 * A lista do que está agendado fica nas preferências, para cancelar o que
 * deixou de valer (concluída, mudou de hora, saiu do retrato).
 */
object Reminders {
    private const val TAG = "ISPerLembretes"
    private const val PREFS = "lembretes"
    private const val KEY_IDS = "agendados"
    private const val CHANNEL = "lembretes"
    internal const val ACTION_REMIND = "com.isper.mobile.LEMBRAR"
    internal const val ACTION_DONE = "com.isper.mobile.CONCLUIR"
    internal const val EXTRA_ID = "id"

    /** Até quantos dias à frente se agenda (o resto entra numa próxima vez). */
    private val HORIZON_MS = TimeUnit.DAYS.toMillis(7)

    private fun at(t: MobileTask): Long? = runCatching {
        LocalDateTime.of(LocalDate.parse(t.plannedOn), LocalTime.parse(t.plannedTime))
            .atZone(ZoneId.systemDefault()).toInstant().toEpochMilli()
    }.getOrNull()

    private fun pending(context: Context, id: String): PendingIntent =
        PendingIntent.getBroadcast(
            context,
            id.hashCode(),
            Intent(context, ReminderReceiver::class.java).setAction(ACTION_REMIND).putExtra(EXTRA_ID, id),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )

    /** Refaz os alarmes a partir do dia (rápido: só lê os arquivos do núcleo). */
    fun reschedule(context: Context) {
        val day = runCatching { Tasks.day(context) }.getOrElse {
            Log.w(TAG, "não consegui ler as tarefas", it)
            return
        }
        val now = System.currentTimeMillis()
        val wanted = (day.planned + day.later)
            .filter { it.status == "open" && it.plannedOn != null && it.plannedTime != null }
            .mapNotNull { t -> at(t)?.takeIf { it > now && it - now < HORIZON_MS }?.let { t.id to it } }
        val am = context.getSystemService(AlarmManager::class.java) ?: return
        val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        val old = prefs.getStringSet(KEY_IDS, emptySet()).orEmpty()
        val ids = wanted.map { it.first }.toSet()
        for (gone in old - ids) am.cancel(pending(context, gone))
        for ((id, whenMs) in wanted) {
            am.setAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, whenMs, pending(context, id))
        }
        prefs.edit().putStringSet(KEY_IDS, ids).apply()
    }

    private fun channel(context: Context): NotificationManager {
        val nm = context.getSystemService(NotificationManager::class.java)
        nm.createNotificationChannel(
            NotificationChannel(CHANNEL, context.getString(R.string.channel_reminders), NotificationManager.IMPORTANCE_HIGH),
        )
        return nm
    }

    internal fun notify(context: Context, task: MobileTask) {
        if (ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) !=
            PackageManager.PERMISSION_GRANTED
        ) {
            return
        }
        val open = PendingIntent.getActivity(
            context,
            task.id.hashCode(),
            Intent(context, MainActivity::class.java)
                .putExtra(MainActivity.EXTRA_TAB, MainActivity.TAB_TODAY)
                .addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val done = PendingIntent.getBroadcast(
            context,
            task.id.hashCode() + 1,
            Intent(context, ReminderReceiver::class.java).setAction(ACTION_DONE).putExtra(EXTRA_ID, task.id),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val n = NotificationCompat.Builder(context, CHANNEL)
            .setSmallIcon(R.drawable.ic_stat_task)
            .setContentTitle(task.title)
            .setContentText(task.plannedTime?.let { context.getString(R.string.reminder_at, it) })
            .setCategory(NotificationCompat.CATEGORY_REMINDER)
            .setContentIntent(open)
            .addAction(0, context.getString(R.string.reminder_done), done)
            .setAutoCancel(true)
            .build()
        channel(context).notify(task.id.hashCode(), n)
    }

    internal fun dismiss(context: Context, id: String) {
        context.getSystemService(NotificationManager::class.java)?.cancel(id.hashCode())
    }
}

/** O alarme de uma tarefa e o "Concluir" da notificação. */
class ReminderReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val id = intent.getStringExtra(Reminders.EXTRA_ID) ?: return
        val result = goAsync()
        Thread {
            try {
                when (intent.action) {
                    Reminders.ACTION_REMIND -> {
                        // Concluída ou mudada desde que o alarme foi posto? Então não avisa.
                        val day = Tasks.day(context)
                        (day.planned + day.later)
                            .firstOrNull { it.id == id && it.status == "open" }
                            ?.let { Reminders.notify(context, it) }
                    }
                    Reminders.ACTION_DONE -> {
                        Tasks.book(context).setStatus(id, "done", Tasks.now())
                        Reminders.dismiss(context, id)
                        Tasks.changedHere(context)
                    }
                }
            } catch (e: Exception) {
                Log.w("ISPerLembretes", "lembrete ${intent.action}", e)
            } finally {
                result.finish()
            }
        }.start()
    }
}

/** Depois de religar o aparelho (ou atualizar o app), os alarmes voltam. */
class RemindersBootReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action == Intent.ACTION_BOOT_COMPLETED || intent.action == Intent.ACTION_MY_PACKAGE_REPLACED) {
            val result = goAsync()
            Thread {
                try {
                    Reminders.reschedule(context)
                } finally {
                    result.finish()
                }
            }.start()
        }
    }
}
