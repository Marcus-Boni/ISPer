# Laboratorio do celular (Fase 9.1) de ponta a ponta, num emulador Android.
#
# Sobe um AVD x86_64 (cria se nao existir), instala o APK de debug, roda o
# laboratorio sem tocar na tela (intent "autorun" com o modelo tiny) e confere
# o relatorio: tempos, fator de tempo real, transcricao, falantes e, quando a
# amostra tem referencia, WER e DER.
#
#   powershell -File tools\e2e\android-lab.ps1 [-Apk <app-debug.apk>] [-Model ggml-tiny-q5_1.bin] [-KeepEmulator]
#
# O emulador mede a CORRETUDE (o mesmo pipeline do PC roda no Android), nao o
# desempenho de um celular: numeros de velocidade valem so em aparelho real
# (apps/isper-android/README.md -> "Rodar o spike num celular").

param(
  [string]$Apk = '',
  [string]$Model = 'ggml-tiny-q5_1.bin',
  [string]$Avd = 'isper-lab',
  [string]$Image = 'system-images;android-36;google_apis_playstore;x86_64',
  [int]$TimeoutMinutes = 20,
  [switch]$KeepEmulator
)

# 'Continue': no PowerShell 5.1, um exe nativo que escreve no stderr (o adb
# escreve "No such file" enquanto o relatorio nao existe) vira erro fatal com
# 'Stop'. Os erros que importam sao tratados um a um.
$ErrorActionPreference = 'Continue'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
if (-not $Apk) { $Apk = Join-Path $root 'apps\isper-android\app\build\outputs\apk\debug\app-debug.apk' }
if (-not (Test-Path $Apk)) { throw "APK nao encontrado: $Apk (rode ./gradlew assembleDebug em apps/isper-android)" }

$sdk = if ($env:ANDROID_HOME) { $env:ANDROID_HOME } else { Join-Path $env:LOCALAPPDATA 'Android\Sdk' }
$adb = Join-Path $sdk 'platform-tools\adb.exe'
$emulator = Join-Path $sdk 'emulator\emulator.exe'
$avdmanager = Join-Path $sdk 'cmdline-tools\latest\bin\avdmanager.bat'
foreach ($t in $adb, $emulator, $avdmanager) { if (-not (Test-Path $t)) { throw "ferramenta do SDK ausente: $t" } }

$script:Checks = 0
$script:Failures = 0
function Check {
  param([bool]$Ok, [string]$What)
  $script:Checks++
  if ($Ok) { "  OK     $What" } else { $script:Failures++; "  FALHA  $What" }
}

function Adb { & $adb @args }

"== AVD $Avd"
$avds = & $emulator -list-avds
if ($avds -notcontains $Avd) {
  "criando o AVD ($Image)"
  'no' | & $avdmanager create avd -n $Avd -k $Image -d pixel_7 --force | Out-Null
}

$started = $false
$running = (Adb devices) -match 'emulator-\d+\s+device'
if (-not $running) {
  "subindo o emulador (sem janela)"
  $emuProc = Start-Process $emulator -ArgumentList @('-avd', $Avd, '-no-window', '-no-audio', '-no-boot-anim', '-no-snapshot-save', '-gpu', 'swiftshader_indirect', '-memory', '4096', '-cores', '4') -PassThru -WindowStyle Hidden
  $started = $true
}
Adb wait-for-device | Out-Null
$deadline = (Get-Date).AddMinutes(5)
while ((Adb shell getprop sys.boot_completed 2>$null) -notmatch '1') {
  if ((Get-Date) -gt $deadline) { throw 'o emulador nao terminou de iniciar em 5 min' }
  Start-Sleep -Seconds 3
}
Check $true "emulador pronto ($(Adb shell getprop ro.product.model) - Android $(Adb shell getprop ro.build.version.release))"

"== instalando $([IO.Path]::GetFileName($Apk)) ($([math]::Round((Get-Item $Apk).Length / 1MB)) MB)"
$install = Adb install -r -g $Apk 2>&1 | Out-String
Check ($install -match 'Success') "APK instalado"

$pkg = 'com.isper.mobile'
$report = "/sdcard/Android/data/$pkg/files/spike-report.json"
Adb shell am force-stop $pkg | Out-Null
Adb shell rm -f $report | Out-Null
Adb logcat -c | Out-Null

"== laboratorio: $Model, com diarizacao"
Adb shell am start -n "$pkg/.MainActivity" --ez autorun true --es model $Model --ez diarize true | Out-Null

$deadline = (Get-Date).AddMinutes($TimeoutMinutes)
$json = $null
while ((Get-Date) -lt $deadline) {
  Start-Sleep -Seconds 5
  $text = (Adb shell cat $report 2>$null) -join "`n"
  if ($text -match '^\s*\{') { $json = $text; break }
  $crash = (Adb logcat -d -s AndroidRuntime:E 2>$null) -join "`n"
  if ($crash -match 'FATAL EXCEPTION') { "o app caiu:"; $crash; break }
}
Check ($null -ne $json) "relatorio gravado em $report"

if ($json) {
  $r = $json | ConvertFrom-Json
  if ($r.erro) {
    Check $false "rodada sem erro ($($r.erro))"
  } else {
    Check ($r.modelo -eq $Model) "modelo do relatorio ($($r.modelo))"
    Check ($r.duracao_audio_s -gt 1) "audio lido ($($r.duracao_audio_s) s)"
    Check ($r.fator_tempo_real -gt 0) "fator de tempo real medido ($($r.fator_tempo_real))"
    Check ($r.inicio_da_transcricao.Length -gt 20) "transcricao nao vazia: '$($r.inicio_da_transcricao.Substring(0, [math]::Min(80, $r.inicio_da_transcricao.Length)))...'"
    Check ($r.falantes -ge 1) "falantes encontrados ($($r.falantes))"
    Check ($r.pico_memoria_mb -gt 0) "pico de memoria ($([math]::Round($r.pico_memoria_mb)) MB)"
    Check ($r.pipeline.config -eq 'final') "o mesmo passe final do PC ($($r.pipeline.config), $($r.pipeline.params.profile))"
    if ($null -ne $r.wer) {
      Check ($r.wer -lt 0.6) ("WER {0:P1} com o modelo {1} (o tiny erra muito; aqui so se confere que a medida funciona)" -f $r.wer, $Model)
      Check ($null -ne $r.der) ("DER {0:P1}" -f $r.der)
    }
    ""
    "tempos (s): leitura $($r.tempos_s.leitura) | carga $($r.tempos_s.carga) | transcricao $($r.tempos_s.transcricao) | falantes $($r.tempos_s.falantes) | total $($r.tempos_s.total)"
    $out = Join-Path $root 'target\android-lab-report.json'
    New-Item -ItemType Directory -Force (Split-Path $out) | Out-Null
    [IO.File]::WriteAllText($out, $json)
    "relatorio completo: $out"
  }
}

if ($started -and -not $KeepEmulator) {
  Adb emu kill | Out-Null
}

""
if ($script:Failures -eq 0) {
  "android-lab: $($script:Checks) verificacoes, todas OK"
} else {
  "android-lab: $($script:Failures) de $($script:Checks) verificacoes FALHARAM"
  exit 1
}
