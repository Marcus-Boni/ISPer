<#
.SYNOPSIS
  Atualizador ponta a ponta sem publicar nada: release falsa assinada, servida em localhost.
.DESCRIPTION
  1. Gera um build de TESTE do app (TAURI_CONFIG libera http para o endpoint local).
  2. Cria uma "release" 99.0.0 falsa: exe de 3 MB aleatório assinado com a SUA chave
     (%USERPROFILE%\.tauri\isper.key) e um latest.json apontando para localhost.
  3. Serve a pasta com `python -m http.server` e abre o app com ISPER_UPDATE_ENDPOINT
     (endpoint local) e ISPER_UPDATE_DRY_RUN (baixa e verifica, mas não instala).
  4. Confere: checagem encontra a 99.0.0, banner no Início, download com assinatura
     verificada, download adulterado recusado, recusa durante reunião.
  5. Refaz o build normal (salvo -SkipRebuild). Leva ~6 min (dois builds do app).
.EXAMPLE
  .\tools\e2e\updater-local.ps1
#>
param([string]$KeyPath = "$env:USERPROFILE\.tauri\isper.key", [int]$Port = 8765, [switch]$SkipRebuild)
. "$PSScriptRoot\common.ps1"
$root = $script:E2ERoot
if (-not (Test-Path $KeyPath)) { throw "chave de assinatura nao encontrada em $KeyPath" }
$exe = Join-Path $root 'target\release\isper-app.exe'

"1) build de teste (aceita http no endpoint de atualizacao)"
Stop-Isper
$env:TAURI_CONFIG = '{"plugins":{"updater":{"dangerousInsecureTransportProtocol":true}}}'
try {
  Push-Location $root
  cargo build --release -p isper-app
  if ($LASTEXITCODE -ne 0) { throw "build de teste falhou" }
} finally { Pop-Location; [Environment]::SetEnvironmentVariable('TAURI_CONFIG', $null, 'Process') }

"2) release falsa 99.0.0 assinada com a chave real"
$fake = Join-Path $env:TEMP 'isper-fake-release'
New-Item -ItemType Directory -Force $fake | Out-Null
$fakeExe = Join-Path $fake 'ISPer_99.0.0_x64-setup.exe'
$bytes = New-Object byte[] (3 * 1024 * 1024)
(New-Object System.Random).NextBytes($bytes)
[System.IO.File]::WriteAllBytes($fakeExe, $bytes)
npx --yes '@tauri-apps/cli@^2' signer sign -f $KeyPath -p "" $fakeExe | Out-Null
if (-not (Test-Path "$fakeExe.sig")) { throw "assinatura da release falsa nao gerada" }
$latest = [ordered]@{
  version   = '99.0.0'
  notes     = "Versao de teste do atualizador (servidor local).`n- item um`n- item dois"
  pub_date  = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')
  platforms = [ordered]@{ 'windows-x86_64' = [ordered]@{ url = "http://127.0.0.1:$Port/ISPer_99.0.0_x64-setup.exe"; signature = (Get-Content "$fakeExe.sig" -Raw).Trim() } }
}
[System.IO.File]::WriteAllText((Join-Path $fake 'latest.json'), ($latest | ConvertTo-Json -Depth 5), (New-Object System.Text.UTF8Encoding $false))

"3) servidor local em 127.0.0.1:$Port e app de teste"
$server = Start-Process python -ArgumentList "-m http.server $Port --bind 127.0.0.1" -WorkingDirectory $fake -PassThru -WindowStyle Hidden
try {
  Start-Sleep -Seconds 2
  Check ((Invoke-RestMethod "http://127.0.0.1:$Port/latest.json").version -eq '99.0.0') "servidor local respondendo"
  Check (Start-Isper -Exe $exe -Env @{ ISPER_UPDATE_ENDPOINT = "http://127.0.0.1:$Port/latest.json"; ISPER_UPDATE_DRY_RUN = '1' }) "app de teste abriu"

  "4) fluxo"
  $u = Invoke-Isper 'check_update'
  Check ($u.version -eq '99.0.0' -and $u.notes) "check_update encontrou a 99.0.0 (atual $($u.current))"
  Start-Sleep -Seconds 1
  $b = EvJson 'home.html' 'JSON.stringify({ hidden: document.getElementById("update").hidden, ver: document.getElementById("upd-ver").textContent })'
  Check ((-not $b.hidden) -and $b.ver -eq '99.0.0') "banner de versao nova no Inicio"
  $r = Invoke-Isper 'install_update'
  Check (("$r" -match 'dry-run') -and ("$r" -match 'assinatura verificada')) "download + assinatura verificada (dry run): $r"

  [System.IO.File]::WriteAllBytes($fakeExe, ($bytes + [byte]0))
  $r2 = Invoke-Isper 'install_update'
  Check ("$($r2.__error)" -match 'assinatura') "download adulterado recusado: $($r2.__error)"
  [System.IO.File]::WriteAllBytes($fakeExe, $bytes)

  Invoke-Isper 'toggle_meeting_cmd' | Out-Null
  Start-Sleep -Seconds 2
  $r3 = Invoke-Isper 'install_update'
  Check ("$($r3.__error)" -match 'reuni') "recusa durante reuniao: $($r3.__error)"
  Invoke-Isper 'toggle_meeting_cmd' | Out-Null
  Start-Sleep -Seconds 8
  Check ((Get-JsErrors 'home.html').Count -eq 0) "sem erros de JS no Inicio"
}
finally {
  Stop-Isper
  Stop-Process -Id $server.Id -Force -ErrorAction SilentlyContinue
}

if (-not $SkipRebuild) {
  "5) refazendo o build normal (sem TAURI_CONFIG)"
  Push-Location $root
  try { cargo build --release -p isper-app; if ($LASTEXITCODE -ne 0) { throw "rebuild falhou" } } finally { Pop-Location }
} else {
  "AVISO: target\release\isper-app.exe e um build de TESTE (aceita http); rode cargo build --release -p isper-app antes de distribuir."
}
Finish-E2E 'updater-local'
