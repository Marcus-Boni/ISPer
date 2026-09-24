package com.isper.mobile.recording

import android.content.Context
import java.io.File
import java.time.OffsetDateTime
import java.time.format.DateTimeFormatter
import java.time.temporal.ChronoUnit

/** Onde as gravações moram e como se chamam. */
object Storage {
    /**
     * `Android/data/com.isper.mobile/files/Gravacoes`: é do app (nenhum outro
     * app lê) e sai com ele na desinstalação, a menos que o usuário peça para
     * manter os dados (`hasFragileUserData`). Sem armazenamento externo, a
     * pasta interna.
     */
    fun recordingsDir(context: Context): File =
        (context.getExternalFilesDir("Gravacoes") ?: File(context.filesDir, "Gravacoes"))
            .apply { mkdirs() }

    private val idFormat = DateTimeFormatter.ofPattern("yyyyMMdd-HHmmss")

    /** Nome base de uma gravação que começa agora, e o início em RFC 3339. */
    fun newId(now: OffsetDateTime = OffsetDateTime.now()): Pair<String, String> {
        val t = now.truncatedTo(ChronoUnit.SECONDS)
        return t.format(idFormat) to t.format(DateTimeFormatter.ISO_OFFSET_DATE_TIME)
    }
}
