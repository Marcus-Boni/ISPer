# O gravador do celular (Fase 9.2) de ponta a ponta, num emulador Android.
#
# Dirige a interface de verdade (uiautomator: as etiquetas testTag do Compose)
# e confere os manifestos que o gravador escreve ao lado de cada .opus:
#   1. gravar, marcar um momento e parar: manifesto "finished", duracao e
#      momento certos, .opus legivel;
#   2. gravar e matar o app a forca no meio (am force-stop, como um fabricante
#      matando o processo): na proxima abertura, a gravacao volta como
#      "recovered", com o audio ate a queda;
#   3. gravar com a tela apagada: a gravacao continua.
#
#   powershell -File tools\e2e\android-recorder.ps1 [-Apk <app-debug.apk>] [-KeepEmulator]
#
# O microfone do emulador entrega silencio (ou o do PC, se liberado); o teste
# mede o tempo, nao o conteudo. A qualidade do audio foi medida no corpus
# (ADR 0016).

param(
  [string]$Apk = '',
  [int]$Seconds = 12,
  [switch]$KeepEmulator
)

. "$PSScriptRoot\android-common.ps1"

$dir = "/sdcard/Android/data/$script:Pkg/files/Gravacoes"

function Get-Manifests {
  # So os manifestos: o .sync.json (Fase 9.3) e outra coisa.
  $names = @(Adb shell ls $dir 2>$null | Where-Object { $_ -match '\.json$' -and $_ -notmatch '\.sync\.json$' })
  $out = @()
  foreach ($n in $names) {
    $txt = (Adb shell cat "$dir/$($n.Trim())" 2>$null) -join "`n"
    if ($txt -match '^\s*\{') { $out += ($txt | ConvertFrom-Json) }
  }
  return $out
}

function Wait-State {
  # Espera um manifesto com o id e o estado pedidos (ou qualquer id novo).
  param([string]$State, [string[]]$Except = @(), [int]$TimeoutSec = 20)
  $deadline = (Get-Date).AddSeconds($TimeoutSec)
  while ((Get-Date) -lt $deadline) {
    $m = Get-Manifests | Where-Object { $_.state -eq $State -and $Except -notcontains $_.id } | Select-Object -First 1
    if ($m) { return $m }
    Start-Sleep -Milliseconds 800
  }
  return $null
}

Start-IsperEmulator
Install-IsperApk -Apk $Apk
Adb shell am force-stop $script:Pkg | Out-Null
Adb shell rm -rf $dir 2>$null | Out-Null

# ------------------------------------------------------------- 1. normal
"== 1. gravar, marcar e parar ($Seconds s)"
Start-IsperApp
Check (Invoke-UiTap -Tag 'gravar') "botao Gravar tocado"
$t0 = Get-Date
$rec = Wait-State -State 'recording'
Check ($null -ne $rec) "manifesto em 'recording' desde o primeiro segundo"
$fgs = (Adb shell dumpsys activity services $script:Pkg 2>$null) -join "`n"
Check ($fgs -match 'isForeground=true') "servico em primeiro plano ativo"
Start-Sleep -Seconds ([int]($Seconds / 2))
Check (Invoke-UiTap -Tag 'marcar') "momento marcado"
Start-Sleep -Seconds ([int]($Seconds / 2))
Check (Invoke-UiTap -Tag 'parar') "botao Parar tocado"
# O toque pelo uiautomator leva segundos no emulador: a referencia e o
# relogio entre os dois toques, nao o $Seconds.
$wall = ((Get-Date) - $t0).TotalSeconds
$done = Wait-State -State 'finished'
Check ($null -ne $done) "manifesto 'finished'"
if ($done) {
  Check ([math]::Abs($done.duration_secs - $wall) -lt 3) ("duracao {0:N1} s para {1:N1} s entre os toques" -f $done.duration_secs, $wall)
  Check (@($done.moments).Count -eq 1) "1 momento no manifesto ($(@($done.moments) -join ', ') s)"
  $size = (Adb shell stat -c %s "$dir/$($done.audio_file)" 2>$null) -join ''
  Check ([int64]$size -gt 1000) "arquivo .opus com $size bytes"
  $local = Join-Path $script:AndroidRoot "target\android-rec-$($done.id).opus"
  Adb pull "$dir/$($done.audio_file)" $local 2>$null | Out-Null
  $ffprobe = Get-Command ffprobe -ErrorAction SilentlyContinue
  if ($ffprobe -and (Test-Path $local)) {
    $probe = & $ffprobe.Source -v error -show_entries 'format=duration:stream=codec_name' -of default=nw=1 $local 2>&1 | Out-String
    Check ($probe -match 'codec_name=opus') "o ffprobe le o arquivo como Opus ($(($probe -replace '\s+', ' ').Trim()))"
  }
}
$firstId = if ($done) { $done.id } else { '' }

# ------------------------------------------------------------- 2. queda
"== 2. gravar e matar o app no meio"
Start-Sleep -Seconds 1 # ids sao por segundo
Start-IsperApp
Check (Invoke-UiTap -Tag 'gravar') "botao Gravar tocado"
$t0 = Get-Date
$rec2 = Wait-State -State 'recording' -Except @($firstId)
Check ($null -ne $rec2) "segunda gravacao em 'recording'"
Start-Sleep -Seconds $Seconds
Adb shell am force-stop $script:Pkg | Out-Null
$wall = ((Get-Date) - $t0).TotalSeconds
Start-Sleep -Seconds 2
$still = Get-Manifests | Where-Object { $_.id -eq $rec2.id } | Select-Object -First 1
Check ($still.state -eq 'recording') "depois da queda o manifesto ficou em 'recording' (ninguem fechou)"
Start-IsperApp
$recovered = Wait-State -State 'recovered'
Check ($null -ne $recovered) "ao abrir de novo, a gravacao voltou como 'recovered'"
if ($recovered) {
  $d = [double]$recovered.duration_secs
  # Perde no maximo a pagina em andamento (1 s) e o que o force-stop levou
  # para chegar.
  Check ($d -ge ($wall - 3) -and $d -le ($wall + 1)) ("audio recuperado: {0:N1} s de {1:N1} s gravados ate a queda" -f $d, $wall)
}

# ------------------------------------------------------------- 3. tela apagada
"== 3. gravar com a tela apagada"
Start-Sleep -Seconds 1
Start-IsperApp
Check (Invoke-UiTap -Tag 'gravar') "botao Gravar tocado"
$t0 = Get-Date
$rec3 = Wait-State -State 'recording' -Except @($firstId, $rec2.id)
Adb shell svc power stayon false | Out-Null
Adb shell input keyevent KEYCODE_SLEEP | Out-Null
Start-Sleep -Seconds $Seconds
Adb shell input keyevent KEYCODE_WAKEUP | Out-Null
Adb shell svc power stayon true | Out-Null
Adb shell wm dismiss-keyguard | Out-Null
Start-Sleep -Seconds 1
Check (Invoke-UiTap -Tag 'parar') "botao Parar tocado depois de acordar"
$wall = ((Get-Date) - $t0).TotalSeconds
$done3 = Wait-State -State 'finished' -Except @($firstId)
Check ($null -ne $done3) "terceira gravacao 'finished'"
if ($done3) {
  Check ([math]::Abs($done3.duration_secs - $wall) -lt 3) ("gravou com a tela apagada: {0:N1} s para {1:N1} s entre os toques" -f $done3.duration_secs, $wall)
}

Stop-IsperEmulatorIfStarted -Keep:$KeepEmulator
Finish-Android -Name 'android-recorder'
