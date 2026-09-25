package com.isper.mobile.library

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.widget.Toast
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import com.isper.mobile.R
import com.isper.mobile.core.RecordingInfo
import java.io.File

/**
 * A ata que voltou do PC — é para isso que 90% do uso do Plaud serve.
 * Mostra o Markdown que o PC escreveu (títulos, citações, listas e o
 * **negrito** dos falantes) e o manda para outro app por "Compartilhar".
 */
@Composable
fun MinutesScreen(info: RecordingInfo, onBack: () -> Unit, modifier: Modifier = Modifier) {
    BackHandler(onBack = onBack)
    val context = LocalContext.current
    val markdown = remember(info.minutesPath) {
        info.minutesPath?.let { runCatching { File(it).readText() }.getOrNull() }.orEmpty()
    }
    val blocks = remember(markdown) { parse(markdown) }
    val title = info.remoteTitle ?: blocks.firstOrNull { it.kind == Kind.H1 }?.text ?: info.id

    LazyColumn(
        modifier = modifier.fillMaxSize(),
        contentPadding = PaddingValues(20.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        item {
            Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                TextButton(onClick = onBack, contentPadding = PaddingValues(0.dp)) {
                    Text("← " + stringResource(R.string.minutes_back))
                }
                Text(title, style = MaterialTheme.typography.headlineSmall, modifier = Modifier.testTag("ata-titulo"))
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Button(onClick = { shareText(context, title, markdown) }, modifier = Modifier.testTag("compartilhar-ata")) {
                        Text(stringResource(R.string.minutes_share))
                    }
                    OutlinedButton(onClick = { copy(context, markdown) }) { Text(stringResource(R.string.minutes_copy)) }
                }
            }
        }
        items(blocks.filterNot { it.kind == Kind.H1 && it.text == title }) { b ->
            when (b.kind) {
                Kind.H1 -> Text(inline(b.text), style = MaterialTheme.typography.titleLarge)
                Kind.H2 -> Text(inline(b.text), style = MaterialTheme.typography.titleMedium, modifier = Modifier.padding(top = 8.dp))
                Kind.H3 -> Text(inline(b.text), style = MaterialTheme.typography.titleSmall)
                Kind.QUOTE -> Text(
                    inline(b.text),
                    style = MaterialTheme.typography.bodySmall,
                    fontStyle = FontStyle.Italic,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Kind.BULLET -> Text(inline("•  " + b.text), style = MaterialTheme.typography.bodyMedium)
                Kind.PARAGRAPH -> Text(inline(b.text), style = MaterialTheme.typography.bodyMedium)
            }
        }
    }
}

private enum class Kind { H1, H2, H3, QUOTE, BULLET, PARAGRAPH }

private data class Block(val kind: Kind, val text: String)

/** Markdown do jeito que o ISPer escreve: uma linha, um bloco. */
private fun parse(md: String): List<Block> = md.lines().mapNotNull { raw ->
    val line = raw.trimEnd()
    when {
        line.isBlank() || line == "---" -> null
        line.startsWith("### ") -> Block(Kind.H3, line.removePrefix("### "))
        line.startsWith("## ") -> Block(Kind.H2, line.removePrefix("## "))
        line.startsWith("# ") -> Block(Kind.H1, line.removePrefix("# "))
        line.startsWith("> ") -> Block(Kind.QUOTE, line.removePrefix("> "))
        line.startsWith("- ") || line.startsWith("* ") -> Block(Kind.BULLET, line.substring(2))
        else -> Block(Kind.PARAGRAPH, line)
    }
}

/** `**negrito**` e `` `código` `` dentro da linha. */
private fun inline(text: String): AnnotatedString = buildAnnotatedString {
    var i = 0
    while (i < text.length) {
        when {
            text.startsWith("**", i) -> {
                val end = text.indexOf("**", i + 2)
                if (end < 0) { append(text.substring(i)); break }
                withStyle(SpanStyle(fontWeight = FontWeight.SemiBold)) { append(text.substring(i + 2, end)) }
                i = end + 2
            }
            text[i] == '`' -> {
                val end = text.indexOf('`', i + 1)
                if (end < 0) { append(text.substring(i)); break }
                append(text.substring(i + 1, end))
                i = end + 1
            }
            else -> { append(text[i]); i++ }
        }
    }
}

private fun shareText(context: Context, title: String, markdown: String) {
    val send = Intent(Intent.ACTION_SEND)
        .setType("text/plain")
        .putExtra(Intent.EXTRA_SUBJECT, title)
        .putExtra(Intent.EXTRA_TEXT, markdown)
    context.startActivity(Intent.createChooser(send, context.getString(R.string.minutes_share_title)))
}

private fun copy(context: Context, markdown: String) {
    context.getSystemService(ClipboardManager::class.java)
        ?.setPrimaryClip(ClipData.newPlainText("ata", markdown))
    Toast.makeText(context, R.string.minutes_copied, Toast.LENGTH_SHORT).show()
}
