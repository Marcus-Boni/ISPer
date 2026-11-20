<#
.SYNOPSIS
  Nomes da chamada do Teams ponta a ponta (ADR 0027), no ISPer real e sem Teams.
.DESCRIPTION
  Um "Teams" de mentira (o powershell.exe copiado como isper-e2e-teams.exe)
  toca uma fixture, e a fonte do audio vira process:isper-e2e-teams.exe: o
  process loopback do ISPer grava ESSE processo pelo mesmo caminho de "So o
  Microsoft Teams", sem depender (nem mexer) no Teams da maquina. A opcao e
  ligada pela tela, com "So o Microsoft Teams"; a fonte de teste entra no
  config.toml antes das reunioes. O leitor da chamada roda no modo roteiro
  (ISPER_TEAMS_READER_SCRIPT, que so vale no perfil de teste): em vez do UI
  Automation, ele repete uma leitura em que "Zuleika Sintetica" esta no painel
  Pessoas, marcada falando. Um nome que nao existe em lugar nenhum: se ele
  aparecer fora da Biblioteca, vazou.

  Tres reunioes:
    A) uma voz, marcada o tempo todo -> o nome aparece na Biblioteca (com o
       rotulo "Participante N" por baixo); corrigir pelo clique vira o nome
       da reuniao (e entao, sim, vai para a ata).
    B) a mesma coisa -> "Remover nomes detectados" apaga so as sugestoes.
    C) duas vozes, a mesma pessoa marcada o tempo todo -> dois grupos
       reivindicam a mesma pessoa: nenhum nome (zero atribuicao errada).
  E confere: a opcao nos dois idiomas, os pre-requisitos na tela, e a
  ausencia do nome no Markdown, no SRT, no DOCX, no texto que vai para a IA
  do resumo (uma IA de mentira, fake-llm.mjs), nos logs e no pacote de
  diagnostico.

  Precisa do modelo Whisper e dos modelos de diarizacao. Leva ~5 min.
.EXAMPLE
  .\tools\e2e\teams-names.ps1 -Exe .\target\release\isper-app.exe
#>
param([string]$Exe, [switch]$SkipDisputed)
. "$PSScriptRoot\common.ps1"
$exe = Get-IsperExe $Exe
"teams-names: $exe"

$fixtures = Join-Path $script:E2ERoot 'fixtures'
$oneVoice = Join-Path $fixtures 'fala-16k.wav'
$twoVoices = Join-Path $fixtures 'duas-vozes-16k.wav'
$work = Join-Path ([IO.Path]::GetTempPath()) "isper-e2e-teams-$PID"
New-Item -ItemType Directory -Force $work | Out-Null
$utf8 = New-Object Text.UTF8Encoding $false

# O "Teams": um powershell.exe com um nome so dele.
$fakeName = 'isper-e2e-teams.exe'
$fakeTeams = Join-Path $work $fakeName
Copy-Item (Join-Path $PSHOME 'powershell.exe') $fakeTeams -Force
& $fakeTeams -NoProfile -NonInteractive -Command 'exit 7' | Out-Null
Check ($LASTEXITCODE -eq 7) "o Teams de mentira roda ($fakeTeams)"

# O roteiro do leitor: uma leitura so, repetida (como um Teams parado na
# mesma tela). O nome tem acento: montado com [char], porque este arquivo e
# ASCII (o PowerShell 5.1 le .ps1 sem BOM como ANSI), e gravado em UTF-8.
$name = 'Zuleika Sint' + [char]0xE9 + 'tica'
$scriptPath = Join-Path $work 'leituras.jsonl'
$line = '{"at_ms":0,"state":"ok","reading":{"speaking":["' + $name + '"],"participants":["Pessoa E2E","' + $name + '","Xavier Ficticio"],"me":"Pessoa E2E"}}'
[IO.File]::WriteAllText($scriptPath, $line + "`n", $utf8)

# A IA de mentira do resumo: guarda o texto de cada chamada.
function Get-FreePort {
  $l = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
  $l.Start(); $p = $l.LocalEndpoint.Port; $l.Stop(); $p
}
$llmPort = Get-FreePort
$fakeLlm = Start-Process node -ArgumentList @("`"$PSScriptRoot\fake-llm.mjs`"", "$llmPort") -PassThru -WindowStyle Hidden
function Get-LlmRequests {
  try { @((Invoke-RestMethod "http://127.0.0.1:$llmPort/plain" -TimeoutSec 5) | ForEach-Object { $_ }) }
  catch { Write-Host "  (diagnostico) IA de mentira: $($_.Exception.Message)"; @() }
}

Stop-Isper
Check (Start-Isper -Exe $exe -Env @{ ISPER_TEAMS_READER_SCRIPT = $scriptPath }) "app abriu (CDP) com o roteiro do leitor"
$st = Invoke-Isper 'home_status'
Check ($st.engine.kind -eq 'ready') "motor pronto ($($st.engine.kind))"
$diar = Invoke-Isper 'diarize_status'
Check ($diar -eq $true) "modelos de diarizacao instalados"
if ($st.engine.kind -ne 'ready' -or $diar -ne $true) { "sem modelo Whisper ou de diarizacao: abortando"; Stop-Isper; Finish-E2E 'teams-names' }
Set-Content -Path (Get-E2EDataPath 'llm.toml') -Encoding ASCII -Value @('provider = "openai"', 'model = "fake-resumo"', "base_url = `"http://127.0.0.1:$llmPort/v1`"")

# ---- Configuracoes: a opcao, nos dois idiomas, e o que falta para ela valer.
Invoke-Isper 'open_settings_window' | Out-Null
Check (Wait-IsperWindow 'settings.html') "Configuracoes abriram"
$s0 = Invoke-Isper 'get_settings' 'undefined' 'settings.html'
Check ($s0 -and $s0.teams_speaker_names -eq $false) "a opcao nasce desligada"
$origLang = $s0.ui_lang
$label = { EvJson 'settings.html' 'JSON.stringify({ text: document.querySelector("[data-i18n=\"settings.reunioes.teams-names\"]").textContent, req: document.getElementById("teamsnamesreq").textContent, reqHidden: document.getElementById("teamsnamesreq").hidden, errors: window.__isperErrors || [] })' }
Invoke-Isper 'set_ui_lang' "{ lang: 'pt-BR' }" 'settings.html' | Out-Null
Start-Sleep -Milliseconds 800
$pt = & $label
Check ($pt -and $pt.text -like 'Identificar falantes do Teams*') "rotulo em pt-BR ('$($pt.text)')"
Invoke-Isper 'set_ui_lang' "{ lang: 'en' }" 'settings.html' | Out-Null
Start-Sleep -Milliseconds 800
$en = & $label
Check ($en -and $en.text -like 'Identify Teams speakers*') "rotulo em ingles ('$($en.text)')"
Invoke-Isper 'set_ui_lang' "{ lang: 'pt-BR' }" 'settings.html' | Out-Null
Start-Sleep -Milliseconds 800

# Liga com a fonte errada: a tela diz o que falta.
Ev 'settings.html' '(() => { const c = document.getElementById("teamsnames"); c.checked = true; c.dispatchEvent(new Event("change")); const s = document.getElementById("source"); s.value = "system"; s.dispatchEvent(new Event("change")); return "ok"; })()' | Out-Null
$miss = & $label
Check ($miss -and -not $miss.reqHidden -and $miss.req -like '*Microsoft Teams*') "pre-requisito que falta aparece ('$($miss.req)')"
Ev 'settings.html' '(() => { const s = document.getElementById("source"); s.value = "teams"; s.dispatchEvent(new Event("change")); document.getElementById("finalpass").checked = true; document.getElementById("save").click(); return "ok"; })()' | Out-Null
Start-Sleep -Seconds 2
$ready = & $label
Check ($ready -and $ready.req -like 'Pronto*') "com tudo certo, a tela diz que esta pronto ('$($ready.req)')"
$s1 = Invoke-Isper 'get_settings' 'undefined' 'settings.html'
Check ($s1.teams_speaker_names -eq $true -and $s1.meeting_source -eq 'teams' -and $s1.final_pass -eq $true) "Salvar liga a opcao com 'So o Microsoft Teams'"
Check ((Get-Content (Get-E2EDataPath 'config.toml') -Raw) -match 'teams_speaker_names = true') "config.toml guarda teams_speaker_names = true"
Check (@($ready.errors).Count -eq 0) "Configuracoes sem erro de JS $(Format-JsErrors $ready.errors)"

# A fonte de teste: o processo do "Teams" de mentira, pelo mesmo loopback.
Stop-Isper
$cfgPath = Get-E2EDataPath 'config.toml'
$cfgText = (Get-Content $cfgPath -Raw) -replace 'meeting_source = "teams"', "meeting_source = `"process:$fakeName`""
[IO.File]::WriteAllText($cfgPath, $cfgText, $utf8)
Check (Start-Isper -Exe $exe -Env @{ ISPER_TEAMS_READER_SCRIPT = $scriptPath }) "app reabriu com a fonte process:$fakeName"
Check ((Invoke-Isper 'get_settings').meeting_source -eq "process:$fakeName") "a fonte de teste valeu"

# Dentro de uma funcao que devolve valor, o texto do Check vai para a tela,
# nao para o valor devolvido.
function Show-Check { param([bool]$Ok, [string]$What) Check $Ok $What | Write-Host }

# A linha do log (JSON Lines) que fecha o passe final desta reuniao.
function Test-FinalPassDone {
  param([int]$Id)
  $lines = @(Get-Content (Get-TodayLog) -Encoding UTF8 -ErrorAction SilentlyContinue)
  @($lines | Where-Object {
      ($_ -match 'final publicada' -or $_ -match 'passe final falhou' -or $_ -match 'durante o passe final') -and
      ($_ -match ('"meeting_id":' + $Id + '\b') -or $_ -match ('meeting_id=' + $Id + '\b'))
    }).Count -gt 0
}

# ---- Uma reuniao com o "Teams" tocando: devolve a reuniao depois do passe final.
function Invoke-TeamsMeeting {
  param([string[]]$Wavs, [string]$Tag)
  $before = @(Invoke-Isper 'list_meetings' '{ query: null }')
  $plays = ($Wavs | ForEach-Object { "`$p = New-Object Media.SoundPlayer '$_'; `$p.PlaySync(); Start-Sleep -Milliseconds 1500" }) -join '; '
  $proc = Start-Process $fakeTeams -ArgumentList @('-NoProfile', '-NonInteractive', '-Command', "Start-Sleep -Seconds 4; $plays; Start-Sleep -Seconds 2") -PassThru -WindowStyle Hidden
  Start-Sleep -Milliseconds 800
  $r = Invoke-Isper 'toggle_meeting_cmd'
  Show-Check ($null -eq $r -or -not $r.__error) "[$Tag] reuniao iniciada com o Teams de mentira tocando"
  $proc.WaitForExit(120000) | Out-Null
  Invoke-Isper 'toggle_meeting_cmd' | Out-Null
  $id = $null
  for ($i = 0; $i -lt 40 -and -not $id; $i++) {
    Start-Sleep -Seconds 1
    $now = @(Invoke-Isper 'list_meetings' '{ query: null }')
    $new = @($now | Where-Object { $before.id -notcontains $_.id })
    if ($new.Count) { $id = $new[0].id }
  }
  Show-Check ([bool]$id) "[$Tag] reuniao salva (id $id)"
  if (-not $id) { return $null }
  $done = $false
  for ($i = 0; $i -lt 240 -and -not $done; $i++) {
    Start-Sleep -Seconds 1
    $done = Test-FinalPassDone $id
  }
  Show-Check $done "[$Tag] passe final terminou"
  Start-Sleep -Seconds 2
  Invoke-Isper 'get_meeting' "{ id: $id }"
}

function Test-NoLeak {
  param($Det, [string]$Tag)
  $id = $Det.meeting.id
  $md = Get-Content $Det.meeting.md_path -Raw -Encoding UTF8
  Check ($md -notmatch 'Zuleika') "[$Tag] a ata (.md) nao tem o nome detectado"
  $srt = Invoke-Isper 'export_meeting' "{ id: $id, format: 'srt' }"
  Check ((Test-Path $srt) -and ((Get-Content $srt -Raw -Encoding UTF8) -notmatch 'Zuleika')) "[$Tag] o SRT nao tem o nome detectado"
  $docx = Invoke-Isper 'export_meeting' "{ id: $id, format: 'docx' }"
  $tmp = Join-Path $work "docx-$id"
  Copy-Item $docx "$tmp.zip" -Force
  Expand-Archive "$tmp.zip" $tmp -Force
  Check ((Get-Content (Join-Path $tmp 'word\document.xml') -Raw -Encoding UTF8) -notmatch 'Zuleika') "[$Tag] o DOCX nao tem o nome detectado"
}

# ---- A) o nome aparece na Biblioteca, e corrigir vira nome da reuniao.
$a = Invoke-TeamsMeeting @($oneVoice, $oneVoice, $oneVoice) 'A'
$segsA = @($a.segments)
$named = @($segsA | Where-Object { $_.detected_speaker })
Check ($named.Count -gt 0) "[A] $($named.Count) de $($segsA.Count) falas ganharam o nome lido do Teams"
Check (@($named | Where-Object { $_.detected_speaker -ne $name }).Count -eq 0) "[A] o nome e o do roteiro, com acento"
Check (@($named | Where-Object { $_.speaker -notlike 'Participante *' }).Count -eq 0) "[A] por baixo, o rotulo continua 'Participante N'"
Check (@($segsA | Where-Object { $_.speaker -eq 'Eu' -and $_.detected_speaker }).Count -eq 0) "[A] 'Eu' nunca recebe nome do Teams"
$log = Get-Content (Get-TodayLog) -Raw -Encoding UTF8
Check ($log -match 'nomes do Teams: lendo a chamada') "[A] o log registra a coleta"
Check ($log -match 'nomes do Teams: atribui') "[A] o log registra a atribuicao (so contagens)"
Test-NoLeak $a 'A'

Invoke-Isper 'open_library_window' "{ meeting: $($a.meeting.id) }" | Out-Null
$libJs = 'JSON.stringify({ det: [...document.querySelectorAll("#detail .sp.detected")].map(e => ({ text: e.textContent, title: e.title })), note: !!document.querySelector("#detail .detected-note"), remove: !!document.getElementById("remove-detected"), speakers: [...document.querySelectorAll("#detail .sp")].map(e => e.textContent), errors: window.__isperErrors || [] })'
$lib = $null
for ($i = 0; $i -lt 10; $i++) {
  Start-Sleep -Seconds 2
  $lib = EvJson 'library.html' $libJs
  if ($lib -and @($lib.det).Count -gt 0) { break }
}
Check ($lib -and @($lib.det).Count -gt 0 -and @($lib.det)[0].text -eq $name) "[A] a Biblioteca mostra '$name' com o sublinhado pontilhado"
Check ($lib -and @($lib.det)[0].title -like '*Participante*') "[A] a dica do nome diz o rotulo da ata"
Check ($lib -and $lib.note -and $lib.remove) "[A] aviso dos nomes detectados e o botao de remover na tela"
Check ($lib -and @($lib.errors).Count -eq 0) "[A] Biblioteca sem erro de JS $(Format-JsErrors $lib.errors)"

# Corrigir pelo clique: a edicao e pela chave do falante.
Ev 'library.html' '(() => { const sp = document.querySelector("#detail .sp.detected"); if (!sp) return "sem nome"; sp.click(); const i = sp.querySelector("input"); i.value = "Zuleika Confirmada"; i.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter" })); return "ok"; })()' | Out-Null
Start-Sleep -Seconds 3
$a2 = Invoke-Isper 'get_meeting' "{ id: $($a.meeting.id) }"
$fixed = @($a2.segments | Where-Object { $_.speaker -eq 'Zuleika Confirmada' })
Check ($fixed.Count -gt 0 -and @($a2.segments | Where-Object { $_.detected_speaker }).Count -eq 0) "[A] corrigir troca a sugestao pelo nome dado ($($fixed.Count) falas)"
Check ((Get-Content $a2.meeting.md_path -Raw -Encoding UTF8) -match 'Zuleika Confirmada') "[A] o nome dado a mao vai para a ata"

# ---- B) remover os nomes detectados preserva falas e rotulos.
$b = Invoke-TeamsMeeting @($oneVoice, $oneVoice, $oneVoice) 'B'
$segsB = @($b.segments)
Check (@($segsB | Where-Object { $_.detected_speaker }).Count -gt 0) "[B] a segunda reuniao tambem ganhou o nome"
Test-NoLeak $b 'B'
Invoke-Isper 'open_library_window' "{ meeting: $($b.meeting.id) }" | Out-Null
for ($i = 0; $i -lt 10; $i++) {
  Start-Sleep -Seconds 2
  $lib = EvJson 'library.html' $libJs
  if ($lib -and $lib.remove) { break }
}
# Dois cliques: o primeiro arma, o segundo remove.
Ev 'library.html' '(() => { const b = document.getElementById("remove-detected"); if (!b) return "sem botao"; b.click(); b.click(); return "ok"; })()' | Out-Null
Start-Sleep -Seconds 3
$b2 = Invoke-Isper 'get_meeting' "{ id: $($b.meeting.id) }"
Check ((@($segsB | Where-Object { $_.detected_speaker }).Count -gt 0) -and (@($b2.segments | Where-Object { $_.detected_speaker }).Count -eq 0)) "[B] 'Remover nomes detectados' apagou as sugestoes"
Check (@($b2.segments).Count -eq $segsB.Count -and ((@($b2.segments).speaker -join '|') -eq (@($segsB).speaker -join '|'))) "[B] as falas e os rotulos ficaram"
$lib = EvJson 'library.html' $libJs
Check ($lib -and @($lib.det).Count -eq 0 -and -not $lib.remove) "[B] a Biblioteca volta a 'Participante N' sem o botao"

# ---- C) duas vozes, a mesma pessoa marcada: nome disputado, nenhum nome.
if (-not $SkipDisputed) {
  $c = Invoke-TeamsMeeting @($twoVoices) 'C'
  $groups = @($c.segments | Where-Object { $_.speaker -like 'Participante *' } | ForEach-Object { $_.speaker } | Sort-Object -Unique)
  if ($groups.Count -ge 2) {
    Check (@($c.segments | Where-Object { $_.detected_speaker }).Count -eq 0) "[C] $($groups.Count) grupos reivindicando a mesma pessoa: nenhum nome"
  } else {
    "  (aviso) [C] a diarizacao achou $($groups.Count) grupo(s) nas duas vozes: o caso disputado nao se formou"
  }
}

# ---- O nome nunca saiu: IA do resumo, logs, diagnostico.
Start-Sleep -Seconds 3
$reqs = Get-LlmRequests
Check ($reqs.Count -gt 0) "a IA de mentira recebeu $($reqs.Count) pedidos de resumo"
Check (@($reqs | Where-Object { $_.text -match 'Participante \d' }).Count -gt 0) "o texto do resumo leva os rotulos 'Participante N'"
Check (@($reqs | Where-Object { $_.text -match 'Zuleika S' }).Count -eq 0) "nenhum texto para a IA tem o nome detectado"
$logsLeak = @(Get-ChildItem (Get-E2ELogsDir) -File | Where-Object { (Get-Content $_.FullName -Raw -Encoding UTF8) -match 'Zuleika' })
Check ($logsLeak.Count -eq 0) "nenhum log tem o nome ($($logsLeak.Name -join ', '))"
Invoke-Isper 'open_settings_window' | Out-Null
Wait-IsperWindow 'settings.html' | Out-Null
$zip = Invoke-Isper 'export_diagnostics' 'undefined' 'settings.html'
if ($zip -and -not $zip.__error -and (Test-Path $zip)) {
  $dz = Join-Path $work 'diag'
  Expand-Archive $zip $dz -Force
  $diagLeak = @(Get-ChildItem $dz -Recurse -File | Where-Object { (Get-Content $_.FullName -Raw -Encoding UTF8 -ErrorAction SilentlyContinue) -match 'Zuleika' })
  Check ($diagLeak.Count -eq 0) "o pacote de diagnostico nao tem o nome"
  Remove-Item $zip -Force -ErrorAction SilentlyContinue
} else {
  Check $false "pacote de diagnostico gerado ($($zip.__error))"
}

# ---- Desligar a opcao: o config volta e o leitor nao abre mais. (A tela
# nao conhece a fonte de teste: volta para "So o Microsoft Teams".)
Ev 'settings.html' '(() => { const s = document.getElementById("source"); s.value = "teams"; document.getElementById("teamsnames").checked = false; document.getElementById("save").click(); return "ok"; })()' | Out-Null
Start-Sleep -Seconds 2
Check ((Invoke-Isper 'get_settings' 'undefined' 'settings.html').teams_speaker_names -eq $false) "desligar pela tela grava a opcao desligada"
Check ((Get-Content (Get-TodayLog) -Raw -Encoding UTF8) -match 'nomes do Teams desligados') "desligar invalida a evidencia pendente (log)"
if ($origLang) { Invoke-Isper 'set_ui_lang' "{ lang: '$origLang' }" 'settings.html' | Out-Null }

Stop-Process -Id $fakeLlm.Id -Force -ErrorAction SilentlyContinue
Restart-IsperClean $exe
Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
Finish-E2E 'teams-names'
