<#
.SYNOPSIS
  Reunião ponta a ponta no ISPer real com a fixture de duas vozes.
.DESCRIPTION
  Inicia uma reunião, toca fixtures\duas-vozes-16k.wav nos alto-falantes (o loopback
  captura), confere transcrição ao vivo e legendas no indicador, marca momentos pelo
  comando (com debounce) e pelo atalho global, encerra, e confere banco, Markdown,
  DOCX e Biblioteca. No fim apaga a reunião de teste e os arquivos gerados (salvo
  -KeepMeeting) e relança o app limpo. Leva ~2 min (+ título/resumo por IA, se houver
  provider configurado).

  ATENÇÃO: o loopback captura TUDO que estiver tocando no PC — não rode durante uma
  reunião real nem com música tocando. O título vai para o provider de IA configurado.
.EXAMPLE
  .\tools\e2e\meeting.ps1 -Exe .\target\release\isper-app.exe
#>
param([string]$Exe, [switch]$KeepMeeting)
. "$PSScriptRoot\common.ps1"
$exe = Get-IsperExe $Exe
$fixture = Join-Path $script:E2ERoot 'fixtures\duas-vozes-16k.wav'
if (-not (Test-Path $fixture)) { throw "fixture nao encontrada: $fixture" }
"meeting: $exe"

function ConvertTo-SendKeys([string]$label) {
  # "Ctrl+Alt+K" -> "^%k" (formato do SendKeys)
  $mods = ''; $key = ''
  foreach ($p in ($label -split '\+')) {
    switch ($p.Trim()) {
      'Ctrl' { $mods += '^' }
      'Alt' { $mods += '%' }
      'Shift' { $mods += '+' }
      default { $key = $p.Trim().ToLower() }
    }
  }
  if ($key -in 'espaço', 'espaco', 'space') { $key = ' ' }
  return $mods + $key
}

Stop-Isper
Check (Start-Isper -Exe $exe) "app abriu (CDP)"
$st0 = Invoke-Isper 'home_status'
Check ($st0.engine.kind -eq 'ready') "motor pronto ($($st0.engine.kind))"
if ($st0.engine.kind -ne 'ready') { "sem motor carregado: abortando"; Finish-E2E 'meeting' }
$before = @(Invoke-Isper 'list_meetings' '{ query: null }').Count
$originalMode = if ($st0.overlay_captions) { 'captions' } elseif ((Invoke-Isper 'overlay_prefs').mini) { 'mini' } else { 'normal' }

# Legendas ligadas para conferir o indicador durante a reunião.
Invoke-Isper 'overlay_set_mode' '{ mode: "captions" }' | Out-Null
Invoke-Isper 'show_indicator_cmd' | Out-Null
Ev 'home.html' "(() => { window.__moments = []; window.__TAURI__.event.listen('isper-moment', (e) => window.__moments.push(e.payload.at_secs)); return 'ok'; })()" | Out-Null

$r = Invoke-Isper 'toggle_meeting_cmd'
Check ($null -eq $r -or -not $r.__error) "reuniao iniciada"
Start-Sleep -Seconds 2
$ui = EvJson 'home.html' 'JSON.stringify({ live: !document.getElementById("live").hidden, mark: !document.getElementById("b-mark").hidden, cap: document.getElementById("captions").checked })'
Check ($ui.live -and $ui.mark) "painel ao vivo e botao de marcar visiveis (legendas: $($ui.cap))"

$player = New-Object System.Media.SoundPlayer $fixture
$player.Play()   # 29,5 s, assíncrono
Start-Sleep -Seconds 6
$m = EvJson 'home.html' "(async () => { const inv = window.__TAURI__.core.invoke; const [a, b] = await Promise.all([inv('mark_moment_cmd'), inv('mark_moment_cmd')]); return JSON.stringify({ a, b }); })()"
Check ($m.a -eq $m.b) "debounce: duas marcas no mesmo instante valem uma ($($m.a) s)"
Start-Sleep -Seconds 8

Add-Type -AssemblyName System.Windows.Forms
$null = (New-Object -ComObject WScript.Shell).AppActivate('ISPer')
Start-Sleep -Milliseconds 400
$keys = ConvertTo-SendKeys $st0.mark_shortcut
[System.Windows.Forms.SendKeys]::SendWait($keys)
Start-Sleep -Seconds 10

$live = Invoke-Isper 'live_transcript'
Check (@($live).Count -gt 0) "transcricao ao vivo chegou ($(@($live).Count) falas)"
$cap = EvJson 'http://tauri.localhost/' 'JSON.stringify({ cls: document.body.className, cur: document.getElementById("cap-cur").textContent.trim() })'
$capText = if ($cap -and $cap.cur) { [string]$cap.cur } else { '' }
Check ($cap.cls -eq 'captions' -and $capText.Length -gt 0 -and $capText -notmatch 'legendas ao vivo') "indicador em legendas mostrando fala: '$($capText.Substring(0, [Math]::Min(50, $capText.Length)))'"
$moments = @(EvJson 'home.html' 'JSON.stringify(window.__moments)')
Check ($moments.Count -eq 2) "dois momentos marcados (comando + atalho $($st0.mark_shortcut)): $($moments -join ', ')"
Start-Sleep -Seconds 6

$r = Invoke-Isper 'toggle_meeting_cmd'
Check ($null -eq $r -or -not $r.__error) "reuniao encerrada"
$saved = $false
for ($i = 0; $i -lt 45; $i++) {
  Start-Sleep -Seconds 2
  $s = Invoke-Isper 'home_status'
  if (-not $s.meeting_active) { $saved = $true; break }
}
Check $saved "reuniao salva (apos $(($i + 1) * 2) s)"
Start-Sleep -Seconds 2

$rows = @(Invoke-Isper 'list_meetings' '{ query: null }')
Check ($rows.Count -eq $before + 1) "uma reuniao nova no historico ($before -> $($rows.Count))"
$row = $rows[0]
$det = Invoke-Isper 'get_meeting' "{ id: $($row.id) }"
Check ($row.moments -eq 2 -and @($det.moments).Count -eq 2) "momentos no banco: lista=$($row.moments), detalhe=$(@($det.moments).Count)"
Check (@($det.segments).Count -gt 0) "$(@($det.segments).Count) segmentos transcritos"
$md = Get-Content $det.meeting.md_path -Raw -Encoding UTF8
$bullets = @(($md -split "`n") | Where-Object { $_ -match '^- \*\*\[\d\d:\d\d\]\*\* ' })
Check (($md -match '## Momentos marcados') -and $bullets.Count -eq 2) "Markdown com a secao 'Momentos marcados' e 2 itens"
$docx = Invoke-Isper 'export_meeting' "{ id: $($row.id), format: 'docx' }"
$tmp = Join-Path $env:TEMP 'isper-e2e-docx'
Remove-Item $tmp, "$tmp.zip" -Recurse -Force -ErrorAction SilentlyContinue
Copy-Item $docx "$tmp.zip"
Expand-Archive "$tmp.zip" $tmp -Force
$doc = Get-Content (Join-Path $tmp 'word\document.xml') -Raw -Encoding UTF8
Check ($doc -match 'Momentos marcados') "DOCX com a secao 'Momentos marcados'"
Remove-Item $tmp, "$tmp.zip" -Recurse -Force -ErrorAction SilentlyContinue

Invoke-Isper 'open_library_window' "{ meeting: $($row.id) }" | Out-Null
$lib = $null
for ($i = 0; $i -lt 10; $i++) {
  # A Biblioteca abre, seleciona a reunião e pode re-renderizar quando a diarização termina.
  Start-Sleep -Seconds 2
  $lib = EvJson 'library.html' 'JSON.stringify({ chips: document.querySelectorAll(".moments > *").length, starred: document.querySelectorAll(".seg.star").length, title: (document.querySelector("#detail h2, #detail .title") || {}).textContent, errors: window.__isperErrors || [] })'
  if ($lib -and $lib.chips -ge 2 -and $lib.starred -ge 1) { break }
}
# Dois momentos viram dois chips; os parágrafos destacados são 1 ou 2 conforme as
# marcas caiam no mesmo parágrafo (antes da diarização, "Participantes" é um só).
$libErrors = if ($lib) { @($lib.errors).Count } else { -1 }
Check ($null -ne $lib -and $lib.chips -eq 2 -and $lib.starred -in 1, 2 -and $libErrors -eq 0) "Biblioteca: chips=$($lib.chips) (esperado 2), paragrafos destacados=$($lib.starred) (esperado 1 ou 2), erros de JS=$libErrors"

if (-not $KeepMeeting) {
  Invoke-Isper 'delete_meeting' "{ id: $($row.id) }" | Out-Null
  foreach ($f in @($det.meeting.md_path, $docx)) { if ($f -and (Test-Path $f)) { Remove-Item $f -Force } }
  Check (@(Invoke-Isper 'list_meetings' '{ query: null }').Count -eq $before) "reuniao de teste e arquivos apagados"
} else {
  "reuniao de teste mantida (id $($row.id)): $($det.meeting.md_path)"
}

Invoke-Isper 'overlay_set_mode' "{ mode: `"$originalMode`" }" | Out-Null
Restart-IsperClean $exe
Check (-not (Test-Cdp)) "relancado sem porta CDP"
Finish-E2E 'meeting'
