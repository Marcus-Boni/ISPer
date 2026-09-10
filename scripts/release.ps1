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
  -SkipBuild reaproveita os artefatos já gerados (ex.: o build passou e só a
  publicação falhou).

.EXAMPLE
  .\scripts\release.ps1
  .\scripts\release.ps1 -Publish -Notes "Legendas ao vivo, momentos marcados e atualização automática."
  .\scripts\release.ps1 -Publish -SkipBuild
#>
param(
  [switch]$Publish,
  [switch]$SkipBuild,
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
# DLLs de runtime do sherpa-onnx (identificacao de falantes): o exe importa a
# sherpa-onnx-c-api.dll, que puxa onnxruntime e cargs. O build script do sherpa-rs
# as deixa em target\release; sem elas no instalador o app instalado nem abre.
$sherpaDlls = 'sherpa-onnx-c-api.dll', 'sherpa-onnx-cxx-api.dll', 'onnxruntime.dll', 'onnxruntime_providers_shared.dll', 'cargs.dll'
$sherpaDir = Join-Path $tauriDir 'resources\sherpa'
New-Item -ItemType Directory -Force $sherpaDir | Out-Null
foreach ($dll in $sherpaDlls) {
  $src = Join-Path $root "target\release\$dll"
  if (-not (Test-Path $src)) { throw "DLL do sherpa-onnx ausente: $src (rode um cargo build --release do app antes)" }
  Copy-Item $src (Join-Path $sherpaDir $dll) -Force
}
$tag = "v$version"
if ($Publish) {
  # No PowerShell 5.1 com ErrorActionPreference=Stop, qualquer linha que um exe
  # escreva no stderr vira erro terminante - por isso os comandos do gh rodam
  # com 'Continue' e sao julgados pelo que devolvem, nao pelo stderr.
  $prev = $ErrorActionPreference; $ErrorActionPreference = 'Continue'
  $login = gh api user --jq .login 2>$null
  $existing = gh release list --limit 100 --json tagName --jq '.[].tagName' 2>$null
  $ErrorActionPreference = $prev
  if (-not $login) { throw "gh nao esta autenticado (gh auth login)" }
  if ($existing -contains $tag) { throw "a release $tag ja existe no GitHub" }
  "Publicando como $login"
}

if ($SkipBuild) {
  "ISPer $version - reaproveitando os artefatos ja gerados (-SkipBuild)"
} else {
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
  # As notas vao por arquivo: o PowerShell 5.1 nao escapa aspas embutidas ao montar a
  # linha de comando de um exe, e aspas dentro das notas viravam argumentos soltos.
  $notesFile = Join-Path $bundle 'release-notes.md'
  [System.IO.File]::WriteAllText($notesFile, $notes, (New-Object System.Text.UTF8Encoding $false))
  $prev = $ErrorActionPreference; $ErrorActionPreference = 'Continue'
  gh release create $tag $setup.FullName $sig.FullName $latestPath --title "ISPer $version" --notes-file $notesFile
  $code = $LASTEXITCODE
  $ErrorActionPreference = $prev
  if ($code -ne 0) { throw "gh release create falhou (codigo $code)" }
  "Release $tag publicada. Os ISPers instalados passam a ver a versao nova na proxima checagem."
} else {
  ""
  "Nada foi publicado. Para publicar: .\scripts\release.ps1 -Publish -Notes '...'"
}
