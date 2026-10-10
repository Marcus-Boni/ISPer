package com.isper.mobile.today

import android.Manifest
import android.content.pm.PackageManager
import android.widget.Toast
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.Button
import androidx.compose.material3.Checkbox
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.FilledTonalIconButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.isper.mobile.R
import com.isper.mobile.core.MobileDay
import com.isper.mobile.core.MobileRoutine
import com.isper.mobile.core.MobileTask
import com.isper.mobile.sync.PcSync
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.util.Locale

private val ptBr = Locale.forLanguageTag("pt-BR")

/** "hoje", "amanhã" ou "sex., 16/10". */
@Composable
private fun dayLabel(iso: String): String {
    val d = runCatching { LocalDate.parse(iso) }.getOrNull() ?: return iso
    val today = LocalDate.now()
    return when (d) {
        today -> stringResource(R.string.today_when_today)
        today.plusDays(1) -> stringResource(R.string.today_when_tomorrow)
        else -> d.format(DateTimeFormatter.ofPattern("EEE, dd/MM", ptBr))
    }
}

/**
 * A tela Hoje do celular (Fase 10.6): o mesmo dia da tela Hoje do PC, com
 * criar (digitando ou ditando), concluir, adiar e aceitar da caixa de
 * entrada — tudo na hora, com ou sem o PC por perto.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun TodayScreen(vm: TodayViewModel, modifier: Modifier = Modifier) {
    val context = LocalContext.current
    val day by vm.day.collectAsStateWithLifecycle()
    val dictation by vm.dictation.collectAsStateWithLifecycle()
    val pc by PcSync.pc.collectAsStateWithLifecycle()
    var text by rememberSaveable { mutableStateOf("") }
    var fromVoice by rememberSaveable { mutableStateOf(false) }
    var showLater by rememberSaveable { mutableStateOf(false) }

    LaunchedEffect(Unit) {
        vm.notices.collect { Toast.makeText(context, it, Toast.LENGTH_LONG).show() }
    }
    LaunchedEffect(Unit) {
        vm.heard.collect {
            text = it
            fromVoice = true
        }
    }
    val noModel = stringResource(R.string.today_dictation_no_model)
    val askMic = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { ok ->
        if (ok) vm.startDictation()
    }
    val onMic = {
        when {
            dictation is DictationUi.Listening -> vm.stopDictation()
            dictation is DictationUi.Understanding -> Unit
            !vm.canDictate() -> Toast.makeText(context, noModel, Toast.LENGTH_LONG).show()
            ContextCompat.checkSelfPermission(context, Manifest.permission.RECORD_AUDIO) ==
                PackageManager.PERMISSION_GRANTED -> vm.startDictation()
            else -> askMic.launch(Manifest.permission.RECORD_AUDIO)
        }
    }
    val submit = {
        if (text.isNotBlank()) {
            vm.add(text, fromVoice)
            text = ""
            fromVoice = false
        }
    }

    LazyColumn(
        modifier = modifier.fillMaxWidth(),
        contentPadding = androidx.compose.foundation.layout.PaddingValues(horizontal = 18.dp, vertical = 16.dp),
        verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        item {
            Text(
                LocalDate.now().format(DateTimeFormatter.ofPattern("EEEE, d 'de' MMMM", ptBr))
                    .replaceFirstChar { it.titlecase(ptBr) },
                style = MaterialTheme.typography.labelLarge,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Text(stringResource(R.string.tab_today), style = MaterialTheme.typography.headlineMedium)
            SyncLine(day, paired = pc != null)
            Spacer(Modifier.size(10.dp))
        }

        item {
            Row(verticalAlignment = Alignment.CenterVertically) {
                OutlinedTextField(
                    value = text,
                    onValueChange = { text = it },
                    modifier = Modifier.weight(1f).testTag("hoje-campo"),
                    placeholder = { Text(stringResource(R.string.today_add_placeholder)) },
                    singleLine = true,
                    keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done),
                    keyboardActions = KeyboardActions(onDone = { submit() }),
                )
                Spacer(Modifier.width(8.dp))
                FilledTonalIconButton(
                    onClick = onMic,
                    modifier = Modifier.size(52.dp).testTag("hoje-ditar").semantics {
                        contentDescription = context.getString(
                            if (dictation is DictationUi.Listening) R.string.today_dictation_stop else R.string.today_dictation_start,
                        )
                    },
                ) {
                    Icon(painterResource(R.drawable.ic_mic), contentDescription = null)
                }
            }
            when (val d = dictation) {
                is DictationUi.Listening -> Row(
                    Modifier.padding(top = 8.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Box(Modifier.size(10.dp).clip(CircleShape).background(MaterialTheme.colorScheme.primary))
                    Spacer(Modifier.width(8.dp))
                    Text(
                        stringResource(R.string.today_dictation_listening, d.seconds.toInt()),
                        style = MaterialTheme.typography.bodySmall,
                    )
                    Spacer(Modifier.width(10.dp))
                    LinearProgressIndicator(progress = { d.level }, modifier = Modifier.weight(1f))
                }
                DictationUi.Understanding -> Row(
                    Modifier.padding(top = 8.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    CircularProgressIndicator(Modifier.size(16.dp), strokeWidth = 2.dp)
                    Spacer(Modifier.width(8.dp))
                    Text(stringResource(R.string.today_dictation_understanding), style = MaterialTheme.typography.bodySmall)
                }
                DictationUi.Idle -> Unit
            }
            val parsed = remember(text) { vm.parse(text) }
            if (parsed != null && text.isNotBlank()) {
                Row(
                    Modifier.fillMaxWidth().padding(top = 8.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    val whenText = listOfNotNull(
                        parsed.plannedOn?.let { dayLabel(it) },
                        parsed.plannedTime,
                        parsed.dueOn?.let { stringResource(R.string.today_due, dayLabel(it)) },
                    ).joinToString(" · ")
                    Text(
                        if (whenText.isEmpty()) parsed.title else "${parsed.title} → $whenText",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.weight(1f),
                    )
                    Button(onClick = submit, modifier = Modifier.testTag("hoje-adicionar")) {
                        Text(stringResource(R.string.today_add))
                    }
                }
            }
            Spacer(Modifier.size(8.dp))
        }

        val d = day
        if (d == null) return@LazyColumn

        section(R.string.today_sec_planned, d.planned.size)
        if (d.planned.isEmpty()) {
            item { Empty(stringResource(if (d.doneToday.isEmpty()) R.string.today_empty else R.string.today_all_done)) }
        }
        items(d.planned, key = { "p-" + it.id }) { t -> TaskRow(t, vm) }

        if (d.inbox.isNotEmpty()) {
            section(R.string.today_sec_inbox, d.inbox.size)
            items(d.inbox, key = { "i-" + it.id }) { t -> InboxRow(t, vm) }
        }

        if (d.later.isNotEmpty()) {
            item {
                TextButton(onClick = { showLater = !showLater }) {
                    Text(
                        stringResource(if (showLater) R.string.today_hide_later else R.string.today_show_later, d.later.size),
                    )
                }
            }
            if (showLater) items(d.later, key = { "l-" + it.id }) { t -> TaskRow(t, vm) }
        }

        if (d.doneToday.isNotEmpty()) {
            section(R.string.today_sec_done, d.doneToday.size)
            items(d.doneToday, key = { "d-" + it.id }) { t -> TaskRow(t, vm) }
        }

        if (d.routines.isNotEmpty()) {
            section(R.string.today_sec_routines, d.routines.size)
            items(d.routines, key = { "r-" + it.id }) { r -> RoutineRow(r) }
        }
    }
}

private fun androidx.compose.foundation.lazy.LazyListScope.section(title: Int, count: Int) {
    item(key = "s-$title") {
        Row(Modifier.padding(top = 14.dp, bottom = 2.dp), verticalAlignment = Alignment.CenterVertically) {
            Text(
                stringResource(title).uppercase(ptBr),
                style = MaterialTheme.typography.labelMedium,
                color = MaterialTheme.colorScheme.primary,
                fontWeight = FontWeight.SemiBold,
            )
            if (count > 0) {
                Spacer(Modifier.width(8.dp))
                Text("$count", style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }
    }
}

@Composable
private fun SyncLine(day: MobileDay?, paired: Boolean) {
    val text = when {
        !paired -> stringResource(R.string.today_sync_unpaired)
        day == null -> null
        day.pending > 0u -> pluralStringResource(R.plurals.today_pending, day.pending.toInt(), day.pending.toInt())
        day.syncError != null -> stringResource(R.string.today_sync_error, day.syncError!!)
        day.syncedAtMs != null -> stringResource(
            R.string.today_synced_at,
            Instant.ofEpochMilli(day.syncedAtMs!!).atZone(ZoneId.systemDefault())
                .format(DateTimeFormatter.ofPattern("HH:mm")),
        )
        else -> stringResource(R.string.today_sync_never)
    }
    text?.let {
        Text(
            it,
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.testTag("hoje-sincronia"),
        )
    }
    if (day != null && day.keptPc > 0u) {
        Text(
            pluralStringResource(R.plurals.today_kept_pc, day.keptPc.toInt(), day.keptPc.toInt()),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.tertiary,
        )
    }
}

@Composable
private fun Empty(text: String) {
    Text(
        text,
        style = MaterialTheme.typography.bodyMedium,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = Modifier.padding(vertical = 8.dp),
    )
}

@Composable
private fun Chip(text: String, color: Color = MaterialTheme.colorScheme.onSurfaceVariant) {
    Surface(
        shape = RoundedCornerShape(50),
        color = MaterialTheme.colorScheme.surfaceVariant,
        contentColor = color,
    ) {
        Text(text, style = MaterialTheme.typography.labelSmall, modifier = Modifier.padding(horizontal = 8.dp, vertical = 2.dp))
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun TaskMeta(t: MobileTask) {
    val chips = buildList<@Composable () -> Unit> {
        t.plannedTime?.let { add { Chip(it) } }
        if (t.overdue) add { Chip(stringResource(R.string.today_overdue), MaterialTheme.colorScheme.primary) }
        if (t.status == "open" && t.plannedOn != null && t.plannedOn != LocalDate.now().toString() && !t.overdue) {
            add { Chip(dayLabel(t.plannedOn!!)) }
        }
        t.dueOn?.let { add { Chip(stringResource(R.string.today_due, dayLabel(it))) } }
        if (t.fromRoutine) add { Chip(stringResource(R.string.today_chip_routine)) }
        if (t.source == "voice") add { Chip(stringResource(R.string.today_chip_voice)) }
        if (t.source == "meeting" || t.source == "copilot") add { Chip(stringResource(R.string.today_chip_meeting)) }
        if (t.pending) add { Chip(stringResource(R.string.today_chip_pending), MaterialTheme.colorScheme.tertiary) }
    }
    if (chips.isNotEmpty()) {
        FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            chips.forEach { it() }
        }
    }
}

@Composable
private fun TaskRow(t: MobileTask, vm: TodayViewModel) {
    val done = t.status == "done"
    var menu by remember { mutableStateOf(false) }
    Column {
        Row(
            // As etiquetas levam o título: o e2e (android-tasks.ps1) acha a tarefa por ele.
            Modifier.fillMaxWidth().testTag("tarefa:${t.title}"),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Checkbox(
                checked = done,
                onCheckedChange = { if (it) vm.complete(t.id) else vm.reopen(t.id) },
                modifier = Modifier.testTag("concluir:${t.title}"),
            )
            Column(Modifier.weight(1f).padding(vertical = 6.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Text(
                    t.title,
                    style = MaterialTheme.typography.bodyLarge,
                    textDecoration = if (done) TextDecoration.LineThrough else null,
                    color = if (done) MaterialTheme.colorScheme.onSurfaceVariant else MaterialTheme.colorScheme.onSurface,
                )
                TaskMeta(t)
            }
            if (!done) {
                Box {
                    IconButton(onClick = { menu = true }) {
                        Icon(painterResource(R.drawable.ic_more), contentDescription = stringResource(R.string.today_more, t.title))
                    }
                    DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                        if (t.plannedOn != Tasks.today()) {
                            DropdownMenuItem(text = { Text(stringResource(R.string.today_move_today)) }, onClick = { menu = false; vm.moveTo(t.id, Tasks.today()) })
                        }
                        DropdownMenuItem(text = { Text(stringResource(R.string.today_move_tomorrow)) }, onClick = { menu = false; vm.moveTo(t.id, Tasks.tomorrow()) })
                        DropdownMenuItem(text = { Text(stringResource(R.string.today_move_someday)) }, onClick = { menu = false; vm.moveTo(t.id, null) })
                        DropdownMenuItem(text = { Text(stringResource(R.string.today_drop)) }, onClick = { menu = false; vm.drop(t.id) })
                    }
                }
            }
        }
        HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
    }
}

@Composable
private fun InboxRow(t: MobileTask, vm: TodayViewModel) {
    Column(Modifier.padding(vertical = 6.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Text(t.title, style = MaterialTheme.typography.bodyLarge)
        TaskMeta(t)
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            OutlinedButton(onClick = { vm.moveTo(t.id, Tasks.today()) }) { Text(stringResource(R.string.today_accept_today)) }
            OutlinedButton(onClick = { vm.moveTo(t.id, Tasks.tomorrow()) }) { Text(stringResource(R.string.today_accept_tomorrow)) }
            TextButton(onClick = { vm.drop(t.id) }) { Text(stringResource(R.string.today_drop)) }
        }
        HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
    }
}

private val weekdayNames = mapOf(
    "MO" to "seg", "TU" to "ter", "WE" to "qua", "TH" to "qui", "FR" to "sex", "SA" to "sáb", "SU" to "dom",
)

@Composable
private fun RoutineRow(r: MobileRoutine) {
    val days = r.weekdays.map { it.takeLast(2) }
    val schedule = when {
        r.freq == "daily" -> stringResource(R.string.today_routine_daily)
        r.freq == "weekly" && days == listOf("MO", "TU", "WE", "TH", "FR") -> stringResource(R.string.today_routine_workdays)
        r.freq == "weekly" -> days.joinToString(", ") { weekdayNames[it] ?: it }
        else -> stringResource(R.string.today_routine_monthly)
    } + (r.time?.let { " · $it" } ?: "")
    Row(Modifier.fillMaxWidth().padding(vertical = 6.dp), verticalAlignment = Alignment.CenterVertically) {
        Column(Modifier.weight(1f)) {
            Text(
                r.title,
                style = MaterialTheme.typography.bodyLarge,
                color = if (r.active) MaterialTheme.colorScheme.onSurface else MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Text(schedule, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        if (!r.active) Chip(stringResource(R.string.today_routine_paused))
    }
}
