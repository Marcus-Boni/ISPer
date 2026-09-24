package com.isper.mobile

import android.content.Intent
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import androidx.compose.runtime.getValue
import androidx.lifecycle.compose.collectAsStateWithLifecycle

class MainActivity : ComponentActivity() {
    private val viewModel: SpikeViewModel by viewModels()

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        if (savedInstanceState == null) handleAutorun(intent)
        setContent {
            IsperTheme {
                val state by viewModel.state.collectAsStateWithLifecycle()
                SpikeScreen(state = state, actions = viewModel)
            }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handleAutorun(intent)
    }

    private fun handleAutorun(intent: Intent?) {
        if (intent?.getBooleanExtra("autorun", false) != true) return
        viewModel.autorun(
            model = intent.getStringExtra("model") ?: "ggml-tiny-q5_1.bin",
            diarize = intent.getBooleanExtra("diarize", true),
        )
    }
}
