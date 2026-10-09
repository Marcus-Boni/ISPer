# E2E da tela Hoje (Fase 10, etapa 10.0) pela interface real, via CDP: a
# barra lateral mostra Hoje com o numero de tarefas; criar, concluir, mover
# para amanha, descartar e aceitar da caixa de entrada mudam a lista na hora;
# o Desfazer do aviso e o Ctrl+Z voltam cada mudanca; a paleta (Ctrl+K) cria
# uma tarefa a partir do texto digitado; editar o titulo grava; e tudo
# continua la depois de reabrir o app. O perfil de teste nasce vazio.
#
#   .\tools\e2e\today.ps1 -Exe <caminho do exe>
#
# ASCII puro de proposito: o PowerShell 5.1 le .ps1 sem BOM como ANSI.
param([string]$Exe)
. "$PSScriptRoot\common.ps1"

$exe = Get-IsperExe $Exe
"today: $exe"

# Estado da tela numa leitura so.
$state = 'JSON.stringify({ planned: [...document.querySelectorAll("#list-planned .task .title")].map(e => e.textContent), inbox: document.getElementById("sec-inbox").hidden ? 0 : document.querySelectorAll("#list-inbox .task").length, later: document.getElementById("sec-later").hidden ? 0 : document.querySelectorAll("#list-later .task").length, done: document.getElementById("sec-done").hidden ? 0 : document.querySelectorAll("#list-done .task").length, empty: !!document.querySelector("#list-planned .empty"), toast: (document.querySelector(".toast.has-action .toast-act") || {}).textContent || null })'

function Wait-Today {
  # Le ate a condicao valer ou o prazo acabar; devolve a ultima leitura.
  param([Parameter(Mandatory)][scriptblock]$Ok, [int]$Seconds = 8)
  $sw = [Diagnostics.Stopwatch]::StartNew()
  do {
    $s = EvJson 'today.html' $state
    if ($s -and (& $Ok $s)) { return $s }
    Start-Sleep -Milliseconds 200
  } while ($sw.Elapsed.TotalSeconds -lt $Seconds)
  $s
}

function Click-Today([string]$Selector) {
  EvJson 'today.html' "JSON.stringify((() => { const n = document.querySelector('$Selector'); if (!n) return false; n.click(); return true; })())"
}

function Press-CtrlZ {
  EvJson 'today.html' 'JSON.stringify((() => { document.body.dispatchEvent(new KeyboardEvent("keydown", { key: "z", ctrlKey: true, bubbles: true })); return true; })())' | Out-Null
}

Stop-Isper
Check (Start-Isper -Exe $exe) "app abriu com a tela Inicio (CDP)"
# O roteiro confere rotulos em portugues: fixa o pt-BR (o runner do CI e en-US).
Invoke-Isper 'set_ui_lang' "{ lang: 'pt-BR' }" | Out-Null

$nav = EvJson 'app.html' 'JSON.stringify({ item: !!document.querySelector(".nav-item[data-view=today]"), label: (document.querySelector(".nav-item[data-view=today] .label") || {}).textContent || null, badge: document.getElementById("badge-today").hidden })'
Check ($nav.item -and $nav.label -eq 'Hoje') "barra lateral tem o item '$($nav.label)'"
Check ($nav.badge) "sem tarefas, o numero ao lado de Hoje fica escondido"

Invoke-Isper 'open_today_window' | Out-Null
Check (Wait-IsperWindow 'today.html') "a tela Hoje abre na janela principal"
$s0 = Wait-Today { param($s) $s.empty }
Check ($s0.empty) "dia vazio mostra o estado vazio"

# 1) Criar pelo campo da tela.
EvJson 'today.html' 'JSON.stringify((() => { const i = document.getElementById("add-title"); i.value = "Revisar o PR 482"; document.getElementById("add").requestSubmit(); return true; })())' | Out-Null
$s1 = Wait-Today { param($s) @($s.planned).Count -eq 1 }
Check (@($s1.planned).Count -eq 1 -and $s1.planned[0] -eq 'Revisar o PR 482') "tarefa nova aparece em Para hoje ($(@($s1.planned) -join ', '))"
Start-Sleep -Milliseconds 600
$badge = EvJson 'app.html' 'JSON.stringify({ hidden: document.getElementById("badge-today").hidden, text: document.getElementById("badge-today").textContent })'
Check (-not $badge.hidden -and $badge.text -eq '1') "o numero ao lado de Hoje vira $($badge.text)"

# 2) Concluir -> sai de Para hoje e vai para Feito hoje, com Desfazer.
Click-Today '#list-planned .task .check' | Out-Null
$s2 = Wait-Today { param($s) $s.done -eq 1 -and $s.toast }
Check ($s2.done -eq 1 -and @($s2.planned).Count -eq 0) "concluir leva a tarefa para Feito hoje"
Check ($s2.toast -eq 'Desfazer') "aviso com o botao '$($s2.toast)'"
EvJson 'today.html' 'JSON.stringify((() => { document.querySelector(".toast.has-action .toast-act").click(); return true; })())' | Out-Null
$s3 = Wait-Today { param($s) @($s.planned).Count -eq 1 -and $s.done -eq 0 }
Check (@($s3.planned).Count -eq 1 -and $s3.done -eq 0) "Desfazer devolve a tarefa para Para hoje"

# 3) Mover para amanha -> vai para Depois; Ctrl+Z traz de volta.
Click-Today '#list-planned .task .acts .btn' | Out-Null
$s4 = Wait-Today { param($s) $s.later -eq 1 -and $s.toast }
Check ($s4.later -eq 1 -and @($s4.planned).Count -eq 0) "Amanha leva a tarefa para Depois"
Press-CtrlZ
$s5 = Wait-Today { param($s) @($s.planned).Count -eq 1 -and $s.later -eq 0 }
Check (@($s5.planned).Count -eq 1 -and $s5.later -eq 0) "Ctrl+Z traz a tarefa de volta para hoje"

# 4) Descartar -> some da lista; Ctrl+Z traz de volta (nada e apagado).
Click-Today '#list-planned .task .acts .btn-icon:last-child' | Out-Null
$s6 = Wait-Today { param($s) @($s.planned).Count -eq 0 -and $s.toast }
Check (@($s6.planned).Count -eq 0) "Descartar tira a tarefa da lista"
Press-CtrlZ
$s7 = Wait-Today { param($s) @($s.planned).Count -eq 1 }
Check (@($s7.planned).Count -eq 1) "Ctrl+Z desfaz o descarte"

# 5) Caixa de entrada: uma acao de reuniao chega e e aceita para hoje.
Invoke-Isper 'task_add' "{ task: { title: 'Mandar o link da gravacao', status: 'inbox', source_kind: 'meeting' } }" 'today.html' | Out-Null
$s8 = Wait-Today { param($s) $s.inbox -eq 1 }
Check ($s8.inbox -eq 1) "tarefa vinda de reuniao cai na Caixa de entrada"
$src = EvJson 'today.html' 'JSON.stringify((document.querySelector("#list-inbox .chip-info") || {}).textContent || null)'
Check ($src -eq 'Reuniao' -or $src -eq ([char[]]@(82,101,117,110,105,227,111) -join '')) "a origem aparece no selo ('$src')"
Click-Today '#list-inbox .task .acts .btn' | Out-Null
$s9 = Wait-Today { param($s) $s.inbox -eq 0 -and @($s.planned).Count -eq 2 }
Check ($s9.inbox -eq 0 -and @($s9.planned).Count -eq 2) "Para hoje aceita a tarefa ($(@($s9.planned).Count) para hoje)"

# 6) Paleta: o texto digitado vira tarefa para hoje.
$pal = EvJson 'app.html' '(async () => { document.dispatchEvent(new KeyboardEvent("keydown", { key: "k", code: "KeyK", ctrlKey: true, bubbles: true })); await new Promise(r => setTimeout(r, 250)); const i = document.getElementById("pal-input"); i.value = "Ligar pro contador"; i.dispatchEvent(new Event("input")); await new Promise(r => setTimeout(r, 150)); const items = [...document.querySelectorAll(".pal-item .pi-text")].map(e => e.textContent); const k = items.findIndex(x => x.startsWith("Criar tarefa")); if (k >= 0) document.querySelectorAll(".pal-item")[k].click(); return JSON.stringify({ items, k }); })()'
Check ($null -ne $pal -and $pal.k -ge 0) "a paleta oferece criar a tarefa ('$(@($pal.items)[$pal.k])')"
$s10 = Wait-Today { param($s) @($s.planned).Count -eq 3 }
Check (@($s10.planned) -contains 'Ligar pro contador') "a tarefa da paleta aparece em Para hoje"

# 6b) Data no texto: a previa mostra o titulo limpo, e a tarefa vai para o dia dito.
$pv = EvJson 'today.html' '(async () => { const i = document.getElementById("add-title"); i.value = "sexta as 10h revisar o contrato"; i.dispatchEvent(new Event("input")); await new Promise(r => setTimeout(r, 700)); const b = document.getElementById("add-preview"); return JSON.stringify({ hidden: b.hidden, text: b.textContent }); })()'
Check ($null -ne $pv -and -not $pv.hidden -and $pv.text -like '*Revisar o contrato*') "a previa tira a data do titulo ('$($pv.text)')"
EvJson 'today.html' 'JSON.stringify((() => { document.getElementById("add").requestSubmit(); return true; })())' | Out-Null
$s10b = Wait-Today { param($s) $s.later -eq 1 }
Check ($s10b.later -eq 1 -and -not (@($s10b.planned) -contains 'Revisar o contrato')) "a tarefa com data vai para Depois, no dia dito"
$lt = EvJson 'today.html' 'JSON.stringify([...document.querySelectorAll("#list-later .task")].map(li => ({ title: li.querySelector(".title").textContent, chips: [...li.querySelectorAll(".chip")].map(c => c.textContent) })))'
Check ((@($lt)[0].title -eq 'Revisar o contrato') -and (@(@($lt)[0].chips) -contains '10:00')) "titulo sem a data e a hora no selo ($(@(@($lt)[0].chips) -join ', '))"

# 7) Editar o titulo (duplo clique, Enter grava).
EvJson 'today.html' 'JSON.stringify((() => { const n = [...document.querySelectorAll("#list-planned .task .title")].find(e => e.textContent === "Ligar pro contador"); n.dispatchEvent(new MouseEvent("dblclick", { bubbles: true })); const i = document.querySelector("#list-planned .edit"); i.value = "Ligar pro contador sobre o IR"; i.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true })); return true; })())' | Out-Null
$s11 = Wait-Today { param($s) @($s.planned) -contains 'Ligar pro contador sobre o IR' }
Check (@($s11.planned) -contains 'Ligar pro contador sobre o IR') "editar o titulo grava"

$errs = Get-JsErrors 'today.html'
Check (@($errs).Count -eq 0) "tela Hoje sem erros de JS$(Format-JsErrors $errs)"
$shellErrs = Get-JsErrors 'app.html'
Check (@($shellErrs).Count -eq 0) "janela principal sem erros de JS$(Format-JsErrors $shellErrs)"

# 8) Reabrir: as tarefas continuam no banco do perfil.
Stop-Isper
Check (Start-Isper -Exe $exe) "app reabriu"
$day = Invoke-Isper 'today_load'
Check (@($day.planned).Count -eq 3) "depois de reabrir, as 3 tarefas continuam para hoje ($(@($day.planned).Count))"

Restart-IsperClean -Exe $exe
Check (-not (Test-Cdp)) "relancado sem porta CDP"
Finish-E2E 'today'
