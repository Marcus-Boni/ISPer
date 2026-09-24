package com.isper.mobile.recording

import android.annotation.SuppressLint
import android.app.PendingIntent
import android.content.Intent
import android.os.Build
import android.service.quicksettings.Tile
import android.service.quicksettings.TileService
import com.isper.mobile.R

/**
 * O bloco "Gravar" nas Configurações rápidas: dá para começar uma gravação
 * sem desbloquear a lista de apps, e parar pelo mesmo lugar.
 */
class RecordTileService : TileService() {

    override fun onStartListening() {
        refresh()
    }

    @SuppressLint("StartActivityAndCollapseDeprecated")
    override fun onClick() {
        if (RecorderBus.state.value is RecState.Active) {
            RecordingService.send(this, RecordingService.ACTION_STOP)
        } else {
            val intent = Intent(this, RecordShortcutActivity::class.java)
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
                startActivityAndCollapse(
                    PendingIntent.getActivity(this, 0, intent, PendingIntent.FLAG_IMMUTABLE),
                )
            } else {
                @Suppress("DEPRECATION")
                startActivityAndCollapse(intent)
            }
        }
        refresh()
    }

    private fun refresh() {
        val tile = qsTile ?: return
        val active = RecorderBus.state.value is RecState.Active
        tile.state = if (active) Tile.STATE_ACTIVE else Tile.STATE_INACTIVE
        tile.label = getString(if (active) R.string.tile_stop else R.string.tile_record)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            tile.subtitle = getString(R.string.app_name)
        }
        tile.updateTile()
    }
}
