package com.isper.mobile

import android.app.Application
import android.net.Uri
import android.provider.OpenableColumns
import android.util.Log
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import com.isper.mobile.core.EngineInfo
import com.isper.mobile.core.MobileException
import com.isper.mobile.core.ModelOption
import com.isper.mobile.core.ProgressListener
import com.isper.mobile.core.SpikeReport
import com.isper.mobile.core.Stage
import com.isper.mobile.core.TranscribeOptions
import com.isper.mobile.core.TranscriptionJob
import com.isper.mobile.core.diarizeModelsInstalled
import com.isper.mobile.core.downloadDiarizeModels
import com.isper.mobile.core.downloadModel
import com.isper.mobile.core.downloadVad
import com.isper.mobile.core.engineInfo
import com.isper.mobile.core.mobileModels
import java.io.File
import java.time.Instant
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject

/** De onde vem o áudio medido. */
sealed interface AudioChoice {
    data object Sample : AudioChoice
    data class Picked(val file: File, val name: String) : AudioChoice
}

data class ModelState(val option: ModelOption, val installed: Boolean)

data class Progress(val stage: Stage, val done: Long, val total: Long) {
    val fraction: Float? get() = if (total > 0) (done.toFloat() / total).coerceIn(0f, 1f) else null
}

data class SpikeResult(
    val report: SpikeReport,
    val before: PowerSnapshot,
    val after: PowerSnapshot,
    val json: String,
)

data class SpikeUiState(
    val missingCpu: List<String> = emptyList(),
    val loadError: String? = null,
    val engine: EngineInfo? = null,
    val device: DeviceInfo? = null,
    val power: PowerSnapshot? = null,
    val models: List<ModelState> = emptyList(),
    val selectedModel: String? = null,
    val audio: AudioChoice = AudioChoice.Sample,
    val sampleHasReference: Boolean = false,
    val diarize: Boolean = true,
    val busy: Boolean = false,
    val progress: Progress? = null,
    val message: String? = null,
    val result: SpikeResult? = null,
)

/**
 * O laboratório da Fase 9.1: baixa os modelos e roda o passe final do PC no
 * aparelho, pela fachada `isper-mobile`. Todo o trabalho pesado acontece no
 * Rust, numa thread de E/S; aqui só se guarda o estado da tela.
 */
class SpikeViewModel(app: Application) : AndroidViewModel(app) {
    private val context = app.applicationContext
    private val modelsDir = File(context.filesDir, "models").apply { mkdirs() }
    private val diarizeDir = File(modelsDir, "diarize").apply { mkdirs() }
    private val sampleDir = File(context.cacheDir, "spike")

    private val _state = MutableStateFlow(SpikeUiState())
    val state: StateFlow<SpikeUiState> = _state.asStateFlow()

    private var job: TranscriptionJob? = null

    private val appVersion: String =
        runCatching { context.packageManager.getPackageInfo(context.packageName, 0).versionName }
            .getOrNull() ?: "?"

    private val listener = object : ProgressListener {
        override fun onProgress(stage: Stage, done: ULong, total: ULong) {
            _state.update { it.copy(progress = Progress(stage, done.toLong(), total.toLong())) }
        }
    }

    init {
        val missing = DeviceProbe.missingCpuFeatures()
        _state.update {
            it.copy(
                missingCpu = missing,
                device = DeviceProbe.info(context),
                power = DeviceProbe.power(context),
                sampleHasReference = assetExists("spike/amostra.ref.txt"),
            )
        }
        if (missing.isEmpty()) {
            viewModelScope.launch {
                // A primeira chamada carrega a libisper_mobile.so (JNA).
                val loaded = withContext(Dispatchers.IO) {
                    runCatching { engineInfo() to mobileModels() }
                }
                loaded.onSuccess { (info, options) ->
                    _state.update {
                        it.copy(
                            engine = info,
                            models = options.map { o -> ModelState(o, installed(o.file)) },
                            // Uma escolha feita antes (pelo autorun) não é sobrescrita.
                            selectedModel = it.selectedModel
                                ?: options.firstOrNull { o -> installed(o.file) }?.file
                                ?: options.getOrNull(2)?.file,
                        )
                    }
                }.onFailure { e ->
                    Log.e(TAG, "motor não carregou", e)
                    _state.update { it.copy(loadError = e.message ?: e.javaClass.simpleName) }
                }
            }
        }
    }

    fun selectModel(file: String) = _state.update { it.copy(selectedModel = file) }

    fun setDiarize(on: Boolean) = _state.update { it.copy(diarize = on) }

    fun useSample() = _state.update { it.copy(audio = AudioChoice.Sample) }

    /** Uma gravação da Biblioteca, para medir o passe final sobre ela. */
    fun useRecording(path: String, name: String) =
        _state.update { it.copy(audio = AudioChoice.Picked(File(path), name), message = null, result = null) }

    fun pickAudio(uri: Uri) {
        viewModelScope.launch {
            val picked = withContext(Dispatchers.IO) { runCatching { copyToCache(uri) } }
            picked.onSuccess { choice -> _state.update { it.copy(audio = choice, message = null) } }
                .onFailure { e -> _state.update { it.copy(message = e.message) } }
        }
    }

    fun download(file: String) {
        if (_state.value.busy) return
        viewModelScope.launch { guarded { ensureModels(file, _state.value.diarize) } }
    }

    fun run() {
        val s = _state.value
        val model = s.selectedModel
        if (s.busy || model == null) return
        viewModelScope.launch { guarded { measure(model, s.diarize, s.audio) } }
    }

    fun cancel() {
        job?.cancel()
    }

    /**
     * Roda sem ninguém tocar na tela (usado pelo teste no emulador e para
     * medir em série): `adb shell am start -n com.isper.mobile/.MainActivity
     * --ez autorun true --es model ggml-tiny-q5_1.bin --ez diarize true`.
     * O relatório sai no logcat (tag `ISPerSpike`) e em
     * `Android/data/com.isper.mobile/files/spike-report.json`.
     */
    fun autorun(model: String, diarize: Boolean) {
        if (_state.value.busy) return
        _state.update { it.copy(selectedModel = model, diarize = diarize, audio = AudioChoice.Sample) }
        viewModelScope.launch {
            val outcome = runCatching {
                withContext(Dispatchers.IO) {
                    ensureModels(model, diarize)
                    measure(model, diarize, AudioChoice.Sample)
                }
            }
            outcome.onFailure { e ->
                val error = JSONObject().put("erro", e.message ?: e.javaClass.simpleName).toString()
                Log.e(TAG, "ERROR $error", e)
                writeReport(error)
            }
            _state.update {
                it.copy(busy = false, progress = null, message = outcome.exceptionOrNull()?.message)
            }
        }
    }

    private suspend fun guarded(block: suspend () -> Unit) {
        try {
            withContext(Dispatchers.IO) { block() }
        } catch (e: MobileException) {
            _state.update { it.copy(message = e.message) }
        } catch (e: Exception) {
            Log.e(TAG, "falha no laboratório", e)
            _state.update { it.copy(message = e.message ?: e.javaClass.simpleName) }
        } finally {
            _state.update { it.copy(busy = false, progress = null) }
        }
    }

    private fun ensureModels(file: String, diarize: Boolean) {
        _state.update { it.copy(busy = true, message = null) }
        if (!installed(file)) downloadModel(file, modelsDir.path, listener)
        downloadVad(modelsDir.path, listener)
        if (diarize && !diarizeModelsInstalled(diarizeDir.path)) {
            downloadDiarizeModels(diarizeDir.path, listener)
        }
        _state.update { s ->
            s.copy(models = s.models.map { it.copy(installed = installed(it.option.file)) })
        }
    }

    private fun measure(model: String, diarize: Boolean, audio: AudioChoice) {
        if (!installed(model)) {
            ensureModels(model, diarize)
        }
        if (diarize && !diarizeModelsInstalled(diarizeDir.path)) {
            downloadDiarizeModels(diarizeDir.path, listener)
        }
        _state.update { it.copy(busy = true, message = null, result = null) }
        val (path, name, reference, turns) = when (audio) {
            AudioChoice.Sample -> {
                val wav = extractSample()
                Quad(wav.path, "reunião de exemplo", readAsset("spike/amostra.ref.txt"), readAsset("spike/amostra.turns.tsv"))
            }
            is AudioChoice.Picked -> Quad(audio.file.path, audio.name, null, null)
        }
        val vad = downloadVad(modelsDir.path, listener)
        val before = DeviceProbe.power(context)
        val running = TranscriptionJob()
        job = running
        val report = try {
            running.run(
                path,
                TranscribeOptions(
                    modelPath = File(modelsDir, model).path,
                    vadPath = vad,
                    lang = "pt",
                    diarizeModelsDir = if (diarize) diarizeDir.path else null,
                    numSpeakers = null,
                    diarizeThreads = null,
                    referenceText = reference,
                    referenceTurns = turns,
                ),
                listener,
            )
        } finally {
            job = null
            running.close()
        }
        val after = DeviceProbe.power(context)
        val json = reportJson(report, name, before, after)
        Log.i(TAG, "RESULT $json")
        writeReport(json)
        _state.update { it.copy(result = SpikeResult(report, before, after, json), power = after) }
    }

    private fun reportJson(r: SpikeReport, audioName: String, before: PowerSnapshot, after: PowerSnapshot): String {
        val d = _state.value.device
        val e = _state.value.engine
        fun power(p: PowerSnapshot) = JSONObject()
            .put("bateria_pct", p.batteryPercent)
            .put("temperatura_c", p.temperatureC?.toDouble())
            .put("carregando", p.charging)
            .put("termico", p.thermal)
        return JSONObject()
            .put("app", "ISPer laboratório $appVersion")
            .put("quando", Instant.now().toString())
            .put("aparelho", JSONObject()
                .put("fabricante", d?.manufacturer)
                .put("modelo", d?.model)
                .put("processador", d?.soc)
                .put("sistema", d?.system)
                .put("abi", d?.abi)
                .put("memoria_mb", d?.totalRamMb)
                .put("threads", r.cpuThreads.toInt()))
            .put("motor", JSONObject().put("versao", e?.version).put("alvo", e?.target))
            .put("audio", audioName)
            .put("modelo", r.model)
            .put("duracao_audio_s", r.audioSecs.finite())
            .put("tempos_s", JSONObject()
                .put("leitura", r.decodeSecs.finite())
                .put("carga", r.loadSecs.finite())
                .put("transcricao", r.transcribeSecs.finite())
                .put("falantes", r.diarizeSecs.finite())
                .put("total", r.totalSecs.finite()))
            .put("fator_tempo_real", r.realtimeFactor.finite())
            .put("pico_memoria_mb", r.peakRssMb?.finite())
            .put("falantes", r.speakers.toInt())
            .put("wer", r.wer?.finite())
            .put("cer", r.cer?.finite())
            .put("der", r.der?.finite())
            .put("energia_antes", power(before))
            .put("energia_depois", power(after))
            .put("inicio_da_transcricao", r.text.take(400))
            .put("pipeline", JSONObject(r.pipelineJson))
            .put("falas", JSONArray().apply {
                r.utterances.take(20).forEach { u ->
                    put(JSONObject()
                        .put("inicio", u.startSecs.finite())
                        .put("falante", u.speaker?.toInt())
                        .put("texto", u.text))
                }
            })
            .toString(2)
    }

    private fun writeReport(json: String) {
        runCatching {
            val dir = context.getExternalFilesDir(null) ?: context.filesDir
            File(dir, "spike-report.json").writeText(json)
        }.onFailure { Log.w(TAG, "relatório não gravado", it) }
    }

    private fun installed(file: String) = File(modelsDir, file).isFile

    private fun assetExists(name: String) =
        runCatching { context.assets.open(name).close() }.isSuccess

    private fun readAsset(name: String): String? =
        runCatching { context.assets.open(name).bufferedReader().use { it.readText() } }.getOrNull()

    /** O Rust lê caminhos, não assets: a amostra vai para o cache uma vez. */
    private fun extractSample(): File {
        sampleDir.mkdirs()
        val out = File(sampleDir, "amostra.wav")
        if (!out.isFile || out.length() == 0L) {
            context.assets.open("spike/amostra.wav").use { input ->
                out.outputStream().use { input.copyTo(it) }
            }
        }
        return out
    }

    /** O arquivo escolhido vira uma cópia no cache, com a extensão original. */
    private fun copyToCache(uri: Uri): AudioChoice.Picked {
        val resolver = context.contentResolver
        val name = resolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)
            ?.use { c -> if (c.moveToFirst()) c.getString(0) else null }
            ?: "audio"
        val dir = File(context.cacheDir, "escolhido").apply {
            deleteRecursively()
            mkdirs()
        }
        val out = File(dir, name.replace(Regex("[^\\w.\\- ]"), "_"))
        resolver.openInputStream(uri)?.use { input -> out.outputStream().use { input.copyTo(it) } }
            ?: error("não consegui abrir $name")
        return AudioChoice.Picked(out, name)
    }

    private data class Quad(val path: String, val name: String, val reference: String?, val turns: String?)

    companion object {
        const val TAG = "ISPerSpike"
    }
}

/** JSON não aceita NaN nem infinito. */
private fun Float.finite(): Double? = if (isFinite()) toDouble() else null
