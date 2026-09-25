# E2E da sincronia com o celular (fase 9.3) no app real, via CDP, com o
# isper-cli fazendo o papel do celular:
#   1. ligar "Receber as gravacoes do celular": o servidor sobe na porta;
#   2. parear: o QR sai, o "celular" usa o codigo, o PC pergunta "Permitir?"
#      e o teste permite; o aparelho aparece na lista;
#   3. a gravacao, com dois momentos, vira reuniao pelo mesmo passe final,
#      com a origem "<aparelho> . <arquivo>" e os momentos, e a ata volta
#      para o "celular"; mandar de novo nao reenvia nem cria outra reuniao;
#   4. um segundo aparelho que o PC recusa nao entra;
#   5. esquecido no PC, o aparelho nao manda mais nada.
# Precisa de um modelo Whisper instalado e do isper-cli compilado
# (cargo build --release -p isper-cli).
#
#   .\tools\e2e\sync.ps1 -Exe <caminho do exe>
#
# Tudo pelo loopback (127.0.0.1): nenhum aparelho da rede entra no teste. Na
# primeira vez, o Windows pode perguntar se o exe testado pode usar a rede;
# o teste passa mesmo recusando.
# ASCII puro de proposito: o PowerShell 5.1 le .ps1 sem BOM como ANSI.
param([string]$Exe)
. "$PSScriptRoot\common.ps1"
$exe = Get-IsperExe $Exe
"sync: $exe"
$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$cli = Join-Path $root 'target\release\isper-cli.exe'
if (-not (Test-Path $cli)) { throw "isper-cli nao encontrado: $cli (cargo build --release -p isper-cli)" }
$work = Join-Path ([IO.Path]::GetTempPath()) ('isper-e2e-sync-' + [guid]::NewGuid().ToString('N').Substring(0, 8))
New-Item -ItemType Directory -Force $work | Out-Null
$audio = Join-Path $work '20260925-101500.opus'
Copy-Item (Join-Path $root 'fixtures\formatos\fala-2s.opus') $audio
$created = @()

function Get-Phone { Invoke-Isper 'phone_sync_status' }

function Wait-Phone([scriptblock]$Cond, [int]$Secs = 20) {
  $s = $null
  for ($i = 0; $i -lt $Secs * 2; $i++) {
    $s = Get-Phone
    if ($s -and (& $Cond $s)) { return $s }
    Start-Sleep -Milliseconds 500
  }
  return $s
}

# O codigo traz os enderecos da rede; o teste fala pelo loopback.
function Get-LocalCode([string]$Code, [int]$Port) {
  return ($Code -replace '&a=[^&]*', "&a=127.0.0.1:$Port")
}

# Um "celular": o isper-cli enviar em segundo plano (ele espera o "Permitir?").
function Start-Phone([string]$State, [string]$Name, [string]$Code, [string[]]$Extra = @()) {
  $log = Join-Path $work ("{0}-{1}.log" -f ($Name -replace '\W', ''), [guid]::NewGuid().ToString('N').Substring(0, 6))
  $cliArgs = @('enviar', $audio, '--estado', $State, '--nome', $Name, '--esperar', '240') + $Extra
  if ($Code) { $cliArgs += @('--codigo', $Code) }
  # Start-Process no PowerShell 5.1 junta os argumentos sem aspas.
  $line = ($cliArgs | ForEach-Object { '"' + ($_ -replace '"', '\"') + '"' }) -join ' '
  $p = Start-Process -FilePath $cli -ArgumentList $line -RedirectStandardOutput $log -RedirectStandardError "$log.err" -PassThru -WindowStyle Hidden
  # Sem pegar o Handle agora, o PowerShell perde o ExitCode quando o processo termina.
  $null = $p.Handle
  return @{ Process = $p; Log = $log }
}

function Read-PhoneLog($Phone) {
  $out = ''
  foreach ($f in @($Phone.Log, "$($Phone.Log).err")) {
    if (Test-Path $f) { $out += (Get-Content -Raw -Encoding UTF8 $f) }
  }
  return $out
}

Stop-Isper
Check (Start-Isper -Exe $exe) "app abriu (CDP) no perfil de teste"
$st0 = Invoke-Isper 'home_status'
Check ($st0.engine.kind -eq 'ready') "motor pronto ($($st0.engine.kind))"
if ($st0.engine.kind -ne 'ready') { "sem motor carregado: abortando"; Finish-E2E 'sync' }
$before = @(Invoke-Isper 'list_meetings' '{ query: null }').Count

# ---- 1. ligar
$s = Get-Phone
Check (-not $s.enabled -and -not $s.running) "a sincronia vem desligada"
Invoke-Isper 'phone_sync_set_enabled' '{ enabled: true }' | Out-Null
$s = Wait-Phone { param($x) $x.running }
Check ($s.running -and $s.port -gt 0) "ligada: o PC ouve na porta $($s.port)"
$port = [int]$s.port

# ---- 2. parear (e o PC pergunta)
Invoke-Isper 'phone_sync_start_pairing' | Out-Null
$s = Wait-Phone { param($x) $null -ne $x.pairing }
Check ($s.pairing.code.StartsWith('isper://parear?v=1&pc=') -and $s.pairing.svg -match '<svg' -and $s.pairing.remaining_secs -gt 60) "QR de pareamento (vale $($s.pairing.remaining_secs) s)"
$phoneA = Join-Path $work 'celular-a'
$phone = Start-Phone $phoneA 'Celular de teste' (Get-LocalCode $s.pairing.code $port) @('--momentos', '0.5,1.5')
$s = Wait-Phone { param($x) $null -ne $x.pending } 40
Check ($s.pending.name -eq 'Celular de teste') "o PC pergunta: '$($s.pending.name)' quer parear"
Invoke-Isper 'phone_sync_answer' '{ allow: true }' | Out-Null

# ---- 3. a gravacao vira reuniao e a ata volta
$done = $phone.Process.WaitForExit(300000)
$log = Read-PhoneLog $phone
Check ($done -and $phone.Process.ExitCode -eq 0) "o celular pareou, enviou e recebeu a ata (saida $($phone.Process.ExitCode))$(if ($phone.Process.ExitCode) { ': ' + $log })"
$ata = [IO.Path]::ChangeExtension($audio, 'ata.md')
Check ((Test-Path $ata) -and ((Get-Content -Raw -Encoding UTF8 $ata) -match 'Transcrito 100% localmente')) "a ata do PC chegou ao celular"
$s = Get-Phone
Check (@($s.devices).Count -eq 1 -and $s.devices[0].name -eq 'Celular de teste') "o aparelho aparece na lista do PC"
$list = @(Invoke-Isper 'list_meetings' '{ query: null }')
Check ($list.Count -eq $before + 1) "a gravacao virou uma reuniao na Biblioteca"
$m = $list | Sort-Object id -Descending | Select-Object -First 1
if ($m) {
  $created += $m.id
  $det = Invoke-Isper 'get_meeting' "{ id: $($m.id) }"
  Check ($det.meeting.source_name -like 'Celular de teste*20260925-101500.opus') "a origem mostra o aparelho e o arquivo ($($det.meeting.source_name))"
  Check (@($det.moments).Count -eq 2) "os momentos marcados no celular entraram na reuniao ($(@($det.moments) -join ', '))"
}
$recebidos = Get-E2EDocsPath 'Do celular'
Check (@(Get-ChildItem -Recurse -Filter '20260925-101500.opus' $recebidos -ErrorAction SilentlyContinue).Count -eq 1) "o audio ficou em Documentos\ISPer\Do celular"

$again = Start-Phone $phoneA 'Celular de teste' ''
$again.Process.WaitForExit(90000) | Out-Null
$againLog = Read-PhoneLog $again
Check ($again.Process.ExitCode -eq 0 -and $againLog -notmatch 'enviando') "mandar de novo nao reenvia o audio"
Check (@(Invoke-Isper 'list_meetings' '{ query: null }').Count -eq $before + 1) "nem cria outra reuniao"

# ---- 4. o PC recusa um segundo aparelho
Invoke-Isper 'phone_sync_start_pairing' | Out-Null
$s = Wait-Phone { param($x) $null -ne $x.pairing }
$other = Start-Phone (Join-Path $work 'celular-b') 'Outro celular' (Get-LocalCode $s.pairing.code $port)
$s = Wait-Phone { param($x) $null -ne $x.pending } 40
Check ($s.pending.name -eq 'Outro celular') "o segundo aparelho tambem passa pelo 'Permitir?'"
Invoke-Isper 'phone_sync_answer' '{ allow: false }' | Out-Null
$other.Process.WaitForExit(90000) | Out-Null
Check ($other.Process.ExitCode -ne 0 -and (Read-PhoneLog $other) -match 'recus') "o PC recusou o segundo aparelho"
Check (@((Get-Phone).devices).Count -eq 1) "so o primeiro ficou pareado"

# ---- 5. esquecido no PC, o aparelho nao manda mais
$dev = @((Get-Phone).devices)[0]
Invoke-Isper 'phone_sync_forget' "{ id: '$($dev.id)' }" | Out-Null
Check (@((Get-Phone).devices).Count -eq 0) "aparelho esquecido no PC"
$forgot = Start-Phone $phoneA 'Celular de teste' ''
$forgot.Process.WaitForExit(90000) | Out-Null
Check ($forgot.Process.ExitCode -ne 0 -and (Read-PhoneLog $forgot) -match 'pareado com o PC') "o celular esquecido nao manda mais nada"

# ---- 6. a tela
Invoke-Isper 'open_settings_window' | Out-Null
Wait-IsperWindow 'settings.html' | Out-Null
Start-Sleep -Seconds 2
$ui = EvJson 'settings.html' 'JSON.stringify({ on: document.getElementById("phonesync").checked, state: document.getElementById("phonestate").textContent, devices: document.querySelectorAll("#phonedevices .dev").length, pair: !document.getElementById("phonepair").disabled })'
Check ($ui.on -and $ui.pair -and $ui.state -match "$port") "Configuracoes: ligada, com a porta e o botao Parear ($($ui.state))"
$errs = Get-JsErrors 'settings.html'
Check (@($errs).Count -eq 0) "settings.html sem erros de JS$(Format-JsErrors $errs)"

# ---- limpeza
Invoke-Isper 'phone_sync_set_enabled' '{ enabled: false }' | Out-Null
$s = Wait-Phone { param($x) -not $x.running }
Check (-not $s.running) "desligada de novo"
$undoMs = 0
foreach ($id in $created) {
  $sched = Invoke-Isper 'delete_meeting' "{ id: $id }"
  if ($sched.undo_ms -gt $undoMs) { $undoMs = [int]$sched.undo_ms }
}
if ($undoMs -gt 0) { Start-Sleep -Milliseconds ($undoMs + 1500) }
Check (@(Invoke-Isper 'list_meetings' '{ query: null }').Count -eq $before) "reunioes de teste excluidas da Biblioteca"
Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
Restart-IsperClean -Exe $exe
Check (-not (Test-Cdp)) "relancado sem porta CDP"
Finish-E2E 'sync'
