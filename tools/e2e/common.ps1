# Utilitários dos testes ponta a ponta (dot-source: . "$PSScriptRoot\common.ps1").
# Sobem o ISPer real com a porta de depuração do WebView2 e conversam com as
# janelas via cdp.mjs. Cada verificação passa por Check; Finish-E2E resume e
# devolve código de saída 1 se algo falhou.

$script:E2ERoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$script:CdpPort = 9223
$script:Checks = 0
$script:Failures = 0

function Get-IsperExe {
  # Exe a testar: o informado, senão o instalado, senão o de desenvolvimento.
  param([string]$Exe)
  if ($Exe) { return (Resolve-Path $Exe).Path }
  $installed = Join-Path $env:LOCALAPPDATA 'Programs\ISPer\isper-app.exe'
  if (Test-Path $installed) { return $installed }
  return Join-Path $script:E2ERoot 'target\release\isper-app.exe'
}

function Stop-Isper {
  Stop-Process -Name isper-app -Force -ErrorAction SilentlyContinue
  Start-Sleep -Seconds 1
}

function Test-Cdp {
  try { $null = Invoke-RestMethod "http://127.0.0.1:$script:CdpPort/json" -TimeoutSec 2; return $true } catch { return $false }
}

function Start-Isper {
  # Abre o exe (com a porta CDP, salvo -NoCdp, e variáveis extras) e espera a tela Início.
  param([Parameter(Mandatory)][string]$Exe, [hashtable]$Env = @{}, [switch]$NoCdp)
  if (-not $NoCdp) { $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=$script:CdpPort" }
  foreach ($k in $Env.Keys) { [Environment]::SetEnvironmentVariable($k, [string]$Env[$k], 'Process') }
  Start-Process $Exe -WorkingDirectory (Split-Path $Exe)
  [Environment]::SetEnvironmentVariable('WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS', $null, 'Process')
  foreach ($k in $Env.Keys) { [Environment]::SetEnvironmentVariable($k, $null, 'Process') }
  if ($NoCdp) { Start-Sleep -Seconds 4; return $true }
  for ($i = 0; $i -lt 40; $i++) {
    Start-Sleep -Seconds 1
    try {
      if ((Invoke-RestMethod "http://127.0.0.1:$script:CdpPort/json") | Where-Object { $_.url -like '*home.html*' }) {
        Start-Sleep -Seconds 3
        return $true
      }
    } catch {}
  }
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
  try { return (($raw | ConvertFrom-Json) | ConvertFrom-Json) } catch { return ($raw | ConvertFrom-Json) }
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

function Check {
  param([bool]$Ok, [string]$What)
  $script:Checks++
  if ($Ok) { "  OK     $What" } else { $script:Failures++; "  FALHA  $What" }
}

function Get-TodayLog {
  Join-Path $env:LOCALAPPDATA "ISPer\logs\isper.log.$(Get-Date -Format yyyy-MM-dd)"
}

function Restart-IsperClean {
  # Relança o exe sem porta CDP (estado normal de uso).
  param([Parameter(Mandatory)][string]$Exe)
  Stop-Isper
  Start-Process $Exe -WorkingDirectory (Split-Path $Exe)
  Start-Sleep -Seconds 3
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
