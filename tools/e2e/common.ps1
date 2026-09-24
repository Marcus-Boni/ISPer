# Utilitários dos testes ponta a ponta (dot-source: . "$PSScriptRoot\common.ps1").
# Sobem o ISPer real com a porta de depuração do WebView2 e conversam com as
# janelas via cdp.mjs. Cada verificação passa por Check; Finish-E2E resume e
# devolve código de saída 1 se algo falhou.

$script:E2ERoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$script:CdpPort = 9223
$script:Checks = 0
$script:Failures = 0

# Os scripts sao feitos de verificacoes (Check) e seguem em frente quando uma
# falha; um erro nao-terminante ou um exe saindo com codigo != 0 (o cdp.mjs
# quando a janela ainda nao existe) NAO pode abortar o roteiro. O GitHub
# Actions roda o pwsh com ErrorActionPreference=Stop: aqui voltamos ao normal.
$ErrorActionPreference = 'Continue'
$PSNativeCommandUseErrorActionPreference = $false

function Get-IsperExe {
  # Exe a testar: o informado, senão o instalado, senão o de desenvolvimento.
  param([string]$Exe)
  if ($Exe) { return (Resolve-Path $Exe).Path }
  $installed = Join-Path $env:LOCALAPPDATA 'Programs\ISPer\isper-app.exe'
  if (Test-Path $installed) { return $installed }
  return Join-Path $script:E2ERoot 'target\release\isper-app.exe'
}

# O ISPer que o usuario usa no dia a dia. Os e2e NUNCA o derrubam: processos
# nao ficam isolados (nem no sandbox do agente), e matar o app no meio de uma
# reuniao perderia a gravacao. So encerram o que eles mesmos abriram - uma
# copia de teste, ou o instalado quando foi o e2e que o abriu, com a porta CDP.
$script:InstalledDir = Join-Path $env:LOCALAPPDATA 'Programs\ISPer'

function Get-AllIsperWebViews {
  # Processos msedgewebview2 do ISPer: os que usam a pasta de dados com.isper.desktop.
  @(Get-CimInstance Win32_Process -Filter "name='msedgewebview2.exe'" -ErrorAction SilentlyContinue |
    Where-Object { $_.CommandLine -like '*com.isper.desktop*' })
}

function Test-OpenedByE2E {
  # O processo isper-app foi aberto por um e2e? Copia fora da pasta instalada:
  # sim (build de desenvolvimento, zip portatil de teste). Na pasta instalada,
  # so se o WebView2 dele tiver a porta de depuracao que o Start-Isper liga.
  # Sem caminho legivel, nao da para saber: fica em paz (o pior caso e um
  # teste falhar, nunca derrubar o app de alguem).
  param([Parameter(Mandatory)]$Process, $WebViews)
  $path = $Process.Path
  if (-not $path) { return $false }
  if (-not $path.StartsWith($script:InstalledDir, [StringComparison]::OrdinalIgnoreCase)) { return $true }
  if ($null -eq $WebViews) { $WebViews = Get-AllIsperWebViews }
  $filhos = @($WebViews | Where-Object { [int]$_.ParentProcessId -eq $Process.Id })
  return [bool](@($filhos | Where-Object { $_.CommandLine -like '*--remote-debugging-port*' }).Count)
}

function Get-E2EIsper {
  # As instancias do ISPer que os e2e podem encerrar.
  $webviews = Get-AllIsperWebViews
  @(Get-Process -Name isper-app -ErrorAction SilentlyContinue | Where-Object { Test-OpenedByE2E -Process $_ -WebViews $webviews })
}

function Get-UserIsper {
  # O ISPer instalado aberto pelo usuario (sem porta CDP): intocavel.
  $webviews = Get-AllIsperWebViews
  @(Get-Process -Name isper-app -ErrorAction SilentlyContinue | Where-Object { -not (Test-OpenedByE2E -Process $_ -WebViews $webviews) })
}

function Get-IsperWebViews {
  # WebView2 que podem ser encerrados: os que NAO descendem do ISPer do usuario.
  $all = Get-AllIsperWebViews
  $user = @(Get-UserIsper | ForEach-Object { $_.Id })
  if ($user.Count -eq 0) { return $all }
  $byPid = @{}
  foreach ($w in $all) { $byPid[[int]$w.ProcessId] = $w }
  @($all | Where-Object {
    $cur = $_
    for ($n = 0; $cur -and $n -lt 8; $n++) {
      if ($user -contains [int]$cur.ParentProcessId) { return $false }
      $cur = $byPid[[int]$cur.ParentProcessId]
    }
    $true
  })
}

function Assert-NoUserIsper {
  # O Windows so deixa uma instancia do ISPer rodar: com a do usuario aberta,
  # a de teste nem sobe. Em vez de derruba-la, para e explica.
  $user = @(Get-UserIsper)
  if ($user.Count -gt 0) {
    throw "O ISPer instalado esta aberto (PID $($user.Id -join ', ')). Feche-o pela bandeja (Sair) antes de rodar os e2e - eles nunca o encerram por conta propria, para nao cortar uma reuniao em andamento."
  }
}

function Stop-Isper {
  # Fecha o app de teste E os processos do WebView2 dele. Sem isso, os
  # msedgewebview2 da instância anterior sobrevivem alguns segundos e a
  # instância nova se acopla a eles — SEM a porta CDP (ECONNREFUSED em 9223, e o
  # teste falha à toa). O ISPer do usuário (instalado, sem CDP) fica de fora.
  Get-E2EIsper | Stop-Process -Force -ErrorAction SilentlyContinue
  for ($i = 0; $i -lt 20; $i++) {
    $alive = @(Get-E2EIsper).Count
    $webviews = @(Get-IsperWebViews)
    if ($alive -eq 0 -and $webviews.Count -eq 0) { break }
    if ($alive -eq 0 -and $i -ge 3) {
      # O app já saiu; o que sobrou do WebView2 não vai fechar sozinho a tempo.
      foreach ($w in $webviews) { Stop-Process -Id $w.ProcessId -Force -ErrorAction SilentlyContinue }
    }
    Start-Sleep -Milliseconds 500
  }
  Start-Sleep -Seconds 1
}

function Test-Cdp {
  try { $null = Invoke-RestMethod "http://127.0.0.1:$script:CdpPort/json" -TimeoutSec 2; return $true } catch { return $false }
}

# Chaves de politica do WebView2 que injetam argumentos no browser de UM exe.
# Usadas so quando ISPER_E2E_CDP_REGISTRY=1 (CI): em processo elevado, como no
# runner do GitHub Actions, o WebView2 ignora a variavel de ambiente
# WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS (e a chave em HKCU); a politica em HKLM
# exige administrador e por isso vale mesmo elevada. Gravamos as duas.
$script:CdpRegistryKeys = @(
  'HKLM:\SOFTWARE\Policies\Microsoft\Edge\WebView2\AdditionalBrowserArguments',
  'HKCU:\Software\Policies\Microsoft\Edge\WebView2\AdditionalBrowserArguments'
)

function Start-Isper {
  # Abre o exe (com a porta CDP, salvo -NoCdp, e variáveis extras) e espera a tela Início.
  # Numa máquina sem config.toml (runner novo do CI) quem abre é a primeira
  # configuração: ela é concluída pelo caminho do usuário (onboarding_finish)
  # e o Início abre em seguida. Com -KeepOnboarding, para nela.
  param([Parameter(Mandatory)][string]$Exe, [hashtable]$Env = @{}, [switch]$NoCdp, [switch]$KeepOnboarding)
  Assert-NoUserIsper
  $exeName = Split-Path $Exe -Leaf
  $viaRegistry = (-not $NoCdp) -and [bool]$env:ISPER_E2E_CDP_REGISTRY
  if (-not $NoCdp) { $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=$script:CdpPort" }
  if ($viaRegistry) {
    foreach ($key in $script:CdpRegistryKeys) {
      try {
        New-Item -Path $key -Force -ErrorAction Stop | Out-Null
        Set-ItemProperty -Path $key -Name $exeName -Value "--remote-debugging-port=$script:CdpPort" -ErrorAction Stop
      } catch { Write-Host "  (aviso) nao consegui gravar $key`: $($_.Exception.Message)" }
    }
  }
  foreach ($k in $Env.Keys) { [Environment]::SetEnvironmentVariable($k, [string]$Env[$k], 'Process') }
  Start-Process $Exe -WorkingDirectory (Split-Path $Exe)
  [Environment]::SetEnvironmentVariable('WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS', $null, 'Process')
  foreach ($k in $Env.Keys) { [Environment]::SetEnvironmentVariable($k, $null, 'Process') }
  if ($NoCdp) { Start-Sleep -Seconds 4; return $true }
  $ok = $false
  $lastError = ''
  $finished = $false
  for ($i = 0; $i -lt 40; $i++) {
    Start-Sleep -Seconds 1
    try {
      $targets = @(Invoke-RestMethod "http://127.0.0.1:$script:CdpPort/json")
      $onb = $targets | Where-Object { $_.url -like '*onboarding.html*' }
      if ($onb -and $KeepOnboarding) {
        Start-Sleep -Seconds 2
        $ok = $true
        break
      }
      if ($onb -and -not $finished) {
        Start-Sleep -Seconds 1
        Invoke-Isper 'onboarding_finish' 'undefined' 'onboarding.html' | Out-Null
        Write-Host "  (primeira configuracao aberta: concluida para seguir ao Inicio)"
        $finished = $true
        continue
      }
      if (-not $KeepOnboarding -and ($targets | Where-Object { $_.url -like '*home.html*' })) {
        Start-Sleep -Seconds 3
        $ok = $true
        break
      }
    } catch { $lastError = $_.Exception.Message }
  }
  # A chave FICA enquanto o app testado vive: cada janela nova cria seu proprio
  # ambiente WebView2 e ele precisa ter os MESMOS argumentos do browser ja
  # aberto — sem a chave, Biblioteca e Configuracoes falhavam com
  # ERROR_INVALID_STATE (0x8007139F). Restart-IsperClean a remove antes do
  # relancamento limpo.
  if ($ok) { return $true }
  # Nao abriu: diz por que, em vez de so "FALHA" (processos, porta, log).
  # Write-Host: saida de diagnostico NAO pode virar valor de retorno da funcao.
  Write-Host "  (diagnostico) ultimo erro ao consultar a porta CDP $script:CdpPort`: $lastError"
  Get-CimInstance Win32_Process -ErrorAction SilentlyContinue |
    Where-Object { $_.Name -in 'isper-app.exe', 'msedgewebview2.exe' } |
    ForEach-Object {
      $cmd = [string]$_.CommandLine
      if ($cmd.Length -gt 220) { $cmd = $cmd.Substring(0, 220) + '...' }
      Write-Host "  (diagnostico) $($_.Name) pid=$($_.ProcessId) ppid=$($_.ParentProcessId) $cmd"
    }
  $listening = @(netstat -ano 2>$null | Select-String ":$script:CdpPort ")
  Write-Host "  (diagnostico) netstat porta $script:CdpPort`: $(if ($listening.Count) { ($listening | ForEach-Object { $_.Line.Trim() }) -join ' | ' } else { 'nada escutando' })"
  $log = Get-TodayLog
  if (Test-Path $log) { Get-Content $log -Tail 25 | ForEach-Object { Write-Host "  (log) $_" } } else { Write-Host "  (diagnostico) sem log em $log" }
  return $false
}

function Ev {
  # Avalia JS numa janela ("home.html", "library.html", "settings.html", "http://tauri.localhost/" = indicador).
  # A expressão vai por variável de ambiente: o PowerShell 5.1 não escapa aspas duplas
  # em argumentos de exe, e querySelector(".x") chegaria ao node sem as aspas.
  param([Parameter(Mandatory)][string]$Target, [Parameter(Mandatory)][string]$Expr)
  [Environment]::SetEnvironmentVariable('CDP_EXPR', $Expr, 'Process')
  try { & node (Join-Path $PSScriptRoot 'cdp.mjs') $Target }
  finally { [Environment]::SetEnvironmentVariable('CDP_EXPR', $null, 'Process') }
}

function EvJson {
  # Como Ev, mas devolve objeto: a página costuma devolver JSON.stringify(...), que o
  # cdp.mjs imprime como string JSON — daí a dupla decodificação.
  param([Parameter(Mandatory)][string]$Target, [Parameter(Mandatory)][string]$Expr)
  $raw = Ev $Target $Expr
  if (-not $raw) { return $null }
  # "alvo nao encontrado", "TIMEOUT" e "EXCEPTION" nao sao JSON: viram $null
  # (a verificacao falha, o roteiro continua) em vez de abortar o script.
  try { return (($raw | ConvertFrom-Json -ErrorAction Stop) | ConvertFrom-Json -ErrorAction Stop) } catch {}
  try { return ($raw | ConvertFrom-Json -ErrorAction Stop) } catch {
    Write-Host "  (diagnostico) resposta nao-JSON de '$Target': $raw"
    return $null
  }
}

function Wait-IsperWindow {
  # Espera a janela (trecho da URL, ex.: 'library.html') aparecer entre os alvos
  # CDP — ate $Seconds. Um sleep fixo nao serve: o runner do CI abre janelas
  # bem mais devagar que a maquina de desenvolvimento.
  param([Parameter(Mandatory)][string]$Target, [int]$Seconds = 20)
  for ($i = 0; $i -lt $Seconds * 2; $i++) {
    try {
      $targets = @(Invoke-RestMethod "http://127.0.0.1:$script:CdpPort/json" -TimeoutSec 2)
      if ($targets | Where-Object { $_.type -eq 'page' -and $_.url -like "*$Target*" }) {
        Start-Sleep -Milliseconds 800   # a pagina ainda esta montando o DOM
        return $true
      }
    } catch {}
    Start-Sleep -Milliseconds 500
  }
  Write-Host "  (diagnostico) a janela '$Target' nao apareceu em $Seconds s"
  return $false
}

function Invoke-Isper {
  # Chama um comando Tauri pela janela informada; erros voltam como objeto { __error }.
  param([Parameter(Mandatory)][string]$Command, [string]$ArgsJson = 'undefined', [string]$Window = 'home.html')
  EvJson $Window "window.__TAURI__.core.invoke('$Command', $ArgsJson).then(v => JSON.stringify(v === undefined ? null : v)).catch(e => JSON.stringify({ __error: String(e) }))"
}

function Get-JsErrors {
  param([string]$Window = 'home.html')
  @(EvJson $Window 'JSON.stringify(window.__isperErrors || [])')
}

function Format-JsErrors {
  # Texto dos erros para a linha FALHA: um teste que esconde a causa nao ajuda ninguem.
  param($Items)
  $list = @($Items | Where-Object { $_ })
  if ($list.Count -eq 0) { return '' }
  return ' -> ' + (($list | ForEach-Object { [string]$_ }) -join ' | ')
}

function Check {
  param([bool]$Ok, [string]$What)
  $script:Checks++
  if ($Ok) { "  OK     $What" } else { $script:Failures++; "  FALHA  $What" }
}

function Get-TodayLog {
  # A partir da 0.12.2 os logs ficam na pasta do identificador do app; a antiga e reserva.
  $name = "isper.log.$(Get-Date -Format yyyy-MM-dd)"
  $new = Join-Path $env:LOCALAPPDATA "com.isper.desktop\logs\$name"
  if (Test-Path $new) { return $new }
  Join-Path $env:LOCALAPPDATA "ISPer\logs\$name"
}

function Remove-CdpRegistry {
  # Apaga a politica de argumentos do WebView2 gravada por Start-Isper (modo CI).
  param([Parameter(Mandatory)][string]$Exe)
  $exeName = Split-Path $Exe -Leaf
  foreach ($key in $script:CdpRegistryKeys) { Remove-ItemProperty -Path $key -Name $exeName -ErrorAction SilentlyContinue }
}

function Restart-IsperClean {
  # Fecha a instância de teste e relança o exe sem porta CDP (estado normal de
  # uso). Uma cópia de desenvolvimento só é relançada no CI (ou com
  # ISPER_E2E_RELAUNCH=1): na máquina de quem usa o ISPer, ela ficaria na
  # bandeja no lugar do app instalado, com outra pasta de dados.
  param([Parameter(Mandatory)][string]$Exe)
  Stop-Isper
  Remove-CdpRegistry -Exe $Exe
  $installed = $Exe.StartsWith($script:InstalledDir, [StringComparison]::OrdinalIgnoreCase)
  if ($installed -or $env:CI -or $env:ISPER_E2E_RELAUNCH) {
    Start-Process $Exe -WorkingDirectory (Split-Path $Exe)
    Start-Sleep -Seconds 3
  } else {
    Write-Host "  (a copia de teste foi fechada; o ISPer instalado pode ser aberto de novo)"
  }
}

function Finish-E2E {
  param([Parameter(Mandatory)][string]$Name)
  ""
  if ($script:Failures -eq 0) {
    "$Name`: $($script:Checks) verificacoes, todas OK"
  } else {
    "$Name`: $($script:Failures) de $($script:Checks) verificacoes FALHARAM"
    exit 1
  }
}
