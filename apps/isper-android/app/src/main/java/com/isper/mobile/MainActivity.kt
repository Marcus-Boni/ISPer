package com.isper.mobile

import android.content.ContentResolver
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import androidx.compose.runtime.mutableIntStateOf
import com.isper.mobile.library.LibraryViewModel

/**
 * A casa do app: três destinos (Gravar, Biblioteca, Laboratório).
 *
 * Esta Activity é exportada (é a do ícone), então não aceita nenhuma intent
 * que comece uma gravação — isso só acontece por um toque na tela ou pelos
 * atalhos do próprio ISPer ([com.isper.mobile.recording.RecordShortcutActivity]).
 * O que ela aceita de fora: um áudio compartilhado (vira gravação importada)
 * e o `autorun` do laboratório, que só processa a amostra embutida.
 */
class MainActivity : ComponentActivity() {
    private val lab: SpikeViewModel by viewModels()
    private val library: LibraryViewModel by viewModels()
    private val tab = mutableIntStateOf(TAB_RECORD)

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        if (savedInstanceState == null) handle(intent) else tab.intValue = savedInstanceState.getInt(STATE_TAB)
        setContent {
            IsperTheme {
                IsperApp(tab = tab.intValue, onTab = { tab.intValue = it }, library = library, lab = lab)
            }
        }
    }

    override fun onResume() {
        super.onResume()
        // Uma gravação que caiu enquanto o app estava fechado aparece aqui.
        library.refresh()
    }

    override fun onSaveInstanceState(outState: Bundle) {
        super.onSaveInstanceState(outState)
        outState.putInt(STATE_TAB, tab.intValue)
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handle(intent)
    }

    private fun handle(intent: Intent?) {
        intent ?: return
        intent.getIntExtra(EXTRA_TAB, -1).takeIf { it in TAB_RECORD..TAB_LAB }?.let { tab.intValue = it }
        if (intent.action == Intent.ACTION_SEND) {
            // Só content://: um file:// de outro app poderia apontar para os
            // arquivos privados do próprio ISPer.
            sharedUri(intent)?.takeIf { it.scheme == ContentResolver.SCHEME_CONTENT }?.let {
                library.importShared(it)
                tab.intValue = TAB_LIBRARY
            }
        }
        if (intent.getBooleanExtra("autorun", false)) {
            tab.intValue = TAB_LAB
            lab.autorun(
                model = intent.getStringExtra("model") ?: "ggml-tiny-q5_1.bin",
                diarize = intent.getBooleanExtra("diarize", true),
            )
        }
    }

    private fun sharedUri(intent: Intent): Uri? =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            intent.getParcelableExtra(Intent.EXTRA_STREAM, Uri::class.java)
        } else {
            @Suppress("DEPRECATION")
            intent.getParcelableExtra(Intent.EXTRA_STREAM)
        }

    companion object {
        const val EXTRA_TAB = "com.isper.mobile.ABA"
        const val TAB_RECORD = 0
        const val TAB_LIBRARY = 1
        const val TAB_LAB = 2
        private const val STATE_TAB = "aba"
    }
}
