package com.isper.mobile.recording

import android.app.PendingIntent
import android.appwidget.AppWidgetManager
import android.appwidget.AppWidgetProvider
import android.content.Context
import android.content.Intent
import android.widget.RemoteViews
import com.isper.mobile.R

/** Widget da tela inicial: um botão que começa a gravar. */
class RecordWidget : AppWidgetProvider() {
    override fun onUpdate(context: Context, manager: AppWidgetManager, ids: IntArray) {
        val tap = PendingIntent.getActivity(
            context,
            0,
            Intent(context, RecordShortcutActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val views = RemoteViews(context.packageName, R.layout.widget_record).apply {
            setOnClickPendingIntent(R.id.widget_root, tap)
        }
        ids.forEach { manager.updateAppWidget(it, views) }
    }
}
