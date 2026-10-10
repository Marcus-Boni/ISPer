package com.isper.mobile.today

import android.content.Context
import com.isper.mobile.core.MobileDay
import com.isper.mobile.core.TaskBook
import com.isper.mobile.sync.PcSync
import java.io.File
import java.time.LocalDate
import java.time.ZonedDateTime
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.asSharedFlow

/**
 * As tarefas no celular (Fase 10.6), para o app inteiro: a cópia do que o PC
 * mandou e a fila do que mudou aqui ficam no núcleo ([TaskBook]), na mesma
 * pasta da sincronia — é a rodada do [PcSync] que leva a fila e traz o
 * retrato novo.
 *
 * Toda mudança feita aqui avisa a tela, refaz os lembretes e pede uma rodada
 * logo, se houver PC pareado.
 */
object Tasks {
    private var book: TaskBook? = null

    private val _changed = MutableSharedFlow<Unit>(extraBufferCapacity = 4)
    /** As tarefas mudaram (aqui ou pela sincronia): a tela relê. */
    val changed: SharedFlow<Unit> = _changed.asSharedFlow()

    @Synchronized
    fun book(context: Context): TaskBook =
        book ?: TaskBook(File(context.applicationContext.filesDir, "sync").apply { mkdirs() }.path)
            .also { book = it }

    /** Hoje, no fuso do aparelho (`AAAA-MM-DD`). */
    fun today(): String = LocalDate.now().toString()

    fun tomorrow(): String = LocalDate.now().plusDays(1).toString()

    /** O fuso do aparelho agora, em minutos (para "feitas hoje"). */
    fun offsetMinutes(): Int = ZonedDateTime.now().offset.totalSeconds / 60

    fun now(): Long = System.currentTimeMillis()

    fun day(context: Context): MobileDay = book(context).day(today(), offsetMinutes())

    /** Uma mudança feita aqui: a tela relê, os lembretes se refazem e o PC fica sabendo. */
    fun changedHere(context: Context) {
        _changed.tryEmit(Unit)
        Reminders.reschedule(context)
        PcSync.syncSoon(context)
    }

    /** O retrato do PC chegou (fim de uma rodada). */
    fun refreshed(context: Context) {
        _changed.tryEmit(Unit)
        Reminders.reschedule(context)
    }
}
