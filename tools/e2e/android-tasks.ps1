# As tarefas no bolso (Fase 10.6) de ponta a ponta, num emulador Android, com
# o isper-cli fazendo o papel do PC (`isper-cli receber --banco`):
#   1. parear (como no android-sync.ps1);
#   2. na aba Hoje, escrever "amanha as 15h ligar pro contador": a previa
#      mostra o dia e a hora, a tarefa aparece na hora e, na sincronia, chega
#      ao isper.db do PC com o dia e a hora entendidos (o criterio da 10.6);
#   3. concluir no celular: a conclusao chega ao PC;
#   4. uma tarefa para daqui a 2 minutos: o lembrete aparece na hora, com
#      "Concluir".
#
#   powershell -File tools\e2e\android-tasks.ps1 [-Apk <app-debug.apk>] [-KeepEmulator]
#
# Ditar nao entra aqui: o emulador nao tem o modelo do Whisper nem microfone
# de verdade (o ditado e o mesmo Whisper do PC, testado no nucleo).
#
# ASCII puro de proposito: o PowerShell 5.1 le .ps1 sem BOM como ANSI.

param(
  [string]$Apk = '',
  [switch]$KeepEmulator,
  [switch]$KeepWork
)

. "$PSScriptRoot\android-common.ps1"

$cli = Join-Path $script:AndroidRoot 'target\release\isper-cli.exe'
if (-not (Test-Path $cli)) { throw "isper-cli nao encontrado: $cli (cargo build --release -p isper-cli --no-default-features)" }
$work = Join-Path ([IO.Path]::GetTempPath()) ('isper-e2e-android-tasks-' + [guid]::NewGuid().ToString('N').Substring(0, 8))
$pcDir = Join-Path $work 'pc'
$db = Join-Path $work 'isper.db'
New-Item -ItemType Directory -Force $pcDir | Out-Null

function Set-UiText([string]$Tag, [string]$Text) {
  # Num emulador recem-ligado, o teclado ainda se configura na primeira vez e
  # engole o que chega: mais tentativas, com uma pausa depois do toque.
  for ($try = 1; $try -le 5; $try++) {
    if (-not (Invoke-UiTap -Tag $Tag)) { return $false }
    Start-Sleep -Milliseconds (500 * $try)
    Adb shell input keycombination 113 29 | Out-Null   # Ctrl+A
    Adb shell input keyevent 67 | Out-Null             # apagar
    for ($i = 0; $i -lt $Text.Length; $i += 24) {
      $part = $Text.Substring($i, [Math]::Min(24, $Text.Length - $i)).Replace(' ', '%s')
      Adb shell "input text '$part'" | Out-Null
      Start-Sleep -Milliseconds 250
    }
    Start-Sleep -Milliseconds 800
    if ((Get-UiText $Tag) -eq $Text) { return $true }
  }
  return $false
}

# O que o "PC" recebeu, linha a linha ("tarefa do celular <aparelho>: <id> ...").
function Wait-ReceiverLine([string]$Like, [int]$Secs = 90) {
  for ($i = 0; $i -lt $Secs; $i++) {
    $line = Get-Content $receiverLog -ErrorAction SilentlyContinue | Where-Object { $_ -like $Like } | Select-Object -Last 1
    if ($line) { return $line }
    Start-Sleep -Seconds 1
  }
  return $null
}

Start-IsperEmulator
Install-IsperApk -Apk $Apk
Adb shell am force-stop $script:Pkg | Out-Null
Adb shell run-as $script:Pkg rm -rf files/sync 2>$null | Out-Null
Adb shell pm grant $script:Pkg android.permission.POST_NOTIFICATIONS 2>$null | Out-Null

function Get-FreeUdpPort {
  $udp = New-Object System.Net.Sockets.UdpClient 0
  try { return $udp.Client.LocalEndPoint.Port } finally { $udp.Close() }
}
$port = Get-FreeUdpPort
$receiverLog = Join-Path $work 'receber.log'
$receiver = Start-Process -FilePath $cli -ArgumentList "receber `"$pcDir`" --porta $port --aprovar --anunciar 10.0.2.2:$port --validade 600 --banco `"$db`"" `
  -RedirectStandardOutput $receiverLog -RedirectStandardError "$receiverLog.err" -PassThru -WindowStyle Hidden
$null = $receiver.Handle
$codeFile = Join-Path $pcDir 'codigo.txt'
for ($i = 0; $i -lt 40 -and -not (Test-Path $codeFile); $i++) { Start-Sleep -Milliseconds 500 }
Check (Test-Path $codeFile) "o PC de teste (isper-cli receber --banco) abriu o pareamento"
$code = (Get-Content -Raw $codeFile).Trim()

try {
  "== 1. parear"
  Open-Tab 1
  Check (Invoke-UiTap -Tag 'colar-codigo') "botao Colar o codigo"
  Check (Set-UiText -Tag 'codigo' -Text $code) "o codigo foi colado inteiro"
  Check (Invoke-UiTap -Tag 'usar-codigo') "Usar o codigo"
  Check (Invoke-UiTap -Tag 'confirmar-pareamento') "confirmou 'Parear com o PC?'"
  $connected = Wait-UiText 'pc-conectado' { param($t) $t -like 'Conectado a*' } 60
  Check ($connected -like 'Conectado a*') "pareado: '$connected'"

  "== 2. uma tarefa escrita no celular chega ao PC"
  Open-Tab 2
  $status = Wait-UiText 'hoje-sincronia' { param($t) $t -like 'Atualizado*' } 60
  Check ($status -like 'Atualizado com o PC*') "a aba Hoje ja sincronizou com o PC ('$status')"
  Check (Set-UiText -Tag 'hoje-campo' -Text 'amanha as 15h ligar pro contador') "escreveu a tarefa"
  Check ($null -ne (Get-UiNode -Tag 'hoje-adicionar')) "a previa oferece Adicionar"
  Check (Invoke-UiTap -Tag 'hoje-adicionar') "Adicionar"
  # A de amanha fica em "Depois", recolhido.
  Check (Invoke-UiTap -Tag 'hoje-depois') "abriu o Depois"
  Check ($null -ne (Get-UiNode -Tag 'tarefa:Ligar pro contador')) "a tarefa aparece na hora, com o titulo sem a data"
  $line = Wait-ReceiverLine '*"Ligar pro contador"*'
  Check ($null -ne $line) "a tarefa chegou ao PC ($line)"
  Check ($line -like '*planned_on*' -and $line -like '*planned_time*') "com o dia e a hora entendidos"
  $taskId = if ($line -match ': ([0-9a-f-]{36}) ') { $Matches[1] } else { $null }
  $synced = Wait-UiText 'hoje-sincronia' { param($t) $t -like 'Atualizado*' } 60
  Check ($synced -like 'Atualizado com o PC*') "no celular, a fila esvaziou ('$synced')"

  "== 3. concluir no celular chega ao PC"
  # A de amanha fica em "Depois": mudar para hoje e concluir.
  Check (Set-UiText -Tag 'hoje-campo' -Text 'hoje pagar o boleto') "escreveu outra tarefa, para hoje"
  Check (Invoke-UiTap -Tag 'hoje-adicionar') "Adicionar"
  $boleto = Wait-ReceiverLine '*"Pagar o boleto"*'
  Check ($null -ne $boleto) "a de hoje chegou ao PC"
  Check (Invoke-UiTap -Tag 'concluir:Pagar o boleto') "concluiu no celular"
  $done = Wait-ReceiverLine '*(status)*'
  Check ($null -ne $done) "a conclusao chegou ao PC ($done)"

  "== 4. o lembrete na hora"
  # A hora do proprio emulador (ele costuma estar em UTC, nao no fuso da maquina).
  $emuNow = [datetime]::ParseExact(((Adb shell "date +%H:%M") -join '').Trim(), 'HH:mm', $null)
  $hhmm = $emuNow.AddMinutes(2).ToString('HH:mm')
  Check (Set-UiText -Tag 'hoje-campo' -Text "hoje as $hhmm ligar pro banco") "uma tarefa para as $hhmm"
  Check (Invoke-UiTap -Tag 'hoje-adicionar') "Adicionar"
  $shown = $false
  for ($i = 0; $i -lt 240 -and -not $shown; $i += 5) {
    $notif = (Adb shell dumpsys notification --noredact 2>$null) -join "`n"
    $shown = $notif -match 'Ligar pro banco'
    if (-not $shown) { Start-Sleep -Seconds 5 }
  }
  Check $shown "o lembrete apareceu (as $hhmm, ate 2 min de folga do Android)"
  Check ($notif -match 'Concluir') "com a acao Concluir"
} finally {
  if (-not $receiver.HasExited) { Stop-Process -Id $receiver.Id -Force -Confirm:$false }
  if ($KeepWork) { "pasta do PC de teste mantida: $work" } else { Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue }
}

Stop-IsperEmulatorIfStarted -Keep:$KeepEmulator
Finish-Android -Name 'android-tasks'
