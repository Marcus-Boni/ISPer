package com.isper.mobile.recording

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.content.ContextCompat
import com.isper.mobile.MainActivity

/**
 * O "um toque para gravar" do widget e do bloco nas Configurações rápidas.
 *
 * É uma Activity de propósito: o Android só deixa ligar o microfone de um
 * serviço em primeiro plano quando o app está visível, então o atalho abre o
 * app por um instante. E ela **não é exportada** (ver o manifesto): só os
 * atalhos do próprio ISPer chegam aqui — nenhum outro app consegue disparar
 * uma gravação.
 */
class RecordShortcutActivity : ComponentActivity() {

    private val askPermissions =
        registerForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) { granted ->
            if (granted[Manifest.permission.RECORD_AUDIO] == true) startAndShow() else showApp()
        }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        if (RecorderBus.state.value is RecState.Active) {
            showApp()
            return
        }
        if (ContextCompat.checkSelfPermission(this, Manifest.permission.RECORD_AUDIO) ==
            PackageManager.PERMISSION_GRANTED
        ) {
            startAndShow()
        } else {
            askPermissions.launch(permissionsToAsk())
        }
    }

    private fun startAndShow() {
        RecordingService.start(this)
        showApp()
    }

    private fun showApp() {
        startActivity(
            Intent(this, MainActivity::class.java)
                .putExtra(MainActivity.EXTRA_TAB, MainActivity.TAB_RECORD)
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP),
        )
        finish()
    }

    companion object {
        fun permissionsToAsk(): Array<String> =
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                arrayOf(Manifest.permission.RECORD_AUDIO, Manifest.permission.POST_NOTIFICATIONS)
            } else {
                arrayOf(Manifest.permission.RECORD_AUDIO)
            }
    }
}
