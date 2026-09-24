package com.isper.mobile

import android.app.ActivityManager
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.os.BatteryManager
import android.os.Build
import android.os.PowerManager
import java.io.File

/** O que o relatório registra sobre o aparelho. */
data class DeviceInfo(
    val manufacturer: String,
    val model: String,
    val soc: String?,
    val system: String,
    val abi: String,
    val totalRamMb: Long,
)

/** Bateria, temperatura e estado térmico num instante. */
data class PowerSnapshot(
    val batteryPercent: Int?,
    val temperatureC: Float?,
    val charging: Boolean,
    val thermal: String,
)

object DeviceProbe {
    fun info(context: Context): DeviceInfo {
        val memory = ActivityManager.MemoryInfo()
        context.getSystemService(ActivityManager::class.java).getMemoryInfo(memory)
        val soc = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            "${Build.SOC_MANUFACTURER} ${Build.SOC_MODEL}".trim()
        } else {
            null
        }
        return DeviceInfo(
            manufacturer = Build.MANUFACTURER,
            model = Build.MODEL,
            soc = soc,
            system = "Android ${Build.VERSION.RELEASE} (API ${Build.VERSION.SDK_INT})",
            abi = Build.SUPPORTED_ABIS.firstOrNull() ?: "?",
            totalRamMb = memory.totalMem / (1024 * 1024),
        )
    }

    fun power(context: Context): PowerSnapshot {
        val battery = context.registerReceiver(null, IntentFilter(Intent.ACTION_BATTERY_CHANGED))
        val level = battery?.getIntExtra(BatteryManager.EXTRA_LEVEL, -1) ?: -1
        val scale = battery?.getIntExtra(BatteryManager.EXTRA_SCALE, -1) ?: -1
        val tenths = battery?.getIntExtra(BatteryManager.EXTRA_TEMPERATURE, Int.MIN_VALUE) ?: Int.MIN_VALUE
        val plugged = (battery?.getIntExtra(BatteryManager.EXTRA_PLUGGED, 0) ?: 0) != 0
        return PowerSnapshot(
            batteryPercent = if (level >= 0 && scale > 0) level * 100 / scale else null,
            temperatureC = if (tenths != Int.MIN_VALUE) tenths / 10f else null,
            charging = plugged,
            thermal = thermalName(context.getSystemService(PowerManager::class.java).currentThermalStatus),
        )
    }

    private fun thermalName(status: Int): String = when (status) {
        PowerManager.THERMAL_STATUS_NONE -> "normal"
        PowerManager.THERMAL_STATUS_LIGHT -> "leve"
        PowerManager.THERMAL_STATUS_MODERATE -> "moderado"
        PowerManager.THERMAL_STATUS_SEVERE -> "severo"
        PowerManager.THERMAL_STATUS_CRITICAL -> "crítico"
        PowerManager.THERMAL_STATUS_EMERGENCY -> "emergência"
        PowerManager.THERMAL_STATUS_SHUTDOWN -> "desligando"
        else -> "desconhecido"
    }

    /**
     * A biblioteca arm64 é compilada para ARMv8.2 com dotprod e fp16 (ver o
     * `app/build.gradle.kts`). Num aparelho mais antigo ela morreria com
     * instrução ilegal ao carregar; aqui isso vira uma mensagem. Devolve o que
     * falta — vazio quando está tudo lá, ou quando não deu para ler o
     * `/proc/cpuinfo` (aí vale tentar).
     */
    fun missingCpuFeatures(): List<String> {
        if (Build.SUPPORTED_ABIS.firstOrNull() != "arm64-v8a") return emptyList()
        val features = runCatching { File("/proc/cpuinfo").readLines() }
            .getOrDefault(emptyList())
            .filter { it.startsWith("Features") }
            .flatMap { it.substringAfter(":").trim().split(" ") }
            .toSet()
        if (features.isEmpty()) return emptyList()
        return listOf("asimddp" to "dotprod", "fphp" to "fp16")
            .filter { (flag, _) -> flag !in features }
            .map { (_, name) -> name }
    }
}
