# Utilitarios dos testes ponta a ponta do app Android (dot-source:
# . "$PSScriptRoot\android-common.ps1"). Sobem o emulador, instalam o APK e
# dirigem a interface pelo uiautomator (as etiquetas testTag do Compose viram
# resource-id). Cada verificacao passa por Check; Finish-Android resume e
# devolve codigo de saida 1 se algo falhou.
#
# Texto so em ASCII: o PowerShell 5.1 le .ps1 sem BOM como ANSI.

# 'Continue': no PowerShell 5.1, um exe nativo que escreve no stderr vira erro
# fatal com 'Stop'. Os erros que importam sao tratados um a um.
$ErrorActionPreference = 'Continue'

$script:AndroidRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$script:Sdk = if ($env:ANDROID_HOME) { $env:ANDROID_HOME } else { Join-Path $env:LOCALAPPDATA 'Android\Sdk' }
$script:AdbExe = Join-Path $script:Sdk 'platform-tools\adb.exe'
$script:EmulatorExe = Join-Path $script:Sdk 'emulator\emulator.exe'
$script:AvdManager = Join-Path $script:Sdk 'cmdline-tools\latest\bin\avdmanager.bat'
$script:Pkg = 'com.isper.mobile'
$script:Checks = 0
$script:Failures = 0
$script:StartedEmulator = $false

foreach ($t in $script:AdbExe, $script:EmulatorExe, $script:AvdManager) {
  if (-not (Test-Path $t)) { throw "ferramenta do SDK ausente: $t" }
}

function Adb { & $script:AdbExe @args }

function Check {
  param([bool]$Ok, [string]$What)
  $script:Checks++
  if ($Ok) { "  OK     $What" } else { $script:Failures++; "  FALHA  $What" }
}

function Start-Detached([string]$CommandLine) {
  # Um processo que nao e filho deste script (criado pelo servico do WMI).
  $r = Invoke-CimMethod -ClassName Win32_Process -MethodName Create -Arguments @{ CommandLine = $CommandLine }
  if ($r.ReturnValue -ne 0) { throw "nao consegui iniciar: $CommandLine (Win32_Process.Create = $($r.ReturnValue))" }
}

function Start-IsperEmulator {
  param(
    [string]$Avd = 'isper-lab',
    [string]$Image = 'system-images;android-36;google_apis_playstore;x86_64'
  )
  $avds = & $script:EmulatorExe -list-avds
  if ($avds -notcontains $Avd) {
    "criando o AVD ($Image)"
    'no' | & $script:AvdManager create avd -n $Avd -k $Image -d pixel_7 --force | Out-Null
  }
  # O servidor do adb e o emulador continuam rodando depois do script. Como
  # filhos dele, herdariam os handles e segurariam aberto o pipe de quem
  # chamou: um `android-recorder.ps1 | tail` nunca terminava. Pelo WMI, eles
  # nascem sem herdar nada.
  if (-not (Get-Process adb -ErrorAction SilentlyContinue)) {
    Start-Detached "`"$($script:AdbExe)`" start-server"
    Start-Sleep -Seconds 2
  }
  if (-not ((Adb devices) -match 'emulator-\d+\s+device')) {
    "subindo o emulador (sem janela)"
    $argLine = @('-avd', $Avd, '-no-window', '-no-audio', '-no-boot-anim', '-no-snapshot-save', '-gpu', 'swiftshader_indirect', '-memory', '4096', '-cores', '4') -join ' '
    Start-Detached "`"$($script:EmulatorExe)`" $argLine"
    $script:StartedEmulator = $true
  }
  Adb wait-for-device | Out-Null
  $deadline = (Get-Date).AddMinutes(5)
  while ((Adb shell getprop sys.boot_completed 2>$null) -notmatch '1') {
    if ((Get-Date) -gt $deadline) { throw 'o emulador nao terminou de iniciar em 5 min' }
    Start-Sleep -Seconds 3
  }
  # Tela sempre acesa e desbloqueada: o uiautomator precisa ver a interface.
  Adb shell svc power stayon true | Out-Null
  Adb shell input keyevent KEYCODE_WAKEUP | Out-Null
  Adb shell wm dismiss-keyguard | Out-Null
  Check $true "emulador pronto ($(Adb shell getprop ro.product.model) - Android $(Adb shell getprop ro.build.version.release))"
}

function Install-IsperApk {
  param([string]$Apk = '')
  if (-not $Apk) { $Apk = Join-Path $script:AndroidRoot 'apps\isper-android\app\build\outputs\apk\debug\app-debug.apk' }
  if (-not (Test-Path $Apk)) { throw "APK nao encontrado: $Apk (rode ./gradlew assembleDebug em apps/isper-android)" }
  "== instalando $([IO.Path]::GetFileName($Apk)) ($([math]::Round((Get-Item $Apk).Length / 1MB)) MB)"
  # -g concede as permissoes de execucao (microfone, notificacoes).
  $out = Adb install -r -g $Apk 2>&1 | Out-String
  Check ($out -match 'Success') "APK instalado"
}

function Get-UiNode {
  # Procura na tela atual um no com o resource-id (testTag) pedido e devolve o
  # centro dele, ou $null.
  param([Parameter(Mandatory)][string]$Tag)
  Adb shell uiautomator dump /sdcard/isper-ui.xml 2>$null | Out-Null
  $xml = (Adb shell cat /sdcard/isper-ui.xml 2>$null) -join ''
  # Um emulador recem-ligado costuma mostrar "System UI isn't responding";
  # "Wait" dispensa o aviso sem matar nada.
  $anr = [regex]::Match($xml, 'resource-id="android:id/aerr_wait"[^>]*?bounds="\[(\d+),(\d+)\]\[(\d+),(\d+)\]"')
  if ($anr.Success) {
    $ax = ([int]$anr.Groups[1].Value + [int]$anr.Groups[3].Value) / 2
    $ay = ([int]$anr.Groups[2].Value + [int]$anr.Groups[4].Value) / 2
    Adb shell input tap ([int]$ax) ([int]$ay) | Out-Null
    return $null
  }
  $m = [regex]::Match($xml, "resource-id=""$Tag""[^>]*?bounds=""\[(\d+),(\d+)\]\[(\d+),(\d+)\]""")
  if (-not $m.Success) { return $null }
  $x = ([int]$m.Groups[1].Value + [int]$m.Groups[3].Value) / 2
  $y = ([int]$m.Groups[2].Value + [int]$m.Groups[4].Value) / 2
  return @{ X = [int]$x; Y = [int]$y }
}

function Invoke-UiTap {
  # Espera o no aparecer (ate $TimeoutSec) e toca nele.
  param([Parameter(Mandatory)][string]$Tag, [int]$TimeoutSec = 40)
  $deadline = (Get-Date).AddSeconds($TimeoutSec)
  while ((Get-Date) -lt $deadline) {
    $n = Get-UiNode -Tag $Tag
    if ($n) {
      Adb shell input tap $n.X $n.Y | Out-Null
      return $true
    }
    Start-Sleep -Milliseconds 700
  }
  return $false
}

function Start-IsperApp {
  Adb shell am start -n "$script:Pkg/.MainActivity" 2>$null | Out-Null
  Start-Sleep -Seconds 2
}

function Stop-IsperEmulatorIfStarted {
  param([switch]$Keep)
  if ($script:StartedEmulator -and -not $Keep) { Adb emu kill | Out-Null }
}

function Finish-Android {
  param([Parameter(Mandatory)][string]$Name)
  ""
  if ($script:Failures -eq 0) {
    "$Name`: $($script:Checks) verificacoes, todas OK"
  } else {
    "$Name`: $($script:Failures) de $($script:Checks) verificacoes FALHARAM"
    exit 1
  }
}
