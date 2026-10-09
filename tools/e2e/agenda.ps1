# E2E da agenda e das reunioes (Fase 10.3) pela janela real, via CDP, contra
# um OptTime de mentira (fake-opttime.mjs, em 127.0.0.1): sem conector a secao
# Agenda nem aparece; com ele, a tela Hoje mostra os eventos do dia (o que
# esta acontecendo, o livre, o de dia inteiro), o link de entrar e o preparo
# com quem organiza. A tarefa que saiu de uma reuniao leva a Biblioteca no
# minuto em que foi dita, e as duas opcoes novas aparecem nas Configuracoes.
#
#   .\tools\e2e\agenda.ps1 -Exe <caminho do exe>
#
# Nao clica em "Entrar" (abriria o navegador) nem dispara o aviso de preparo:
# o proximo evento do OptTime de mentira fica a 30 min, fora da janela.
#
# ASCII puro de proposito: o PowerShell 5.1 le .ps1 sem BOM como ANSI.
param([string]$Exe)
. "$PSScriptRoot\common.ps1"

$exe = Get-IsperExe $Exe
"agenda: $exe"

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

$agendaJs = 'JSON.stringify((() => { const sec = document.getElementById("sec-agenda"); const tg = document.querySelector("#list-agenda .ev-past-toggle button"); return { hidden: sec.hidden, toggle: tg ? tg.textContent : null, note: document.getElementById("agenda-note").hidden ? null : document.getElementById("agenda-note").textContent, allday: document.getElementById("agenda-allday").hidden ? null : document.getElementById("agenda-allday").textContent, events: [...document.querySelectorAll("#list-agenda .ev")].map(li => ({ key: li.dataset.key, cls: li.className, text: li.textContent, buttons: [...li.querySelectorAll(".ev-acts button")].map(b => b.textContent), prep: li.querySelector(".prep") ? li.querySelector(".prep").textContent : null })) }; })())'
function Wait-Agenda {
  param([scriptblock]$Ok, [int]$Seconds = 12)
  $sw = [Diagnostics.Stopwatch]::StartNew()
  do {
    $s = EvJson 'today.html' $agendaJs
    if ($s -and (& $Ok $s)) { return $s }
    Start-Sleep -Milliseconds 300
  } while ($sw.Elapsed.TotalSeconds -lt $Seconds)
  $s
}
function Find-Event($s, [string]$uid) { @($s.events) | Where-Object { $_.key -like "$uid|*" } | Select-Object -First 1 }

try {
  Stop-Isper
  Check (Start-Isper -Exe $exe) "app abriu com a tela Inicio (CDP)"
  Invoke-Isper 'set_ui_lang' "{ lang: 'pt-BR' }" | Out-Null
  Invoke-Isper 'opttime_clear_token' | Out-Null
  Invoke-Isper 'opttime_set_url' "{ url: null }" | Out-Null

  # 1) Sem conector, a secao Agenda nem aparece.
  Invoke-Isper 'open_today_window' | Out-Null
  Wait-IsperWindow 'today.html' | Out-Null
  $a0 = Wait-Agenda { param($s) $s.hidden -eq $true } -Seconds 4
  Check ($a0.hidden -eq $true) "sem o OptTime conectado, a Agenda nao aparece"

  # 2) Com o conector, os eventos do dia.
  Invoke-Isper 'opttime_set_url' "{ url: '$url' }" | Out-Null
  Invoke-Isper 'opttime_set_token' "{ token: '$token' }" | Out-Null
  $a1 = Wait-Agenda { param($s) (-not $s.hidden) -and @($s.events).Count -ge 4 }
  Check ((-not $a1.hidden) -and @($a1.events).Count -eq 4) "conectado, a Agenda mostra os 4 eventos com hora ($(@($a1.events).Count))"
  Check ($a1.allday -like '*Feriado municipal*') "o de dia inteiro fica num selo a parte ('$($a1.allday)')"
  $daily = Find-Event $a1 'ev-daily'
  Check ($daily.cls -like '*now*' -and $daily.text -like '*agora*') "o que esta acontecendo ganha 'agora'"
  Check (($daily.buttons -contains 'Entrar') -and ($daily.buttons -contains 'Preparo')) "a reuniao online tem Entrar e Preparo ($($daily.buttons -join ', '))"
  Check ($a1.toggle -like '*que ja passou*' -or $a1.toggle -like '*que j*passou*') "a que ja passou fica recolhida ('$($a1.toggle)')"
  Check ($null -eq (Find-Event $a1 'ev-cedo')) "e nao aparece ate pedir"
  EvJson 'today.html' 'JSON.stringify((() => { document.querySelector("#list-agenda .ev-past-toggle button").click(); return true; })())' | Out-Null
  $aPast = Wait-Agenda { param($s) $null -ne (Find-Event $s 'ev-cedo') }
  $cedo = Find-Event $aPast 'ev-cedo'
  Check ($cedo.cls -like '*past*' -and -not ($cedo.buttons -contains 'Entrar')) "aberta, a que passou aparece apagada e sem Entrar"
  EvJson 'today.html' 'JSON.stringify((() => { document.querySelector("#list-agenda .ev-past-toggle button").click(); return true; })())' | Out-Null
  $refino = Find-Event $a1 'ev-refino'
  Check ($refino.text -like '*em 30 min*' -or $refino.text -like '*em 29 min*' -or $refino.text -like '*em 31 min*') "a proxima diz quanto falta ('$($refino.text)')"
  $foco = Find-Event $a1 'ev-foco'
  Check ($foco.cls -like '*free*' -and $foco.text -like '*livre*' -and -not ($foco.buttons -contains 'Preparo')) "o bloqueio livre nao pede preparo"
  $visita = Find-Event $a1 'ev-visita'
  Check (-not ($visita.buttons -contains 'Entrar') -and ($visita.buttons -contains 'Preparo')) "a presencial nao tem link, mas tem preparo"

  # 3) O preparo, com quem organiza.
  $js = 'JSON.stringify((() => { const li = [...document.querySelectorAll("#list-agenda .ev")].find(x => x.dataset.key.startsWith("ev-refino|")); const b = [...li.querySelectorAll(".ev-acts button")].find(x => x.textContent === "Preparo"); b.click(); return true; })())'
  EvJson 'today.html' $js | Out-Null
  $a2 = Wait-Agenda { param($s) (Find-Event $s 'ev-refino').prep -like '*Organiza*' -and (Find-Event $s 'ev-refino').prep -notlike '*Juntando*' }
  $prep = (Find-Event $a2 'ev-refino').prep
  Check ($prep -like '*Organiza: Ana Souza*' -and $prep -like '*Primeira vez*') "o preparo diz quem organiza e que e a primeira vez ('$prep')"

  # 4) Atualizar le de novo do OptTime.
  $before = @(Get-FakeCalls | Where-Object { $_.name -eq 'opt_time_get_my_agenda' }).Count
  EvJson 'today.html' 'JSON.stringify((() => { document.getElementById("agenda-refresh").click(); return true; })())' | Out-Null
  $after = $before
  for ($i = 0; $i -lt 20 -and $after -le $before; $i++) {
    Start-Sleep -Milliseconds 250
    $after = @(Get-FakeCalls | Where-Object { $_.name -eq 'opt_time_get_my_agenda' }).Count
  }
  Check ($after -gt $before) "Atualizar le a agenda de novo ($before -> $after)"

  # 5) A tarefa que saiu de uma reuniao leva a ela, no minuto.
  Invoke-Isper 'task_add' "{ task: { title: 'Mandar a planilha de estimativas', status: 'inbox', source_kind: 'meeting', source_ref: { meeting_id: 999, at_secs: 754.6, meeting_title: 'Daily do Portal' } } }" | Out-Null
  $chipJs = 'JSON.stringify((() => { const b = [...document.querySelectorAll("#list-inbox .chip.as-link")].find(x => x.textContent.includes("Reuni")); return b ? { text: b.textContent, title: b.title } : null; })())'
  $chip = $null
  for ($i = 0; $i -lt 20 -and -not $chip; $i++) { $chip = EvJson 'today.html' $chipJs; if (-not $chip) { Start-Sleep -Milliseconds 250 } }
  Check ($chip.text -like '*12:34*' -and $chip.title -eq 'Daily do Portal') "a tarefa da caixa mostra a reuniao e o minuto ('$($chip.text)')"
  EvJson 'today.html' 'JSON.stringify((() => { const b = [...document.querySelectorAll("#list-inbox .chip.as-link")].find(x => x.textContent.includes("Reuni")); b.click(); return true; })())' | Out-Null
  Check (Wait-IsperWindow 'library.html') "o selo abre a Biblioteca"
  $lib = $null
  for ($i = 0; $i -lt 20; $i++) {
    $lib = EvJson 'library.html' 'JSON.stringify(document.getElementById("detail") ? document.getElementById("detail").textContent : document.body.innerText)'
    if ($lib -like '*encontr*') { break }
    Start-Sleep -Milliseconds 250
  }
  Check ($lib -like '*encontr*') "a Biblioteca recebe a reuniao pedida (a 999 nao existe neste perfil)"

  # 6) As opcoes novas nas Configuracoes.
  Invoke-Isper 'open_settings_window' "{ section: 'reunioes' }" | Out-Null
  Wait-IsperWindow 'settings.html' | Out-Null
  $opts = EvJson 'settings.html' 'JSON.stringify({ inbox: document.getElementById("meetinginbox").checked, prep: document.getElementById("meetingprep").checked })'
  Check ($opts.inbox -and $opts.prep) "Configuracoes: caixa de entrada e preparo ligados por padrao"
  $st = Invoke-Isper 'get_settings'
  Check ($st.meeting_inbox -eq $true -and $st.meeting_prep -eq $true) "e o app concorda ($($st.meeting_inbox), $($st.meeting_prep))"

  $errs = @(Get-JsErrors 'today.html') + @(Get-JsErrors 'settings.html') + @(Get-JsErrors 'library.html')
  Check (@($errs | Where-Object { $_ }).Count -eq 0) "Hoje, Configuracoes e Biblioteca sem erros de JS$(Format-JsErrors $errs)"
} finally {
  Invoke-Isper 'opttime_clear_token' | Out-Null
  Invoke-Isper 'opttime_set_url' "{ url: null }" | Out-Null
  if ($fake -and -not $fake.HasExited) { Stop-Process -Id $fake.Id -Force -ErrorAction SilentlyContinue }
}

Restart-IsperClean -Exe $exe
Check (-not (Test-Cdp)) "relancado sem porta CDP"
Finish-E2E 'agenda'
