# E2E das rotinas (Fase 10.2) pela janela real, via CDP, contra um OptTime de
# mentira (fake-opttime.mjs, em 127.0.0.1): o conector nas Configuracoes
# (token no cofre do perfil de teste, whoami, token errado), a rotina de horas
# pelo modelo da tela Hoje, a conferencia, a revisao das sugestoes e o
# lancamento depois do toque, que fecha o dia e conclui a tarefa. Tambem uma
# rotina comum pelo formulario, pausar e o aviso do lembrete duplicado.
#
#   .\tools\e2e\routines.ps1 -Exe <caminho do exe>
#
# A rotina de horas passa a "todo dia, as 23:59": assim o roteiro vale em
# qualquer dia e hora sem o agendador conferir sozinho no meio dele.
#
# ASCII puro de proposito: o PowerShell 5.1 le .ps1 sem BOM como ANSI.
param([string]$Exe)
. "$PSScriptRoot\common.ps1"

$exe = Get-IsperExe $Exe
"routines: $exe"

$token = 'opt_tok_e2e_' + [guid]::NewGuid().ToString('N').Substring(0, 8)
$listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
$listener.Start()
$port = $listener.LocalEndpoint.Port
$listener.Stop()
$url = "http://127.0.0.1:$port/api/mcp"
$fake = Start-Process node -ArgumentList @("`"$PSScriptRoot\fake-opttime.mjs`"", "$port", $token) -PassThru -WindowStyle Hidden
# O Invoke-RestMethod do PowerShell 5.1 devolve o array JSON como um objeto so:
# passar pela variavel e pelo pipeline desmonta o array em itens.
function Get-FakeCalls {
  $list = Invoke-RestMethod "http://127.0.0.1:$port/calls" -TimeoutSec 5
  @($list | ForEach-Object { $_ })
}
$up = $false
for ($i = 0; $i -lt 40 -and -not $up; $i++) {
  try { Get-FakeCalls | Out-Null; $up = $true } catch { Start-Sleep -Milliseconds 250 }
}
Check $up "o OptTime de mentira responde ($url)"

# O estado da linha do verificador de uma tarefa de rotina na tela Hoje.
$rlineJs = 'JSON.stringify((() => { const li = document.querySelector("li[data-id=\"TASK\"]"); const r = li && li.querySelector(".rline"); return r ? { text: r.textContent, cls: r.className, buttons: [...r.querySelectorAll("button")].map(b => b.textContent) } : { text: null, done: !!(li && li.closest("#list-done")) }; })())'
function Wait-Rline {
  param([string]$TaskId, [scriptblock]$Ok, [int]$Seconds = 12)
  $js = $rlineJs.Replace('TASK', $TaskId)
  $sw = [Diagnostics.Stopwatch]::StartNew()
  do {
    $s = EvJson 'today.html' $js
    if ($s -and (& $Ok $s)) { return $s }
    Start-Sleep -Milliseconds 300
  } while ($sw.Elapsed.TotalSeconds -lt $Seconds)
  $s
}
function Click-Rline {
  param([string]$TaskId, [string]$Label)
  $js = 'JSON.stringify((() => { const li = document.querySelector("li[data-id=\"TASK\"]"); const b = li && [...li.querySelectorAll(".rline button")].find(x => x.textContent.includes("LABEL")); if (b) b.click(); return !!b; })())'
  EvJson 'today.html' $js.Replace('TASK', $TaskId).Replace('LABEL', $Label)
}
$connJs = 'JSON.stringify({ state: document.getElementById("otstate").textContent, diag: document.getElementById("otdiag").hidden ? null : document.getElementById("otdiag").textContent, warn: document.getElementById("otwarn").hidden ? null : document.getElementById("otwarn").textContent, saved: !document.getElementById("ottokenstate").hidden })'
function Wait-Connector {
  param([scriptblock]$Ok, [int]$Seconds = 12)
  $sw = [Diagnostics.Stopwatch]::StartNew()
  do {
    $s = EvJson 'settings.html' $connJs
    if ($s -and (& $Ok $s)) { return $s }
    Start-Sleep -Milliseconds 300
  } while ($sw.Elapsed.TotalSeconds -lt $Seconds)
  $s
}

try {
  Stop-Isper
  Check (Start-Isper -Exe $exe) "app abriu com a tela Inicio (CDP)"
  Invoke-Isper 'set_ui_lang' "{ lang: 'pt-BR' }" | Out-Null
  # O cofre e o endereco do perfil de teste comecam limpos.
  Invoke-Isper 'opttime_clear_token' | Out-Null
  Invoke-Isper 'opttime_set_url' "{ url: null }" | Out-Null

  # 1) A rotina de horas pelo modelo da tela Hoje.
  Invoke-Isper 'open_today_window' | Out-Null
  Wait-IsperWindow 'today.html' | Out-Null
  $tpl = EvJson 'today.html' 'JSON.stringify((() => { document.getElementById("routines-box").open = true; const b = document.getElementById("r-opttime"); const shown = !b.hidden; b.click(); return shown; })())'
  Check ($tpl -eq $true) "o modelo 'Registrar 8h no OptTime' aparece nas rotinas"
  $hours = $null
  for ($i = 0; $i -lt 20 -and -not $hours; $i++) {
    $hours = @(Invoke-Isper 'routines_list') | Where-Object { $_.verifier -eq 'opttime.day_complete' } | Select-Object -First 1
    if (-not $hours) { Start-Sleep -Milliseconds 250 }
  }
  Check ($null -ne $hours) "o modelo cria a rotina de horas"
  Check ($hours.rrule -eq 'FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR;BYHOUR=17;BYMINUTE=0' -and $hours.mode -eq 'ask' -and $hours.action -eq 'opttime.fill_day') "dias uteis as 17:00, conferida no OptTime, pedindo um toque ($($hours.rrule), $($hours.mode))"

  # Todo dia as 23:59: vale em qualquer dia, e o agendador nao confere antes.
  # A tarefa de hoje (se ja nasceu, num dia util) acompanha a hora nova.
  Invoke-Isper 'routine_update' "{ id: '$($hours.id)', patch: { rrule: 'FREQ=DAILY;BYHOUR=23;BYMINUTE=59' } }" | Out-Null
  $occ = $null
  for ($i = 0; $i -lt 20 -and -not $occ; $i++) {
    $occ = @((Invoke-Isper 'today_load').planned) | Where-Object { $_.routine_id -eq $hours.id } | Select-Object -First 1
    if (-not $occ) { Start-Sleep -Milliseconds 250 }
  }
  Check ($occ -and $occ.source_kind -eq 'routine' -and $occ.planned_time -eq '23:59') "a tarefa de hoje nasce da rotina e segue a hora dela ($($occ.source_kind) $($occ.planned_time))"
  $again = @((Invoke-Isper 'today_load').planned) | Where-Object { $_.routine_id -eq $hours.id }
  Check (@($again).Count -eq 1) "abrir o dia de novo nao duplica a tarefa ($(@($again).Count))"

  $r1 = Wait-Rline $occ.id { param($s) $s.text -like '*Conecte o OptTime*' }
  Check ($r1.text -like '*Conecte o OptTime*' -and ($r1.buttons -contains 'Conectar')) "sem token, a tarefa pede para conectar ('$($r1.text)')"

  # 2) O conector nas Configuracoes.
  Check ((Invoke-Isper 'opttime_set_url' "{ url: '$url' }") -eq $url) "endereco de teste aceito (http so no proprio PC)"
  $bad = Invoke-Isper 'opttime_set_url' "{ url: 'http://example.com/api/mcp' }"
  Check ($bad.__error -like '*https*') "http fora do PC e recusado"
  Invoke-Isper 'open_settings_window' "{ section: 'conectores' }" | Out-Null
  Wait-IsperWindow 'settings.html' | Out-Null
  $c0 = Wait-Connector { param($s) $s.state -eq 'sem token' }
  Check ($c0.state -eq 'sem token' -and -not $c0.saved) "Configuracoes mostram o OptTime sem token ('$($c0.state)')"
  $badTok = Invoke-Isper 'opttime_set_token' "{ token: 'opt_tok_a b' }"
  Check ($badTok.__error -like '*espa*') "token com espaco no meio e recusado"
  EvJson 'settings.html' ('JSON.stringify((() => { const i = document.getElementById("ottoken"); i.value = "TOKEN"; document.getElementById("otsave").click(); return true; })())'.Replace('TOKEN', $token)) | Out-Null
  $c1 = Wait-Connector { param($s) $s.state -eq 'conectado' }
  Check ($c1.state -eq 'conectado' -and $c1.saved) "guardar o token confere sozinho e conecta ('$($c1.state)')"
  Check ($c1.diag -like '*Fulano de Teste*' -and $c1.diag -like '*conectada*' -and $c1.diag -like '*calendar:read*') "o retrato do whoami: conta, escopos e Microsoft"
  Check ($c1.warn -like '*dois lembretes*') "avisa do lembrete duplicado com a rotina de horas ligada"
  $leak = EvJson 'settings.html' 'JSON.stringify(document.body.innerText.includes("opt_tok_e2e") || document.getElementById("ottoken").value.length > 0)'
  Check ($leak -eq $false) "o token nao fica na tela depois de guardado"

  Invoke-Isper 'opttime_set_token' "{ token: 'opt_tok_errado' }" | Out-Null
  $wrong = Invoke-Isper 'opttime_check'
  Check ((-not $wrong.ok) -and $wrong.error.code -eq 'UNAUTHORIZED') "token errado vira 'recusado' ($($wrong.error.code))"
  Invoke-Isper 'opttime_set_token' "{ token: '$token' }" | Out-Null
  Check ((Invoke-Isper 'opttime_check').ok -eq $true) "com o token certo de novo, conecta"

  # 3) Conferir, revisar e lancar na tela Hoje.
  Invoke-Isper 'open_today_window' | Out-Null
  Wait-IsperWindow 'today.html' | Out-Null
  $r2 = Wait-Rline $occ.id { param($s) $s.text -like '*23:59*' }
  Check ($r2.text -like '*23:59*' -and ($r2.buttons -contains 'Conferir agora')) "conectado, a tarefa diz quando confere ('$($r2.text)')"
  Click-Rline $occ.id 'Conferir agora' | Out-Null
  $r3 = Wait-Rline $occ.id { param($s) $s.text -like '*Faltam 3h20*' }
  Check ($r3.text -like '*4h40 de 8h*' -and $r3.text -like '*Faltam 3h20*' -and $r3.cls -like '*warn*') "a conferencia mostra o que falta ('$($r3.text)')"

  Click-Rline $occ.id 'Revisar' | Out-Null
  $rvJs = 'JSON.stringify((() => { const d = document.getElementById("review"); const items = [...d.querySelectorAll(".rv-item")]; return { open: d.open, sub: document.getElementById("rv-sub").textContent, items: items.map(li => ({ on: li.querySelector("input[type=checkbox]").checked, off: li.querySelector("input[type=checkbox]").disabled, text: li.textContent })), apply: document.getElementById("rv-apply").textContent, disabled: document.getElementById("rv-apply").disabled }; })())'
  $rv = $null
  $sw = [Diagnostics.Stopwatch]::StartNew()
  do { $rv = EvJson 'today.html' $rvJs; if ($rv -and @($rv.items).Count -gt 0) { break }; Start-Sleep -Milliseconds 300 } while ($sw.Elapsed.TotalSeconds -lt 15)
  Check ($rv.open -and @($rv.items).Count -eq 3) "as sugestoes chegam na revisao ($(@($rv.items).Count))"
  Check ($rv.items[0].on -and $rv.items[1].on -and (-not $rv.items[2].on) -and $rv.items[2].off) "com projeto e confianca vem marcada; sem projeto fica de fora"
  Check ($rv.apply -like '*2*3h20*' -and -not $rv.disabled) "o botao diz quantas e quanto ('$($rv.apply)')"
  Check ($rv.sub -like '*4h40*' -and $rv.sub -like '*3h20*') "o subtitulo diz o registrado e o que falta ('$($rv.sub)')"
  $before = @(Get-FakeCalls | Where-Object { $_.name -eq 'opt_time_apply_suggestions' }).Count
  Check ($before -eq 0) "nada foi lancado antes do toque"

  EvJson 'today.html' 'JSON.stringify((() => { const d = document.querySelector("#review .rv-item .rv-edit input"); d.value = "Refinamento do backlog (sprint 42)"; d.dispatchEvent(new Event("input", { bubbles: true })); document.getElementById("rv-apply").click(); return true; })())' | Out-Null
  $r4 = Wait-Rline $occ.id { param($s) $s.done }
  Check ($r4.done -eq $true) "lancar fecha o dia e conclui a tarefa"
  $apply = @(Get-FakeCalls | Where-Object { $_.name -eq 'opt_time_apply_suggestions' })
  Check ($apply.Count -eq 1) "um lancamento so ($($apply.Count))"
  $args0 = $apply[0].args
  Check ($args0.idempotencyKey.Length -eq 36 -and @($args0.items).Count -eq 2 -and $args0.items[0].suggestionId -eq 'sug-1') "com chave de idempotencia e as duas aprovadas"
  Check ($args0.items[0].description -eq 'Refinamento do backlog (sprint 42)' -and $null -eq $args0.items[1].description) "so a descricao editada vai junto"
  $open = EvJson 'today.html' 'JSON.stringify(document.getElementById("review").open)'
  Check ($open -eq $false) "a revisao fecha depois de lancar"
  $done = @((Invoke-Isper 'today_load').done_today) | Where-Object { $_.id -eq $occ.id }
  Check ($done -and $done.source_ref.check.status -eq 'complete') "a tarefa guarda a conferencia final ($($done.source_ref.check.status))"

  # 4) Uma rotina comum pelo formulario, e pausar.
  EvJson 'today.html' 'JSON.stringify((() => { document.getElementById("routines-box").open = true; document.getElementById("r-title").value = "Revisar os PRs abertos"; document.querySelector("#r-freq .pick[data-freq=daily]").click(); const tm = document.getElementById("r-time"); tm.value = "23:58"; tm.dispatchEvent(new Event("change", { bubbles: true })); document.getElementById("rform").requestSubmit(); return true; })())' | Out-Null
  $prs = $null
  for ($i = 0; $i -lt 20 -and -not $prs; $i++) {
    $prs = @(Invoke-Isper 'routines_list') | Where-Object { $_.title -eq 'Revisar os PRs abertos' } | Select-Object -First 1
    if (-not $prs) { Start-Sleep -Milliseconds 250 }
  }
  Check ($prs.rrule -eq 'FREQ=DAILY;BYHOUR=23;BYMINUTE=58' -and -not $prs.verifier) "o formulario cria a rotina comum ($($prs.rrule))"
  $prsTask = @((Invoke-Isper 'today_load').planned) | Where-Object { $_.routine_id -eq $prs.id }
  Check (@($prsTask).Count -eq 1) "e a tarefa de hoje dela aparece"
  $row = EvJson 'today.html' ('JSON.stringify((() => { const li = document.querySelector("#list-routines li[data-id=\"ID\"]"); return li ? li.textContent : null; })())'.Replace('ID', $prs.id))
  Check ($row -like '*Todo dia, *23:58*') "a lista diz como ela se repete ('$row')"
  EvJson 'today.html' ('JSON.stringify((() => { const i = document.querySelector("#list-routines li[data-id=\"ID\"] .switch input"); i.click(); return true; })())'.Replace('ID', $prs.id)) | Out-Null
  $paused = $null
  for ($i = 0; $i -lt 20; $i++) {
    $paused = @(Invoke-Isper 'routines_list') | Where-Object { $_.id -eq $prs.id } | Select-Object -First 1
    if ($paused -and -not $paused.active) { break }
    Start-Sleep -Milliseconds 250
  }
  Check ($paused -and -not $paused.active) "o interruptor pausa a rotina"

  $errs = @(Get-JsErrors 'today.html') + @(Get-JsErrors 'settings.html')
  Check (@($errs | Where-Object { $_ }).Count -eq 0) "Hoje e Configuracoes sem erros de JS$(Format-JsErrors $errs)"
} finally {
  # O cofre e o endereco do perfil de teste ficam como estavam.
  Invoke-Isper 'opttime_clear_token' | Out-Null
  Invoke-Isper 'opttime_set_url' "{ url: null }" | Out-Null
  if ($fake -and -not $fake.HasExited) { Stop-Process -Id $fake.Id -Force -ErrorAction SilentlyContinue }
}

Restart-IsperClean -Exe $exe
Check (-not (Test-Cdp)) "relancado sem porta CDP"
Finish-E2E 'routines'
