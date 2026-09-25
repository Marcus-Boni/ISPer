package com.isper.mobile.sync

import android.content.Context
import android.os.Build
import android.provider.Settings
import androidx.work.Constraints
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.ExistingWorkPolicy
import androidx.work.NetworkType
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import com.isper.mobile.core.PcInfo
import com.isper.mobile.core.PcLink
import com.isper.mobile.core.SyncSummary
import com.isper.mobile.core.initLogging
import java.io.File
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.withContext

/**
 * A ligação do celular com o PC (Fase 9.3), para o app inteiro: quem está
 * pareado, o envio em andamento e as rodadas de sincronia.
 *
 * O trabalho pesado fica no núcleo ([PcLink]: iroh, pareamento, envio
 * retomável). Aqui ficam as regras do Android: rodar no WorkManager, com
 * rede, tentando de novo sozinho, e avisar a interface.
 */
object PcSync {
    private const val WORK_NOW = "isper-sync"
    private const val WORK_LATER = "isper-sync-depois"
    private const val WORK_PERIODIC = "isper-sync-periodica"

    private var link: PcLink? = null

    private val _pc = MutableStateFlow<PcInfo?>(null)
    /** O PC pareado, ou `null`. */
    val pc: StateFlow<PcInfo?> = _pc.asStateFlow()

    /** O envio em andamento, para a Biblioteca mostrar a porcentagem. */
    data class Sending(val id: String, val fraction: Float)

    private val _sending = MutableStateFlow<Sending?>(null)
    val sending: StateFlow<Sending?> = _sending.asStateFlow()

    private val _lastError = MutableStateFlow<String?>(null)
    /** Por que a última rodada parou (o PC fora de alcance), se parou. */
    val lastError: StateFlow<String?> = _lastError.asStateFlow()

    private val _changed = MutableSharedFlow<Unit>(extraBufferCapacity = 4)
    /** Uma rodada mudou o estado das gravações: a Biblioteca relista. */
    val changed: SharedFlow<Unit> = _changed.asSharedFlow()

    /** Uma rodada por vez: duas mandando o mesmo arquivo só se atrapalhariam. */
    internal val round = Mutex()

    /** Rodadas seguidas sem alcançar o PC: a próxima tentativa espera mais. */
    private val failures = AtomicInteger(0)

    /** A identidade do celular fica na pasta interna do app. */
    @Synchronized
    fun link(context: Context): PcLink =
        link ?: run {
            val dir = File(context.applicationContext.filesDir, "sync").apply { mkdirs() }
            // O log do núcleo (o porquê de uma rodada não ter falado com o PC).
            initLogging(File(dir, "nucleo.log").path)
            PcLink(dir.path)
        }.also {
            link = it
            _pc.value = it.pairedPc()
        }

    /** O nome que o PC mostra: o do aparelho nas Configurações do Android. */
    fun deviceName(context: Context): String =
        Settings.Global.getString(context.contentResolver, Settings.Global.DEVICE_NAME)
            ?.takeIf { it.isNotBlank() }
            ?: "${Build.MANUFACTURER} ${Build.MODEL}".trim()

    fun appVersion(context: Context): String =
        runCatching { context.packageManager.getPackageInfo(context.packageName, 0).versionName }
            .getOrNull() ?: "?"

    /** O nome do PC de um código (do QR ou colado), sem conectar. */
    suspend fun readCode(context: Context, code: String): String =
        withContext(Dispatchers.IO) { link(context).readCode(code) }

    /** Pareia. Espera alguém tocar em "Permitir" no PC. */
    suspend fun pair(context: Context, code: String): PcInfo {
        val pc = withContext(Dispatchers.IO) {
            link(context).pair(code, deviceName(context), appVersion(context))
        }
        _pc.value = pc
        _lastError.value = null
        ensurePeriodic(context)
        syncSoon(context)
        return pc
    }

    /** Desfaz o pareamento (avisa o PC, se ele estiver ao alcance). */
    suspend fun unpair(context: Context) {
        withContext(Dispatchers.IO) { link(context).unpair(deviceName(context), appVersion(context)) }
        forgetLocally(context)
    }

    internal fun forgetLocally(context: Context) {
        _pc.value = null
        _sending.value = null
        val wm = WorkManager.getInstance(context)
        wm.cancelUniqueWork(WORK_PERIODIC)
        wm.cancelUniqueWork(WORK_LATER)
        _changed.tryEmit(Unit)
    }

    private val network = Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build()

    /**
     * Uma rodada logo (ao terminar uma gravação, ao abrir o app, em "Enviar
     * agora"). Com uma rodada rodando, esta entra atrás dela: a gravação que
     * acabou de terminar ficou de fora da lista daquela. Sem nenhuma rodando,
     * esta começa já, no lugar de qualquer uma que estivesse só esperando.
     */
    fun syncSoon(context: Context) {
        if (link(context).pairedPc() == null) return
        val work = OneTimeWorkRequestBuilder<SyncWorker>()
            .setConstraints(network)
            .build()
        val policy = if (round.isLocked) ExistingWorkPolicy.APPEND_OR_REPLACE else ExistingWorkPolicy.REPLACE
        WorkManager.getInstance(context).enqueueUniqueWork(WORK_NOW, policy, work)
    }

    /**
     * O PC não respondeu: tenta de novo em 1, 2, 4, 8 e depois a cada 15 min
     * (a rodada periódica também continua). A espera nunca fica na fila do
     * [syncSoon], senão um "Enviar agora" esperaria atrás dela.
     */
    internal fun retryLater(context: Context) {
        val n = failures.incrementAndGet().coerceAtMost(5)
        syncLater(context, (60L shl (n - 1)).coerceAtMost(15 * 60L))
    }

    internal fun reached() = failures.set(0)

    /** Outra rodada daqui a pouco: o PC ainda está transcrevendo. */
    internal fun syncLater(context: Context, seconds: Long) {
        val work = OneTimeWorkRequestBuilder<SyncWorker>()
            .setConstraints(network)
            .setInitialDelay(seconds, TimeUnit.SECONDS)
            .build()
        WorkManager.getInstance(context)
            .enqueueUniqueWork(WORK_LATER, ExistingWorkPolicy.REPLACE, work)
    }

    /**
     * A rede de segurança: a cada 15 min (o mínimo do Android), com rede.
     * É o que manda sozinho quando o celular volta para perto do PC.
     */
    fun ensurePeriodic(context: Context) {
        if (link(context).pairedPc() == null) return
        val work = PeriodicWorkRequestBuilder<SyncWorker>(15, TimeUnit.MINUTES)
            .setConstraints(network)
            .build()
        WorkManager.getInstance(context)
            .enqueueUniquePeriodicWork(WORK_PERIODIC, ExistingPeriodicWorkPolicy.KEEP, work)
    }

    internal fun progress(id: String, sent: ULong, total: ULong) {
        val f = if (total == 0UL) 0f else (sent.toDouble() / total.toDouble()).toFloat()
        _sending.value = Sending(id, f.coerceIn(0f, 1f))
    }

    internal fun roundFinished(context: Context, summary: SyncSummary?) {
        _sending.value = null
        _pc.value = link(context).pairedPc()
        _lastError.value = summary?.error
        _changed.tryEmit(Unit)
    }
}
