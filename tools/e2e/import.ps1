# E2E da importacao de gravacoes (fase 9.0) no app real, via CDP: um MP3
# entregue a fila (como o botao "Importar audio" e o arrastar fazem) vira
# reuniao com o mesmo passe final, a origem no banco e no .md; o mesmo audio
# de novo aponta a reuniao que ja existe; um .opus e recusado na entrada; um
# OGG deixado na pasta vigiada vira reuniao com data e titulo do nome e muda
# para Importados; um "mp3" que nao e audio vai para "Nao importados" com o
# motivo. Precisa de um modelo Whisper instalado.
#
#   .\tools\e2e\import.ps1 -Exe <caminho do exe>
#
# A pasta vigiada do teste fica em %TEMP% (ISPER_IMPORT_DIR) e e apagada no
# fim; as reunioes de teste saem da Biblioteca pelo Desfazer e somem com os .md.
# ASCII puro de proposito: o PowerShell 5.1 le .ps1 sem BOM como ANSI.
param([string]$Exe)
. "$PSScriptRoot\common.ps1"

$exe = Get-IsperExe $Exe
"import: $exe"
$fixtures = (Resolve-Path (Join-Path $PSScriptRoot '..\..\fixtures\formatos')).Path
$dir = Join-Path $env:TEMP 'isper-import-e2e'
if (Test-Path $dir) { Remove-Item -Recurse -Force $dir }
New-Item -ItemType Directory -Force $dir | Out-Null
$created = @()

function Wait-ImportOf([string]$file, [int]$secs = 240) {
  # Espera a fila terminar este arquivo; devolve o ultimo resultado.
  for ($i = 0; $i -lt $secs; $i++) {
    $s = Invoke-Isper 'import_status'
    if ($s -and $s.last -and $s.last.file -eq $file -and -not $s.current) { return $s.last }
    Start-Sleep -Seconds 1
  }
  return $null
}

Stop-Isper
Check (Start-Isper -Exe $exe -Env @{ ISPER_IMPORT_DIR = $dir }) "app abriu (CDP) com a pasta vigiada de teste"
$st0 = Invoke-Isper 'home_status'
Check ($st0.engine.kind -eq 'ready') "motor pronto ($($st0.engine.kind))"
if ($st0.engine.kind -ne 'ready') { "sem motor carregado: abortando"; Finish-E2E 'import' }
$before = @(Invoke-Isper 'list_meetings' '{ query: null }').Count

# ---- escolhido/arrastado: MP3 44,1 kHz estereo
$mp3 = (Join-Path $fixtures 'fala-44k-stereo.mp3') -replace '\\', '/'
$opus = (Join-Path $fixtures 'fala-2s.opus') -replace '\\', '/'
$q = Invoke-Isper 'import_audio_files' "{ paths: ['$mp3', '$opus'] }"
Check ($q.queued -eq 1 -and @($q.rejected) -contains 'fala-2s.opus') "MP3 entra na fila e o .opus e recusado na entrada ($($q.queued) / $(@($q.rejected) -join ','))"
$last = Wait-ImportOf 'fala-44k-stereo.mp3'
Check ($null -ne $last -and $last.ok -and -not $last.duplicate -and $last.meeting_id) "MP3 virou reuniao (id $($last.meeting_id))"
if ($last -and $last.meeting_id) {
  $created += $last.meeting_id
  $det = Invoke-Isper 'get_meeting' "{ id: $($last.meeting_id) }"
  $texto = (@($det.segments) | ForEach-Object { $_.text }) -join ' '
  Check ($det.meeting.source_name -eq 'fala-44k-stereo.mp3') "a reuniao guarda de que arquivo veio ($($det.meeting.source_name))"
  Check (@($det.segments).Count -gt 0 -and $texto -match 'transcrevendo') "o passe final transcreveu o MP3 ('$texto')"
  Check (@($det.segments | Where-Object { $_.speaker -eq 'Eu' }).Count -eq 0) "gravacao importada nao tem canal Eu (so participantes)"
  $md = if ($det.meeting.md_path -and (Test-Path $det.meeting.md_path)) { Get-Content -Raw -Encoding UTF8 $det.meeting.md_path } else { '' }
  Check ($md -match 'fala-44k-stereo\.mp3') "o .md tem a linha da origem"

  # ---- o mesmo audio de novo: aponta a reuniao que ja existe
  $q2 = Invoke-Isper 'import_audio_files' "{ paths: ['$mp3'] }"
  $dup = Wait-ImportOf 'fala-44k-stereo.mp3' 60
  Check ($q2.queued -eq 1 -and $dup.duplicate -and $dup.meeting_id -eq $last.meeting_id) "o mesmo audio nao vira outra reuniao (aponta a $($dup.meeting_id))"
  Check (@(Invoke-Isper 'list_meetings' '{ query: null }').Count -eq $before + 1) "a Biblioteca tem uma reuniao a mais, nao duas"
}

# ---- pasta vigiada: OGG com data e titulo no nome
$nome = 'Reuniao com fornecedor 2026-09-22 15-30.ogg'
Copy-Item (Join-Path $fixtures 'fala-22k.ogg') (Join-Path $dir $nome)
$w = Wait-ImportOf $nome 240
Check ($null -ne $w -and $w.ok -and $w.meeting_id) "arquivo deixado na pasta vigiada virou reuniao (id $($w.meeting_id))"
if ($w -and $w.meeting_id) {
  $created += $w.meeting_id
  $wd = Invoke-Isper 'get_meeting' "{ id: $($w.meeting_id) }"
  Check ($wd.meeting.started_at -eq '22/09/2026 15:30') "a data vem do nome do arquivo ($($wd.meeting.started_at))"
  Check ($wd.meeting.title -like 'Reuniao com fornecedor*') "o nome descritivo vira o titulo ($($wd.meeting.title))"
}
Check ((Test-Path (Join-Path $dir "Importados\$nome")) -and -not (Test-Path (Join-Path $dir $nome))) "o arquivo foi para Importados (nada apagado)"

# ---- pasta vigiada: "mp3" que nao e audio
$ruim = 'nao-e-audio.mp3'
[IO.File]::WriteAllText((Join-Path $dir $ruim), ('isto nao e um mp3 ' * 200))
$r = Wait-ImportOf $ruim 90
Check ($null -ne $r -and -not $r.ok -and $r.message) "arquivo com defeito da erro que explica ('$($r.message)')"
$falhou = Get-ChildItem $dir -Directory | Where-Object { $_.Name -like 'N*o importados' } | Select-Object -First 1
$motivo = if ($falhou) { Join-Path $falhou.FullName 'nao-e-audio.motivo.txt' } else { '' }
Check ($falhou -and (Test-Path (Join-Path $falhou.FullName $ruim)) -and (Test-Path $motivo)) "o arquivo foi para 'Nao importados' com o motivo ao lado"

# ---- telas
Invoke-Isper 'open_library_window' '{ meeting: null }' | Out-Null
Wait-IsperWindow 'library.html' | Out-Null
Start-Sleep -Seconds 2
$lib = EvJson 'library.html' 'JSON.stringify({ chips: [...document.querySelectorAll("#list .badge")].filter(b => b.textContent === I18N.t("library.source.chip")).length, button: !!document.getElementById("importbtn"), strip: document.getElementById("imports").hidden })'
Check ($lib.button -and $lib.chips -ge 2) "Biblioteca: botao Importar audio e o selo nas reunioes importadas ($($lib.chips))"
$errs = Get-JsErrors 'library.html'
Check (@($errs).Count -eq 0) "library.html sem erros de JS$(Format-JsErrors $errs)"
Invoke-Isper 'open_settings_window' | Out-Null
Wait-IsperWindow 'settings.html' | Out-Null
Start-Sleep -Seconds 2
$set = EvJson 'settings.html' 'JSON.stringify({ watch: document.getElementById("importwatch").checked, dir: document.getElementById("importdir").textContent })'
Check ($set.watch -and $set.dir -eq $dir) "Configuracoes mostram a pasta vigiada ligada ($($set.dir))"
$errs = Get-JsErrors 'settings.html'
Check (@($errs).Count -eq 0) "settings.html sem erros de JS$(Format-JsErrors $errs)"

# ---- limpeza: pelo Desfazer, como o usuario faria; os .md vao junto
$undoMs = 0
foreach ($id in $created) {
  $sched = Invoke-Isper 'delete_meeting' "{ id: $id }"
  if ($sched.undo_ms -gt $undoMs) { $undoMs = [int]$sched.undo_ms }
}
if ($undoMs -gt 0) { Start-Sleep -Milliseconds ($undoMs + 1500) }
Check (@(Invoke-Isper 'list_meetings' '{ query: null }').Count -eq $before) "reunioes de teste e .md apagados"
Remove-Item -Recurse -Force $dir -ErrorAction SilentlyContinue
Restart-IsperClean -Exe $exe
Check (-not (Test-Cdp)) "relancado sem porta CDP"
Finish-E2E 'import'
