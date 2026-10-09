# E2E do aprendizado (Fase 10.5) pela janela real, via CDP: a tarefa que se
# repete no mesmo dia da semana vira o cartao "Parece uma rotina" no Hoje, com
# os dias como evidencia; "Agora nao" cala a sugestao e "Criar rotina" cria a
# rotina (com o selo "sugerida pelo ISPer" e a tarefa de hoje). A memoria nas
# Configuracoes: guardar sem repetir, corrigir, fixar, arquivar e o Desfazer.
# O assistente propoe guardar uma preferencia, guarda so depois do toque, e a
# memoria vai nas instrucoes da IA (de mentira, compativel com OpenAI).
#
#   .\tools\e2e\learning.ps1 -Exe <caminho do exe>
#
# Nada sai do PC: a IA e um processo node local, encerrado no fim.
#
# ASCII puro de proposito: o PowerShell 5.1 le .ps1 sem BOM como ANSI.
param([string]$Exe)
. "$PSScriptRoot\common.ps1"

$exe = Get-IsperExe $Exe
"learning: $exe"

function Get-FreePort {
  $l = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
  $l.Start()
  $p = $l.LocalEndpoint.Port
  $l.Stop()
  $p
}
# O Invoke-RestMethod do PowerShell 5.1 devolve o array JSON como um objeto so.
function Get-List([string]$Url) {
  $list = Invoke-RestMethod $Url -TimeoutSec 5
  @($list | ForEach-Object { $_ })
}

$llmPort = Get-FreePort
$llmBase = "http://127.0.0.1:$llmPort/v1"
$fakeLlm = Start-Process node -ArgumentList @("`"$PSScriptRoot\fake-llm.mjs`"", "$llmPort") -PassThru -WindowStyle Hidden
$up = $false
for ($i = 0; $i -lt 40 -and -not $up; $i++) {
  try { Get-List "http://127.0.0.1:$llmPort/requests" | Out-Null; $up = $true } catch { Start-Sleep -Milliseconds 250 }
}
Check $up "a IA de mentira responde ($llmBase)"

$todayJs = 'JSON.stringify((() => ({ shown: !document.getElementById("sec-suggest").hidden, cards: [...document.querySelectorAll("#list-suggest .sug")].map(li => ({ key: li.dataset.key, title: li.querySelector(".sug-title").textContent, why: li.querySelector(".sug-why").textContent, days: li.querySelectorAll(".sug-days .chip").length, buttons: [...li.querySelectorAll("button")].map(b => b.textContent) })), planned: [...document.querySelectorAll("#list-planned > li .title")].map(x => x.textContent), routines: [...document.querySelectorAll("#list-routines > li")].map(li => li.textContent) }))())'
function Get-Today { EvJson 'today.html' $todayJs }
function Wait-Today {
  param([scriptblock]$Ok, [int]$Seconds = 15)
  $sw = [Diagnostics.Stopwatch]::StartNew()
  do {
    $s = Get-Today
    if ($s -and (& $Ok $s)) { return $s }
    Start-Sleep -Milliseconds 300
  } while ($sw.Elapsed.TotalSeconds -lt $Seconds)
  $s
}
function Click-Card([string]$Title, [string]$Prefix) {
  EvJson 'today.html' ('JSON.stringify((() => { const li = [...document.querySelectorAll("#list-suggest .sug")].find(x => x.querySelector(".sug-title").textContent === "' + $Title + '"); if (!li) return false; const b = [...li.querySelectorAll("button")].find(x => x.textContent.startsWith("' + $Prefix + '")); b.click(); return true; })())')
}
$memJs = 'JSON.stringify((() => ({ rows: [...document.querySelectorAll("#memlist .mem")].map(r => ({ id: r.dataset.id, text: (r.querySelector(".mem-text") || r.querySelector(".mem-edit") || {}).textContent, chips: [...r.querySelectorAll(".mem-meta .chip")].map(c => c.textContent), buttons: [...r.querySelectorAll(".mem-acts button")].map(b => b.textContent) })), empty: !document.getElementById("memempty").hidden, archived: document.getElementById("memarchbox").hidden ? null : document.getElementById("memarchsum").textContent }))())'
function Get-Mem { EvJson 'settings.html' $memJs }
function Wait-Mem {
  param([scriptblock]$Ok, [int]$Seconds = 10)
  $sw = [Diagnostics.Stopwatch]::StartNew()
  do {
    $s = Get-Mem
    if ($s -and (& $Ok $s)) { return $s }
    Start-Sleep -Milliseconds 250
  } while ($sw.Elapsed.TotalSeconds -lt $Seconds)
  $s
}
function Add-Mem([string]$Text) {
  $q = ConvertTo-Json $Text
  EvJson 'settings.html' ('JSON.stringify((() => { document.getElementById("memtext").value = ' + $q + '; document.getElementById("memadd").requestSubmit(); return true; })())') | Out-Null
}
function Click-Mem([string]$Like, [string]$Label) {
  EvJson 'settings.html' ('JSON.stringify((() => { const r = [...document.querySelectorAll("#memlist .mem, #memarch .mem")].find(x => x.querySelector(".mem-text").textContent.startsWith("' + $Like + '")); if (!r) return false; [...r.querySelectorAll(".mem-acts button")].find(b => b.textContent === "' + $Label + '").click(); return true; })())')
}

$today = Get-Date
$day = { param([int]$back) $today.AddDays(-$back).ToString('yyyy-MM-dd') }
$llmToml = $null
try {
  Stop-Isper
  Check (Start-Isper -Exe $exe) "app abriu com a tela Inicio (CDP)"
  Invoke-Isper 'set_ui_lang' "{ lang: 'pt-BR' }" | Out-Null
  $llmToml = Get-E2EDataPath 'llm.toml'

  # 1) O que se repete: duas tarefas em 3 das ultimas 4 semanas, no dia da
  #    semana de hoje (uma com hora), e uma que nao se repete.
  foreach ($w in 1, 2, 3) {
    $a = Invoke-Isper 'task_add' "{ task: { title: 'Revisar os PRs abertos', planned_on: '$(& $day (7 * $w))', planned_time: '09:00' } }"
    Invoke-Isper 'task_set_status' "{ id: '$($a.task.id)', status: 'done' }" | Out-Null
    Invoke-Isper 'task_add' "{ task: { title: 'Conferir o backlog', planned_on: '$(& $day (7 * $w))' } }" | Out-Null
  }
  Invoke-Isper 'task_add' "{ task: { title: 'Pagar o boleto', planned_on: '$(& $day 7)' } }" | Out-Null
  Invoke-Isper 'open_today_window' | Out-Null
  Wait-IsperWindow 'today.html' | Out-Null
  $s1 = Wait-Today { param($s) $s.shown -and @($s.cards).Count -ge 2 }
  Check ($s1.shown -and @($s1.cards).Count -eq 2) "o Hoje mostra 2 rotinas sugeridas ($(@($s1.cards).Count))"
  $prs = @($s1.cards) | Where-Object { $_.title -eq 'Revisar os PRs abertos' }
  Check ($prs.why -like '3 de 4 vezes*' -and $prs.why -like '*09:00*' -and $prs.days -eq 3) "com a evidencia: 3 de 4, a hora e os 3 dias ('$($prs.why)')"
  Check ((@($prs.buttons) -contains 'Criar rotina') -and (@($prs.buttons) | Where-Object { $_ -like 'Agora n*' }).Count -eq 1) "e os botoes Criar rotina e Agora nao"
  Check ((@($s1.cards) | Where-Object { $_.title -like 'Pagar*' }).Count -eq 0) "o que aconteceu uma vez so nao vira sugestao"
  Check (@($s1.routines).Count -eq 0) "nada virou rotina sozinho"

  # 2) Agora nao: some e nao volta.
  Click-Card 'Conferir o backlog' 'Agora n' | Out-Null
  $s2 = Wait-Today { param($s) @($s.cards).Count -eq 1 }
  Check (@($s2.cards).Count -eq 1 -and @($s2.cards)[0].title -eq 'Revisar os PRs abertos') "recusada, a sugestao some"
  $v = Invoke-Isper 'today_load'
  Check (-not (@($v.suggestions) | Where-Object { $_.title -eq 'Conferir o backlog' })) "e nao volta ao reler o dia"
  Check (@($v.routines).Count -eq 0) "recusar nao cria rotina"

  # 3) Criar rotina: a rotina com o selo, e a tarefa de hoje.
  Click-Card 'Revisar os PRs abertos' 'Criar rotina' | Out-Null
  $s3 = Wait-Today { param($s) -not $s.shown -and @($s.routines).Count -ge 1 }
  Check (-not $s3.shown) "sem sugestoes, a secao some"
  Check ((@($s3.routines) | Where-Object { $_ -like '*Revisar os PRs abertos*' -and $_ -like '*sugerida pelo ISPer*' }).Count -eq 1) "a rotina aparece com o selo 'sugerida pelo ISPer'"
  $v = Invoke-Isper 'today_load'
  $r = @($v.routines) | Where-Object { $_.title -eq 'Revisar os PRs abertos' }
  Check ($r.learned_from.hits -eq 3 -and @($r.learned_from.days).Count -eq 3 -and $r.rrule -like 'FREQ=WEEKLY;BYDAY=*;BYHOUR=9;BYMINUTE=0') "com a evidencia guardada ($($r.rrule))"
  Check ((@($s3.planned) | Where-Object { $_ -eq 'Revisar os PRs abertos' }).Count -eq 1) "e a tarefa de hoje ja aparece em Para hoje"

  # 4) A memoria nas Configuracoes.
  Invoke-Isper 'open_settings_window' "{ section: 'memoria' }" | Out-Null
  Wait-IsperWindow 'settings.html' | Out-Null
  $m0 = Wait-Mem { param($s) $s.empty }
  Check ($m0.empty -and @($m0.rows).Count -eq 0) "Memoria comeca vazia, com a dica"
  Add-Mem 'Meu gestor e o Carlos'
  $m1 = Wait-Mem { param($s) @($s.rows).Count -eq 1 }
  Check (@($m1.rows).Count -eq 1 -and @($m1.rows)[0].text -eq 'Meu gestor e o Carlos') "Guardar poe a memoria na lista"
  Check ((@($m1.rows)[0].chips -contains 'Fato')) "como fato"
  Add-Mem '  meu gestor e o CARLOS '
  Start-Sleep -Milliseconds 800
  Check (@((Get-Mem).rows).Count -eq 1) "a mesma frase nao repete"
  EvJson 'settings.html' 'JSON.stringify((() => { document.querySelector("#memlist .mem-text").click(); const i = document.querySelector("#memlist .mem-edit"); i.value = "Meu gestor e o Carlos Lima"; i.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true })); return true; })())' | Out-Null
  $m2 = Wait-Mem { param($s) @($s.rows)[0].text -eq 'Meu gestor e o Carlos Lima' }
  Check (@($m2.rows)[0].text -eq 'Meu gestor e o Carlos Lima') "clicar no texto corrige, Enter guarda"
  Click-Mem 'Meu gestor' 'Fixar' | Out-Null
  $m3 = Wait-Mem { param($s) @($s.rows)[0].chips -contains 'fixada' }
  Check ((@($m3.rows)[0].chips -contains 'fixada') -and (@($m3.rows)[0].buttons -contains 'Soltar')) "Fixar marca a memoria"
  Click-Mem 'Meu gestor' 'Arquivar' | Out-Null
  $m4 = Wait-Mem { param($s) $s.empty -and $s.archived }
  Check ($m4.empty -and $m4.archived -eq 'Arquivadas (1)') "Arquivar tira da lista e guarda nas arquivadas ('$($m4.archived)')"
  EvJson 'settings.html' 'JSON.stringify((() => { const b = [...document.querySelectorAll(".toast-act")].pop(); if (!b) return false; b.click(); return true; })())' | Out-Null
  $m5 = Wait-Mem { param($s) @($s.rows).Count -eq 1 }
  Check (@($m5.rows).Count -eq 1 -and -not $m5.archived) "o Desfazer do aviso restaura"

  # 5) O assistente propoe guardar e so guarda com o toque; a memoria vai para a IA.
  Set-Content -Path $llmToml -Encoding ASCII -Value @("provider = `"openai`"", "model = `"fake-agente`"", "base_url = `"$llmBase`"")
  Invoke-Isper 'open_today_window' | Out-Null
  Wait-IsperWindow 'today.html' | Out-Null
  EvJson 'today.html' 'JSON.stringify((() => { const i = document.getElementById("ask-q"); i.value = "lembra que eu prefiro reunioes depois das 10h"; document.getElementById("ask").requestSubmit(); return true; })())' | Out-Null
  $card = $null
  for ($i = 0; $i -lt 40 -and -not $card; $i++) {
    $card = EvJson 'today.html' 'JSON.stringify((() => { const li = document.querySelector("#thread > li.confirm:last-child"); return li ? li.textContent : null; })())'
    if (-not $card) { Start-Sleep -Milliseconds 300 }
  }
  Check ($card -like '*Guardar na mem*prefer*Prefiro reuni*10h*') "o cartao pede para guardar a preferencia ('$card')"
  $before = Invoke-Isper 'memory_list' '{ archived: false }'
  Check (@($before).Count -eq 1) "nada guardado antes do toque"
  EvJson 'today.html' 'JSON.stringify((() => { [...document.querySelectorAll("#thread > li.confirm:last-child .turn-acts button")].find(b => b.textContent === "Fazer").click(); return true; })())' | Out-Null
  $saved = $null
  for ($i = 0; $i -lt 40; $i++) {
    $saved = @(Invoke-Isper 'memory_list' '{ archived: false }' | ForEach-Object { $_ })
    if ($saved.Count -ge 2) { break }
    Start-Sleep -Milliseconds 300
  }
  $pref = $saved | Where-Object { $_.text -like 'Prefiro reuni*' }
  Check ($null -ne $pref -and $pref.origin -eq 'assistant' -and $pref.kind -eq 'preference') "depois do toque, a preferencia e guardada pelo assistente"
  $answer = EvJson 'today.html' 'JSON.stringify(document.querySelector("#thread > li:last-child").textContent)'
  Check ($answer -like '*Guardei na mem*') "e a IA confirma ('$answer')"
  EvJson 'today.html' 'JSON.stringify((() => { const i = document.getElementById("ask-q"); i.value = "O que eu tenho para hoje?"; document.getElementById("ask").requestSubmit(); return true; })())' | Out-Null
  $last = $null
  for ($i = 0; $i -lt 40; $i++) {
    $last = Get-List "http://127.0.0.1:$llmPort/requests" | Select-Object -Last 1
    if ($last.question -like 'O que eu tenho*') { break }
    Start-Sleep -Milliseconds 300
  }
  Check ($last.system -like '*Meu gestor e o Carlos Lima*' -and $last.system -like '*Prefiro reuni*') "a memoria vai nas instrucoes da IA"
  Invoke-Isper 'open_settings_window' "{ section: 'memoria' }" | Out-Null
  Wait-IsperWindow 'settings.html' | Out-Null
  $m6 = Wait-Mem { param($s) @($s.rows).Count -eq 2 }
  $prow = @($m6.rows) | Where-Object { $_.text -like 'Prefiro reuni*' }
  Check ((@($prow.chips) -contains 'pelo assistente') -and (@($prow.chips) | Where-Object { $_ -like 'Prefer*' }).Count -eq 1) "nas Configuracoes, com os selos 'Preferencia' e 'pelo assistente' ($(@($prow.chips) -join ', '))"
  $permJs = 'JSON.stringify((() => { const r = [...document.querySelectorAll("#agisper .perm")].find(x => x.querySelector("code").textContent === "lembrar"); return r ? { title: r.querySelector(".perm-name span").textContent, allowDisabled: r.querySelector("button[data-p=allow]").disabled, on: r.querySelector("[aria-checked=true]").dataset.p } : null; })())'
  $perm = $null
  for ($i = 0; $i -lt 20 -and -not $perm; $i++) { $perm = EvJson 'settings.html' $permJs; if (-not $perm) { Start-Sleep -Milliseconds 300 } }
  Check ($perm.allowDisabled -and $perm.on -eq 'ask' -and $perm.title -like 'Guardar na mem*') "em Assistente, guardar na memoria nunca fica livre"

  $errs = @(Get-JsErrors 'today.html') + @(Get-JsErrors 'settings.html')
  Check (@($errs | Where-Object { $_ }).Count -eq 0) "Hoje e Configuracoes sem erros de JS$(Format-JsErrors $errs)"
} finally {
  if ($llmToml) { Remove-Item $llmToml -ErrorAction SilentlyContinue }
  if ($fakeLlm -and -not $fakeLlm.HasExited) { Stop-Process -Id $fakeLlm.Id -Force -ErrorAction SilentlyContinue }
}

Restart-IsperClean -Exe $exe
Check (-not (Test-Cdp)) "relancado sem porta CDP"
Finish-E2E 'learning'
