# A sincronia do celular com o PC (Fase 9.3) de ponta a ponta, num emulador
# Android, com o isper-cli fazendo o papel do PC (`isper-cli receber`):
#   1. colar o codigo de pareamento na Biblioteca e parear (o "PC" aprova);
#   2. gravar e parar: a gravacao vai sozinha para o PC (WorkManager) e chega
#      inteira, com o manifesto;
#   3. o "PC" termina a ata; "Enviar agora" a traz, a Biblioteca mostra
#      "Ata pronta" e a tela da ata abre com o titulo certo;
#   4. "Desconectar": o PC esquece o aparelho.
#
#   powershell -File tools\e2e\android-sync.ps1 [-Apk <app-debug.apk>] [-KeepEmulator]
#
# O emulador chega ao PC por 10.0.2.2 (o loopback da maquina), entao o codigo
# anuncia esse endereco. Precisa do isper-cli compilado
# (cargo build --release -p isper-cli --no-default-features basta).

param(
  [string]$Apk = '',
  [switch]$KeepEmulator,
  # Guarda a pasta do "PC" de teste (log do receber, gravacoes) para investigar.
  [switch]$KeepWork
)

. "$PSScriptRoot\android-common.ps1"

$cli = Join-Path $script:AndroidRoot 'target\release\isper-cli.exe'
if (-not (Test-Path $cli)) { throw "isper-cli nao encontrado: $cli (cargo build --release -p isper-cli --no-default-features)" }
$dir = "/sdcard/Android/data/$script:Pkg/files/Gravacoes"
$work = Join-Path ([IO.Path]::GetTempPath()) ('isper-e2e-android-sync-' + [guid]::NewGuid().ToString('N').Substring(0, 8))
$pcDir = Join-Path $work 'pc'
New-Item -ItemType Directory -Force $pcDir | Out-Null

function Get-UiText([string]$Tag) {
  Adb shell uiautomator dump /sdcard/isper-ui.xml 2>$null | Out-Null
  $xml = (Adb shell cat /sdcard/isper-ui.xml 2>$null) -join ''
  $m = [regex]::Match($xml, "<node[^>]*?text=""([^""]*)""[^>]*?resource-id=""$Tag""")
  if ($m.Success) { return [System.Net.WebUtility]::HtmlDecode($m.Groups[1].Value) }
  return $null
}

function Wait-UiText([string]$Tag, [scriptblock]$Cond = { param($t) $t }, [int]$Secs = 60) {
  $deadline = (Get-Date).AddSeconds($Secs)
  $t = $null
  while ((Get-Date) -lt $deadline) {
    $t = Get-UiText $Tag
    if ($null -ne $t -and (& $Cond $t)) { return $t }
    Start-Sleep -Milliseconds 800
  }
  return $t
}

# Digita num campo e confere: o teclado do emulador as vezes perde letras de
# um texto longo. Digita em pedacos, le o que ficou e, se nao bater, apaga e
# tenta de novo.
function Set-UiText([string]$Tag, [string]$Text) {
  for ($try = 1; $try -le 3; $try++) {
    if (-not (Invoke-UiTap -Tag $Tag)) { return $false }
    Adb shell input keycombination 113 29 | Out-Null   # Ctrl+A
    Adb shell input keyevent 67 | Out-Null             # apagar
    for ($i = 0; $i -lt $Text.Length; $i += 24) {
      $part = $Text.Substring($i, [Math]::Min(24, $Text.Length - $i))
      # Aspas simples: o shell do Android nao interpreta & e ? do codigo.
      Adb shell "input text '$part'" | Out-Null
      Start-Sleep -Milliseconds 250
    }
    Start-Sleep -Milliseconds 800
    if ((Get-UiText $Tag) -eq $Text) { return $true }
  }
  return $false
}

function Open-Tab([int]$Tab) {
  Adb shell am start -n "$script:Pkg/.MainActivity" --ei com.isper.mobile.ABA $Tab 2>$null | Out-Null
  Start-Sleep -Seconds 2
}

Start-IsperEmulator
Install-IsperApk -Apk $Apk
Adb shell am force-stop $script:Pkg | Out-Null
# Estado limpo: sem gravacoes e sem PC pareado (o APK de debug permite o run-as).
Adb shell rm -rf $dir 2>$null | Out-Null
Adb shell run-as $script:Pkg rm -rf files/sync 2>$null | Out-Null

# ---------------------------------------------------------------- o "PC"
$receiverLog = Join-Path $work 'receber.log'
$receiver = Start-Process -FilePath $cli -ArgumentList "receber `"$pcDir`" --aprovar --anunciar 10.0.2.2:47823 --validade 600" `
  -RedirectStandardOutput $receiverLog -RedirectStandardError "$receiverLog.err" -PassThru -WindowStyle Hidden
$null = $receiver.Handle
$codeFile = Join-Path $pcDir 'codigo.txt'
for ($i = 0; $i -lt 40 -and -not (Test-Path $codeFile); $i++) { Start-Sleep -Milliseconds 500 }
Check (Test-Path $codeFile) "o PC de teste (isper-cli receber) abriu o pareamento"
$code = (Get-Content -Raw $codeFile).Trim()

try {
  # ---------------------------------------------------------- 1. parear
  "== 1. colar o codigo e parear"
  Open-Tab 1
  Check (Invoke-UiTap -Tag 'colar-codigo') "botao Colar o codigo"
  Check (Set-UiText -Tag 'codigo' -Text $code) "o codigo foi colado inteiro ($($code.Length) caracteres)"
  Check (Invoke-UiTap -Tag 'usar-codigo') "Usar o codigo"
  Check (Invoke-UiTap -Tag 'confirmar-pareamento') "confirmou 'Parear com o PC?'"
  $connected = Wait-UiText 'pc-conectado' { param($t) $t -like 'Conectado a*' } 60
  Check ($connected -like 'Conectado a*') "pareado: '$connected'"
  $devices = Get-Content -Raw (Join-Path $pcDir 'aparelhos.json') -ErrorAction SilentlyContinue | ConvertFrom-Json
  Check (@($devices.PSObject.Properties).Count -eq 1) "o PC guardou o aparelho ($(@($devices.PSObject.Properties | ForEach-Object { $_.Value }) -join ', '))"

  # ------------------------------------------------ 2. gravar e mandar
  "== 2. gravar: a gravacao vai sozinha para o PC"
  Open-Tab 0
  Check (Invoke-UiTap -Tag 'gravar') "botao Gravar"
  Start-Sleep -Seconds 6
  Check (Invoke-UiTap -Tag 'parar') "botao Parar"
  $received = $null
  for ($i = 0; $i -lt 90 -and -not $received; $i++) {
    $received = Get-ChildItem -Recurse -Filter '*.opus' $pcDir -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not $received) { Start-Sleep -Seconds 1 }
  }
  Check ($null -ne $received) "a gravacao chegou ao PC ($($received.Name), $($received.Length) bytes)"
  if (-not $received) { throw "sem gravacao no PC: nada mais a testar" }
  $id = $received.BaseName
  $offer = Get-Content -Raw ([IO.Path]::ChangeExtension($received.FullName, 'offer.json')) | ConvertFrom-Json
  Check ($offer.size -eq $received.Length -and $offer.sha256.Length -eq 64) "com o manifesto: $($offer.size) bytes, SHA-256 e inicio $($offer.started_at)"
  $sync = (Adb shell cat "$dir/$id.sync.json" 2>$null) -join "`n" | ConvertFrom-Json
  Check ($sync.stage -eq 'queued') "no celular, a gravacao esta 'na fila do PC' ($($sync.stage))"

  # ------------------------------------------------------ 3. a ata volta
  "== 3. o PC termina a ata e ela volta"
  # Sem BOM, como o ISPer do PC escreve a ata (o Set-Content do PowerShell 5.1 poria um).
  [IO.File]::WriteAllText((Join-Path $received.DirectoryName "$id.ata.md"), "# Reuniao do e2e`n`n**Participante 1:** ola, tudo certo.`n", (New-Object System.Text.UTF8Encoding($false)))
  Open-Tab 1
  Check (Invoke-UiTap -Tag 'enviar-agora') "Enviar agora"
  $ready = Wait-UiText "remoto-$id" { param($t) $t -eq 'Ata pronta' } 90
  Check ($ready -eq 'Ata pronta') "a Biblioteca mostra '$ready'"
  $sync = (Adb shell cat "$dir/$id.sync.json" 2>$null) -join "`n" | ConvertFrom-Json
  Check ($sync.stage -eq 'ready' -and $sync.title -eq 'Reuniao do e2e') "manifesto da sincronia: $($sync.stage), '$($sync.title)'"
  Check (((Adb shell cat "$dir/$id.ata.md" 2>$null) -join "`n") -match 'ola, tudo certo') "a ata esta no celular"
  Check (Invoke-UiTap -Tag "ver-ata-$id") "Ver a ata"
  $title = Wait-UiText 'ata-titulo' { param($t) $t } 20
  Check ($title -eq 'Reuniao do e2e') "a tela da ata abriu: '$title'"
  Adb shell input keyevent KEYCODE_BACK | Out-Null

  # ------------------------------------------------------ 4. desconectar
  "== 4. desconectar"
  Open-Tab 1
  Check (Invoke-UiTap -Tag 'desconectar') "botao Desconectar"
  Check (Invoke-UiTap -Tag 'confirmar-desconectar') "confirmou 'Desconectar?'"
  $gone = $false
  for ($i = 0; $i -lt 30 -and -not $gone; $i++) {
    Start-Sleep -Seconds 1
    $devices = Get-Content -Raw (Join-Path $pcDir 'aparelhos.json') -ErrorAction SilentlyContinue | ConvertFrom-Json
    $gone = @($devices.PSObject.Properties).Count -eq 0
  }
  Check $gone "o PC esqueceu o aparelho"
  Check ($null -ne (Get-UiNode -Tag 'colar-codigo')) "a Biblioteca volta a oferecer o pareamento"
} finally {
  if (-not $receiver.HasExited) { Stop-Process -Id $receiver.Id -Force -Confirm:$false }
  if ($KeepWork) { "pasta do PC de teste mantida: $work" } else { Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue }
}

Stop-IsperEmulatorIfStarted -Keep:$KeepEmulator
Finish-Android -Name 'android-sync'
