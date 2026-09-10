<#
.SYNOPSIS
  Gera o instalador assinado do ISPer e, com -Publish, publica a release no GitHub.

.DESCRIPTION
  1. Confere que Cargo.toml e tauri.conf.json têm a mesma versão e que as DLLs
     do CUDA estão em apps/isper-app/src-tauri/resources/cuda.
  2. Para o ISPer (as DLLs ficam travadas enquanto ele roda).
  3. `tauri build --bundles nsis` com a chave privada de assinatura das
     atualizações (padrão: %USERPROFILE%\.tauri\isper.key — NUNCA no repositório).
     Sai ISPer_<v>_x64-setup.exe + ISPer_<v>_x64-setup.exe.sig.
  4. Monta o latest.json que o app instalado consulta
     (https://github.com/Marcus-Boni/ISPer/releases/latest/download/latest.json).
  5. Com -Publish: `gh release create v<v>` subindo instalador, .sig e latest.json.
     Sem -Publish só gera os arquivos e mostra onde ficaram.

.EXAMPLE
  .\scripts\release.ps1
  .\scripts\release.ps1 -Publish -Notes "Legendas ao vivo, momentos marcados e atualização automática."
#>
param(
  [switch]$Publish,
  [string]$Notes = "",
  [string]$KeyPath = "$env:USERPROFILE\.tauri\isper.key"
)
$ErrorActionPreference = 'Stop'

$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$app = Join-Path $root 'apps\isper-app'
$tauriDir = Join-Path $app 'src-tauri'

# --- 1) versões e pré-requisitos
$conf = Get-Content (Join-Path $tauriDir 'tauri.conf.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$version = $conf.version
$cargoVer = (Select-String -Path (Join-Path $tauriDir 'Cargo.toml') -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
if ($cargoVer -ne $version) { throw "versao divergente: Cargo.toml=$cargoVer, tauri.conf.json=$version" }
if (-not (Test-Path $KeyPath)) {
  throw "chave de assinatura nao encontrada em $KeyPath. Gere uma vez com: npx @tauri-apps/cli@^2 signer generate -w $KeyPath (e cole a .pub em tauri.conf.json > plugins.updater.pubkey)"
}
foreach ($dll in 'cudart64_13.dll', 'cublas64_13.dll', 'cublasLt64_13.dll') {
  $p = Join-Path $tauriDir "resources\cuda\$dll"
  if (-not (Test-Path $p)) { throw "DLL do CUDA ausente: $p (copie do toolkit; ver README)" }
}
$tag = "v$version"
if ($Publish) {
  gh auth status *> $null
  if ($LASTEXITCODE -ne 0) { throw "gh nao esta autenticado (gh auth login)" }
  $exists = gh release view $tag 2>$null
  if ($LASTEXITCODE -eq 0) { throw "a release $tag ja existe no GitHub" }
}

# --- 2) parar o app
"ISPer $version - parando o app (as DLLs do CUDA ficam travadas enquanto ele roda)"
Stop-Process -Name isper-app -Force -ErrorAction SilentlyContinue
Start-Sleep -Seconds 1

# --- 3) build do instalador com assinatura das atualizacoes
$env:TAURI_SIGNING_PRIVATE_KEY = (Get-Content $KeyPath -Raw).Trim()
# `--ci`: sem TAURI_SIGNING_PRIVATE_KEY_PASSWORD, o CLI assume senha vazia em vez de
# perguntar no terminal (no Windows nao existe variavel de ambiente vazia). Chave com
# senha? Defina TAURI_SIGNING_PRIVATE_KEY_PASSWORD antes de rodar o script.
Push-Location $app
try {
  npx --yes @tauri-apps/cli@^2 build --bundles nsis --ci
  if ($LASTEXITCODE -ne 0) { throw "tauri build falhou (codigo $LASTEXITCODE)" }
}
finally {
  Pop-Location
  $env:TAURI_SIGNING_PRIVATE_KEY = $null
}

# --- 4) latest.json
$bundle = Join-Path $root 'target\release\bundle\nsis'
$setup = Get-Item (Join-Path $bundle "ISPer_${version}_x64-setup.exe")
$sig = Get-Item (Join-Path $bundle "ISPer_${version}_x64-setup.exe.sig")
$notes = if ($Notes) { $Notes } else { "ISPer $version" }
$latest = [ordered]@{
  version   = $version
  notes     = $notes
  pub_date  = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')
  platforms = [ordered]@{
    'windows-x86_64' = [ordered]@{
      signature = (Get-Content $sig.FullName -Raw).Trim()
      url       = "https://github.com/Marcus-Boni/ISPer/releases/download/$tag/$($setup.Name)"
    }
  }
}
$latestPath = Join-Path $bundle 'latest.json'
[System.IO.File]::WriteAllText($latestPath, ($latest | ConvertTo-Json -Depth 5), (New-Object System.Text.UTF8Encoding $false))

""
"Artefatos gerados:"
"  $($setup.FullName)  ($([math]::Round($setup.Length / 1MB)) MB)"
"  $($sig.FullName)"
"  $latestPath"

# --- 5) publicar
if ($Publish) {
  gh release create $tag $setup.FullName $sig.FullName $latestPath --title "ISPer $version" --notes $notes
  if ($LASTEXITCODE -ne 0) { throw "gh release create falhou" }
  "Release $tag publicada. Os ISPers instalados passam a ver a versao nova na proxima checagem."
} else {
  ""
  "Nada foi publicado. Para publicar: .\scripts\release.ps1 -Publish -Notes '...'"
}
