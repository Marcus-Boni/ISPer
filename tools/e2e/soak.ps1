<#
.SYNOPSIS
  Soak test: reuniao longa no ISPer real, medindo a memoria do processo.
.DESCRIPTION
  Sobe o exe com a porta CDP, inicia uma reuniao, toca a fixture de duas vozes em
  loop nos alto-falantes (o loopback captura e transcreve em blocos, como numa
  reuniao real) e, a cada -SampleSeconds, anota o Working Set e a memoria privada
  do isper-app, as falas transcritas ao vivo e o tamanho dos .pcm temporarios
  (%TEMP%\ISPer). No fim encerra a reuniao, espera salvar (e a diarizacao, se
  houver), confere que os .pcm sumiram, apaga a reuniao de teste (salvo
  -KeepMeeting) e imprime a tabela.

  O que prova: o audio dos participantes vai para DISCO durante a reuniao, entao a
  memoria de uma reuniao de 2 h deve ficar estavel depois do aquecimento -- nao
  subir ~115 MB por hora como quando ficava em RAM. Falha se a memoria privada
  cresceu mais que -MaxGrowthMB entre o fim do aquecimento e o fim.

  ATENCAO: toca audio nos alto-falantes e o loopback captura tudo que estiver
  tocando no PC; o titulo/resumo vai ao provider de IA configurado, como numa
  reuniao normal. Reserve a maquina: 2 h de reuniao sao 2 h de teste.
.EXAMPLE
  .\tools\e2e\soak.ps1 -Minutes 10                                   # ensaio rapido
  .\tools\e2e\soak.ps1 -Minutes 120 -Exe .\target\release\isper-app.exe   # as 2 h
#>
param(
  [string]$Exe,
  [int]$Minutes = 10,
  [int]$SampleSeconds = 30,
  [int]$WarmupMinutes = 2,
  [int]$MaxGrowthMB = 150,
  [switch]$NoAudio,
  [switch]$KeepMeeting
)
. "$PSScriptRoot\common.ps1"
$exe = Get-IsperExe $Exe
$fixture = Join-Path $script:E2ERoot 'fixtures\duas-vozes-16k.wav'
if (-not $NoAudio -and -not (Test-Path $fixture)) { throw "fixture nao encontrada: $fixture" }
if ($WarmupMinutes -ge $Minutes) { $WarmupMinutes = [Math]::Max(0, $Minutes - 1) }
$pcmDir = Join-Path $env:TEMP 'ISPer'
"soak: $exe ($Minutes min, amostra a cada $SampleSeconds s, aquecimento $WarmupMinutes min)"

function Get-PcmFiles { @(Get-ChildItem $pcmDir -Filter '*.pcm' -ErrorAction SilentlyContinue) }

Stop-Isper
Check (Start-Isper -Exe $exe) "app abriu (CDP)"
$st0 = Invoke-Isper 'home_status'
Check ($st0.engine.kind -eq 'ready') "motor pronto ($($st0.engine.kind))"
if ($st0.engine.kind -ne 'ready') { "sem motor carregado: abortando"; Finish-E2E 'soak' }
$before = @(Invoke-Isper 'list_meetings' '{ query: null }').Count
$pcmBefore = (Get-PcmFiles).Count

$r = Invoke-Isper 'toggle_meeting_cmd'
Check ($null -eq $r -or -not $r.__error) "reuniao iniciada"
$player = $null
if (-not $NoAudio) {
  $player = New-Object System.Media.SoundPlayer $fixture
  $player.PlayLooping()
}

$samples = New-Object System.Collections.Generic.List[object]
$started = Get-Date
$deadline = $started.AddMinutes($Minutes)
$pcmSeen = $false
"  {0,7}  {1,10}  {2,10}  {3,6}  {4,8}" -f 'tempo', 'WS (MB)', 'priv (MB)', 'falas', 'pcm (MB)'
while ((Get-Date) -lt $deadline) {
  Start-Sleep -Seconds $SampleSeconds
  $p = Get-Process -Name isper-app -ErrorAction SilentlyContinue | Select-Object -First 1
  if (-not $p) { Check $false "processo isper-app sumiu durante o soak"; break }
  $elapsed = [int]((Get-Date) - $started).TotalSeconds
  $ws = [math]::Round($p.WorkingSet64 / 1MB, 1)
  $priv = [math]::Round($p.PrivateMemorySize64 / 1MB, 1)
  $live = @(Invoke-Isper 'live_transcript').Count
  $pcm = Get-PcmFiles
  if ($pcm.Count -gt $pcmBefore) { $pcmSeen = $true }
  $pcmMB = [math]::Round((($pcm | Measure-Object -Property Length -Sum).Sum) / 1MB, 1)
  $samples.Add([pscustomobject]@{ s = $elapsed; workingSetMB = $ws; privateMB = $priv; falas = $live; pcmMB = $pcmMB })
  "  {0,6} s  {1,10}  {2,10}  {3,6}  {4,8}" -f $elapsed, $ws, $priv, $live, $pcmMB
}
if ($player) { $player.Stop() }

$r = Invoke-Isper 'toggle_meeting_cmd'
Check ($null -eq $r -or -not $r.__error) "reuniao encerrada"
$saved = $false
for ($i = 0; $i -lt 150; $i++) {
  Start-Sleep -Seconds 2
  if (-not (Invoke-Isper 'home_status').meeting_active) { $saved = $true; break }
}
Check $saved "reuniao salva (apos $(($i + 1) * 2) s)"

# Analise da memoria: crescimento entre o fim do aquecimento e o fim.
$outDir = Join-Path $script:E2ERoot 'target\soak'
New-Item -ItemType Directory -Force $outDir | Out-Null
$csv = Join-Path $outDir ("soak-{0}.csv" -f (Get-Date -Format yyyyMMdd-HHmmss))
$samples | Export-Csv -Path $csv -NoTypeInformation -Encoding UTF8
"amostras em $csv"
Check ($samples.Count -ge 2) "$($samples.Count) amostras coletadas"
if ($samples.Count -ge 2) {
  $after = @($samples | Where-Object { $_.s -ge $WarmupMinutes * 60 })
  if ($after.Count -lt 2) { $after = @($samples) }
  $first = $after[0]
  $last = $after[-1]
  $growth = [math]::Round($last.privateMB - $first.privateMB, 1)
  $peak = ($samples | Measure-Object -Property privateMB -Maximum).Maximum
  "memoria privada apos o aquecimento: $($first.privateMB) MB -> $($last.privateMB) MB (crescimento $growth MB; pico $peak MB)"
  Check ($growth -le $MaxGrowthMB) "crescimento de memoria apos o aquecimento <= $MaxGrowthMB MB (medido: $growth MB)"
  if (-not $NoAudio) { Check ($last.falas -gt 0) "houve transcricao ao vivo ($($last.falas) falas)" }
}
if (-not $NoAudio) { Check $pcmSeen "audio dos participantes foi para disco durante a reuniao ($pcmDir\*.pcm)" }

# O .pcm vive ate a diarizacao (em segundo plano) terminar: espera por ela.
$diarizing = $true
for ($i = 0; $i -lt ($Minutes * 30); $i++) {
  if ($null -eq (Invoke-Isper 'home_status').diarizing_meeting) { $diarizing = $false; break }
  Start-Sleep -Seconds 2
}
if ($diarizing) {
  "diarizacao ainda em andamento: os .pcm somem quando ela terminar (nao conferido)"
} else {
  $left = (Get-PcmFiles).Count - $pcmBefore
  Check ($left -le 0) "arquivos .pcm temporarios apagados depois da reuniao ($left a mais que antes)"
}

$rows = @(Invoke-Isper 'list_meetings' '{ query: null }')
Check ($rows.Count -eq $before + 1) "uma reuniao nova no historico ($before -> $($rows.Count))"
if ($rows.Count -gt $before) {
  $row = $rows[0]
  if ($KeepMeeting) {
    "reuniao do soak mantida (id $($row.id))"
  } else {
    $det = Invoke-Isper 'get_meeting' "{ id: $($row.id) }"
    Invoke-Isper 'delete_meeting' "{ id: $($row.id) }" | Out-Null
    if ($det.meeting.md_path -and (Test-Path $det.meeting.md_path)) { Remove-Item $det.meeting.md_path -Force }
    Check (@(Invoke-Isper 'list_meetings' '{ query: null }').Count -eq $before) "reuniao do soak apagada"
  }
}

Restart-IsperClean $exe
Check (-not (Test-Cdp)) "relancado sem porta CDP"
Finish-E2E 'soak'
