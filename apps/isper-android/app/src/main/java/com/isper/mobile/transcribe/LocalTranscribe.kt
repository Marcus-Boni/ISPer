package com.isper.mobile.transcribe

import android.content.Context
import androidx.work.Constraints
import androidx.work.ExistingWorkPolicy
import androidx.work.NetworkType
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.WorkManager
import com.isper.mobile.DeviceProbe
import com.isper.mobile.core.DevicePlan
import com.isper.mobile.core.devicePlan
import com.isper.mobile.core.diarizeModelsInstalled
import com.isper.mobile.core.vadFileName
import java.io.File
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow

/** Quando o celular transcreve sozinho (Fase 9.4). */
enum class TranscribeMode(val key: String) {
    /** Só na tomada: o padrão. A transcrição usa a CPU por um bom tempo. */
    CHARGING("carregando"),
    /** Assim que a gravação termina, na bateria mesmo. */
    NOW("agora"),
    /** Nunca: quem transcreve é o PC pareado. */
    PC_ONLY("so-pc");

    companion object {
        fun of(key: String?): TranscribeMode = entries.firstOrNull { it.key == key } ?: CHARGING
    }
}

/** As escolhas de quem usa, sobre o plano do aparelho. */
data class LocalSetup(
    val mode: TranscribeMode,
    /** O que o aparelho aguenta, pela memória ([devicePlan]). */
    val plan: DevicePlan,
    /** O modelo escolhido à mão; `null` segue o plano. */
    val modelChoice: String?,
    /** Separar os falantes escolhido à mão; `null` segue o plano. */
    val diarizeChoice: Boolean?,
) {
    val modelFile: String get() = modelChoice ?: plan.modelFile
    val diarize: Boolean get() = diarizeChoice ?: plan.diarize

    /** O celular transcreve: o aparelho aguenta (ou quem usa escolheu um modelo) e o modo deixa. */
    val enabled: Boolean get() = mode != TranscribeMode.PC_ONLY && (plan.onDevice || modelChoice != null)
}

/**
 * A transcrição no próprio celular, para o app inteiro: as escolhas, o
 * agendamento no WorkManager e o andamento para a Biblioteca.
 *
 * O trabalho pesado fica no núcleo ([com.isper.mobile.core.LocalTranscription]:
 * o passe final do PC, que continua de onde parou). Aqui ficam as regras do
 * Android: primeiro baixar os modelos ([ModelsWorker]), depois transcrever
 * ([TranscribeWorker]), por padrão só com o aparelho na tomada.
 */
object LocalTranscribe {
    private const val PREFS = "transcricao"
    private const val KEY_MODE = "modo"
    private const val KEY_MODEL = "modelo"
    private const val KEY_DIARIZE = "falantes"
    const val WORK = "isper-transcrever"

    /** A transcrição em andamento, para a Biblioteca mostrar a porcentagem. */
    data class Running(val id: String, val fraction: Float, val stage: String)

    private val _running = MutableStateFlow<Running?>(null)
    val running: StateFlow<Running?> = _running.asStateFlow()

    private val _changed = MutableSharedFlow<Unit>(extraBufferCapacity = 4)
    /** Uma ata ficou pronta (ou uma falha foi anotada): a Biblioteca relista. */
    val changed: SharedFlow<Unit> = _changed.asSharedFlow()

    fun modelsDir(context: Context): File = File(context.filesDir, "models").apply { mkdirs() }
    fun diarizeDir(context: Context): File = File(modelsDir(context), "diarize").apply { mkdirs() }
    fun vadFile(context: Context): File = File(modelsDir(context), vadFileName())
    fun modelFile(context: Context, file: String): File = File(modelsDir(context), file)

    fun setup(context: Context): LocalSetup {
        val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        return LocalSetup(
            mode = TranscribeMode.of(prefs.getString(KEY_MODE, null)),
            plan = devicePlan(DeviceProbe.info(context).totalRamMb.toULong()),
            modelChoice = prefs.getString(KEY_MODEL, null),
            diarizeChoice = if (prefs.contains(KEY_DIARIZE)) prefs.getBoolean(KEY_DIARIZE, false) else null,
        )
    }

    fun setMode(context: Context, mode: TranscribeMode) {
        edit(context) { putString(KEY_MODE, mode.key) }
        schedule(context, replace = true)
    }

    /** `null` volta ao modelo do plano. */
    fun setModel(context: Context, file: String?) {
        edit(context) { if (file == null) remove(KEY_MODEL) else putString(KEY_MODEL, file) }
        schedule(context, replace = true)
    }

    /** `null` volta ao plano. */
    fun setDiarize(context: Context, on: Boolean?) {
        edit(context) { if (on == null) remove(KEY_DIARIZE) else putBoolean(KEY_DIARIZE, on) }
        schedule(context, replace = true)
    }

    private fun edit(context: Context, block: android.content.SharedPreferences.Editor.() -> Unit) {
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().apply(block).apply()
    }

    /** Os modelos que a transcrição com [setup] precisa estão no aparelho. */
    fun modelsReady(context: Context, setup: LocalSetup = setup(context)): Boolean =
        modelFile(context, setup.modelFile).isFile && vadFile(context).isFile &&
            (!setup.diarize || diarizeModelsInstalled(diarizeDir(context).path))

    /** Quanto os modelos ocupam no aparelho, em bytes. */
    fun modelsBytes(context: Context): Long =
        modelsDir(context).walkTopDown().filter { it.isFile }.sumOf { it.length() }

    /** Apaga os modelos (o laboratório usa a mesma pasta). Voltam no próximo download. */
    fun deleteModels(context: Context) {
        WorkManager.getInstance(context).cancelUniqueWork(WORK)
        modelsDir(context).deleteRecursively()
        _changed.tryEmit(Unit)
    }

    /**
     * Agenda a transcrição do que falta. `now`: é para já, na bateria mesmo
     * ("Transcrever agora"). `replace`: as escolhas mudaram, e o que estava na
     * fila com as regras antigas sai (o que já foi transcrito fica guardado).
     * Sem nenhum dos dois, mantém o que já está na fila.
     */
    fun schedule(context: Context, now: Boolean = false, replace: Boolean = now, anyNetwork: Boolean = now) {
        val setup = setup(context)
        val wm = WorkManager.getInstance(context)
        if (!now && !setup.enabled) {
            wm.cancelUniqueWork(WORK)
            return
        }
        val charging = !now && setup.mode == TranscribeMode.CHARGING
        val models = modelsWork(anyNetwork)
        val transcribe = OneTimeWorkRequestBuilder<TranscribeWorker>()
            .setConstraints(
                Constraints.Builder()
                    .setRequiresCharging(charging)
                    .setRequiresStorageNotLow(true)
                    .build(),
            )
            .build()
        val policy = if (replace) ExistingWorkPolicy.REPLACE else ExistingWorkPolicy.KEEP
        if (modelsReady(context, setup)) {
            wm.enqueueUniqueWork(WORK, policy, transcribe)
        } else {
            wm.beginUniqueWork(WORK, policy, models).then(transcribe).enqueue()
        }
    }

    /**
     * "Baixar agora" dos Ajustes: os modelos vêm já, com qualquer rede, e a
     * transcrição segue as regras de sempre depois. Na mesma fila da
     * transcrição, para dois downloads do mesmo arquivo nunca correrem juntos.
     */
    fun downloadNow(context: Context) {
        if (setup(context).enabled) {
            schedule(context, replace = true, anyNetwork = true)
        } else {
            WorkManager.getInstance(context).enqueueUniqueWork(WORK, ExistingWorkPolicy.REPLACE, modelsWork(anyNetwork = true))
        }
    }

    /** O download (~230 MB na primeira vez) espera o Wi-Fi, a não ser que quem usa tenha pedido para já. */
    private fun modelsWork(anyNetwork: Boolean) = OneTimeWorkRequestBuilder<ModelsWorker>()
        .setConstraints(
            Constraints.Builder()
                .setRequiredNetworkType(if (anyNetwork) NetworkType.CONNECTED else NetworkType.UNMETERED)
                .build(),
        )
        .build()

    internal fun progress(id: String, fraction: Float, stage: String) {
        _running.value = Running(id, fraction.coerceIn(0f, 1f), stage)
    }

    internal fun finished() {
        _running.value = null
        _changed.tryEmit(Unit)
    }

    internal fun notifyChanged() {
        _changed.tryEmit(Unit)
    }
}
