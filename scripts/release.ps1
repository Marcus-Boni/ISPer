<#
.SYNOPSIS
  Gera os instaladores assinados do ISPer (GPU/CUDA e CPU) e, com -Publish, publica a release.

.DESCRIPTION
  Pré-checagens (a release só sai consistente):
    - versão lida do Cargo.toml do app (fonte única — o tauri.conf.json não a repete); CHANGELOG.md com a seção "## [versão]"
      (que vira as notas da release, salvo -Notes); árvore do git limpa (salvo -AllowDirty);
      com -Publish: gh autenticado, tag inexistente e CI verde no commit atual (salvo -SkipCiCheck).
  Recursos que vão dentro do instalador:
    - DLLs do CUDA em apps/isper-app/src-tauri/resources/cuda (só a variante GPU; copie do toolkit);
    - DLLs do sherpa-onnx (identificação de falantes), copiadas de target\release;
    - runtime do Visual C++ (msvcp140/vcruntime140), copiado do VS Build Tools.
  Build (salvo -SkipBuild): para o ISPer e roda `tauri build` por variante, assinando as
  atualizações com a chave privada (padrão %USERPROFILE%\.tauri\isper.key — NUNCA no repositório):
    - GPU: --config tauri.gpu.conf.json  → dist\v<v>\ISPer_<v>_x64-setup.exe (+ .sig, latest.json)
    - CPU: --config tauri.cpu.conf.json --no-default-features, em target-cpu\
           → dist\v<v>\ISPer_<v>_x64-cpu-setup.exe (+ .sig, latest-cpu.json)
  Publicação (-Publish): `gh release create v<v>` com todos os arquivos de dist\v<v>. A tag criada
  dispara o workflow release.yml, que valida tag × manifests × CHANGELOG e roda os testes.

.EXAMPLE
  .\scripts\release.ps1                       # gera as duas variantes em dist\v<versão>\
  .\scripts\release.ps1 -Variant gpu          # só a GPU
  .\scripts\release.ps1 -Publish              # gera e publica (notas = seção do CHANGELOG)
  .\scripts\release.ps1 -Publish -SkipBuild   # publica o que já está em dist\v<versão>\
#>
param(
  [switch]$Publish,
  [switch]$SkipBuild,
  [ValidateSet('both', 'gpu', 'cpu')][string]$Variant = 'both',
  [string]$Notes = "",
  [switch]$AllowDirty,
  [switch]$SkipCiCheck,
  [string]$KeyPath = "$env:USERPROFILE\.tauri\isper.key"
)
$ErrorActionPreference = 'Stop'

$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$app = Join-Path $root 'apps\isper-app'
$tauriDir = Join-Path $app 'src-tauri'

# Comandos nativos (gh, git, cargo) escrevem no stderr; no PowerShell 5.1 com
# ErrorActionPreference=Stop isso vira erro terminante. Roda-os com 'Continue'.
function Invoke-Native([scriptblock]$Block) {
  $prev = $ErrorActionPreference; $ErrorActionPreference = 'Continue'
  try { & $Block } finally { $ErrorActionPreference = $prev }
}

# --- 1) versão, CHANGELOG, chave, git
# Fonte unica da versao: o Cargo.toml do app (o Tauri le de la; o tauri.conf.json nao a repete).
$version = (Select-String -Path (Join-Path $tauriDir 'Cargo.toml') -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
$conf = Get-Content (Join-Path $tauriDir 'tauri.conf.json') -Raw -Encoding UTF8 | ConvertFrom-Json
if ($null -ne $conf.version) { throw "tauri.conf.json nao deve declarar 'version' - a fonte unica e o Cargo.toml ($version)" }
$tag = "v$version"

$changelog = Get-Content (Join-Path $root 'CHANGELOG.md') -Raw -Encoding UTF8
$section = [regex]::Match($changelog, "(?ms)^## \[" + [regex]::Escape($version) + "\][^\n]*\n(.*?)(?=^## \[|\z)")
if (-not $section.Success) { throw "CHANGELOG.md nao tem a secao '## [$version]' - escreva as mudancas antes de lancar" }
$notes = if ($Notes) { $Notes } else { $section.Groups[1].Value.Trim() }
if (-not $notes) { throw "a secao [$version] do CHANGELOG.md esta vazia" }

if (-not (Test-Path $KeyPath)) {
  throw "chave de assinatura nao encontrada em $KeyPath. Gere uma vez com: npx @tauri-apps/cli@^2 signer generate -w $KeyPath (e cole a .pub em tauri.conf.json > plugins.updater.pubkey)"
}

$dirty = Invoke-Native { git -C $root status --porcelain 2>$null }
if ($dirty -and -not $AllowDirty) { throw "arvore do git com mudancas nao commitadas - commite (ou use -AllowDirty):`n$($dirty -join "`n")" }
$sha = (Invoke-Native { git -C $root rev-parse HEAD 2>$null }).Trim()

if ($Publish) {
  $login = Invoke-Native { gh api user --jq .login 2>$null }
  if (-not $login) { throw "gh nao esta autenticado (gh auth login)" }
  $existing = Invoke-Native { gh release list --limit 100 --json tagName --jq '.[].tagName' 2>$null }
  if ($existing -contains $tag) { throw "a release $tag ja existe no GitHub" }
  if (-not $SkipCiCheck) {
    $run = Invoke-Native { gh run list --commit $sha --workflow CI --limit 1 --json status,conclusion,url 2>$null } | ConvertFrom-Json
    if (-not $run -or $run[0].status -ne 'completed' -or $run[0].conclusion -ne 'success') {
      $state = if ($run) { "$($run[0].status)/$($run[0].conclusion) $($run[0].url)" } else { 'nenhum run encontrado (o commit foi enviado?)' }
      throw "o CI do commit $($sha.Substring(0,7)) nao esta verde: $state. Espere/corrija, ou use -SkipCiCheck."
    }
  }
  "Publicando como $login (commit $($sha.Substring(0,7)), CI verde)"
}

# --- 2) recursos empacotados
$wantGpu = $Variant -in 'both', 'gpu'
$wantCpu = $Variant -in 'both', 'cpu'
if ($wantGpu) {
  foreach ($dll in 'cudart64_13.dll', 'cublas64_13.dll', 'cublasLt64_13.dll') {
    $p = Join-Path $tauriDir "resources\cuda\$dll"
    if (-not (Test-Path $p)) { throw "DLL do CUDA ausente: $p (copie de <CUDA>\bin\x64; ver README)" }
  }
}
# sherpa-onnx: o exe importa a sherpa-onnx-c-api.dll, que puxa onnxruntime e cargs.
# O build script do sherpa-rs as deixa em target\release; sem elas o app instalado nem abre.
$sherpaDir = Join-Path $tauriDir 'resources\sherpa'
New-Item -ItemType Directory -Force $sherpaDir | Out-Null
foreach ($dll in 'sherpa-onnx-c-api.dll', 'sherpa-onnx-cxx-api.dll', 'onnxruntime.dll', 'onnxruntime_providers_shared.dll', 'cargs.dll') {
  $src = Join-Path $root "target\release\$dll"
  if (-not (Test-Path $src)) { throw "DLL do sherpa-onnx ausente: $src (rode 'cargo build --release -p isper-app' antes)" }
  Copy-Item $src (Join-Path $sherpaDir $dll) -Force
}
# Runtime do Visual C++ ao lado do exe (deploy "app-local", suportado pela Microsoft):
# o exe e a onnxruntime.dll importam msvcp140/vcruntime140, que uma maquina limpa nao tem.
$vcDir = Join-Path $tauriDir 'resources\vcredist'
New-Item -ItemType Directory -Force $vcDir | Out-Null
$crt = @()
foreach ($vsRoot in @("${env:ProgramFiles(x86)}\Microsoft Visual Studio", "$env:ProgramFiles\Microsoft Visual Studio")) {
  if (Test-Path $vsRoot) { $crt += Resolve-Path "$vsRoot\*\*\VC\Redist\MSVC\*\x64\Microsoft.VC143.CRT" -ErrorAction SilentlyContinue }
}
$crt = $crt | Sort-Object { $_.Path } -Descending | Select-Object -First 1
if (-not $crt) { throw "pasta Microsoft.VC143.CRT do Visual Studio nao encontrada (VC\Redist\MSVC\<versao>\x64) - instale o componente 'MSVC v143 - C++ Redistributable' do Build Tools" }
foreach ($dll in 'msvcp140.dll', 'vcruntime140.dll', 'vcruntime140_1.dll') {
  Copy-Item (Join-Path $crt.Path $dll) (Join-Path $vcDir $dll) -Force
}
"runtime do Visual C++ copiado de $($crt.Path)"

# --- 3) builds
$dist = Join-Path $root "dist\$tag"
New-Item -ItemType Directory -Force $dist | Out-Null
$gpuSetup = Join-Path $dist "ISPer_${version}_x64-setup.exe"
$cpuSetup = Join-Path $dist "ISPer_${version}_x64-cpu-setup.exe"

function Build-Variant([string]$name, [string]$configFile, [string]$targetDir, [string[]]$cargoArgs, [string]$dest) {
  "== build $name (target: $targetDir)"
  $env:TAURI_SIGNING_PRIVATE_KEY = (Get-Content $KeyPath -Raw).Trim()
  $env:CARGO_TARGET_DIR = $targetDir
  Push-Location $app
  try {
    # `--ci`: sem TAURI_SIGNING_PRIVATE_KEY_PASSWORD o CLI assume senha vazia em vez de
    # perguntar (no Windows nao existe variavel de ambiente vazia). Chave com senha?
    # Defina TAURI_SIGNING_PRIVATE_KEY_PASSWORD antes de rodar.
    # (nao chamar de $args: dentro de um scriptblock, $args e a variavel automatica dele, vazia)
    # --config e resolvido a partir do diretorio atual (apps\isper-app): caminho absoluto.
    $npxArgs = @('--yes', '@tauri-apps/cli@^2', 'build', '--bundles', 'nsis', '--ci', '--config', (Join-Path $tauriDir $configFile))
    if ($cargoArgs) { $npxArgs += '--'; $npxArgs += $cargoArgs }
    Invoke-Native { npx @npxArgs }
    if ($LASTEXITCODE -ne 0) { throw "tauri build ($name) falhou (codigo $LASTEXITCODE)" }
  }
  finally {
    Pop-Location
    $env:TAURI_SIGNING_PRIVATE_KEY = $null
    $env:CARGO_TARGET_DIR = $null
  }
  $built = Join-Path $targetDir "release\bundle\nsis\ISPer_${version}_x64-setup.exe"
  if (-not (Test-Path $built) -or -not (Test-Path "$built.sig")) { throw "artefatos do build $name nao encontrados em $built" }
  Copy-Item $built $dest -Force
  Copy-Item "$built.sig" "$dest.sig" -Force
}

if (-not $SkipBuild) {
  "ISPer $version - parando o app (o bundler reescreve o exe e as DLLs ficam travadas)"
  Stop-Process -Name isper-app -Force -ErrorAction SilentlyContinue
  Start-Sleep -Seconds 1
  if ($wantGpu) { Build-Variant 'GPU (CUDA)' 'tauri.gpu.conf.json' (Join-Path $root 'target') @() $gpuSetup }
  if ($wantCpu) { Build-Variant 'CPU' 'tauri.cpu.conf.json' (Join-Path $root 'target-cpu') @('--no-default-features') $cpuSetup }
} else {
  "ISPer $version - reaproveitando os artefatos de $dist (-SkipBuild)"
}

# --- 4) manifests do atualizador
function Write-Latest([string]$setup, [string]$name, [string]$path) {
  $latest = [ordered]@{
    version   = $version
    notes     = $notes
    pub_date  = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')
    platforms = [ordered]@{
      'windows-x86_64' = [ordered]@{
        signature = (Get-Content "$setup.sig" -Raw).Trim()
        url       = "https://github.com/Marcus-Boni/ISPer/releases/download/$tag/$name"
      }
    }
  }
  [System.IO.File]::WriteAllText($path, ($latest | ConvertTo-Json -Depth 5), (New-Object System.Text.UTF8Encoding $false))
}
$files = @()
if ($wantGpu) {
  if (-not (Test-Path $gpuSetup)) { throw "instalador GPU ausente: $gpuSetup" }
  Write-Latest $gpuSetup (Split-Path $gpuSetup -Leaf) (Join-Path $dist 'latest.json')
  $files += $gpuSetup, "$gpuSetup.sig", (Join-Path $dist 'latest.json')
}
if ($wantCpu) {
  if (-not (Test-Path $cpuSetup)) { throw "instalador CPU ausente: $cpuSetup" }
  Write-Latest $cpuSetup (Split-Path $cpuSetup -Leaf) (Join-Path $dist 'latest-cpu.json')
  $files += $cpuSetup, "$cpuSetup.sig", (Join-Path $dist 'latest-cpu.json')
}
$notesFile = Join-Path $dist 'release-notes.md'
[System.IO.File]::WriteAllText($notesFile, $notes, (New-Object System.Text.UTF8Encoding $false))

""
"Artefatos em ${dist}:"
foreach ($f in $files) { $fi = Get-Item $f; "  {0,-38} {1,8:N1} MB" -f $fi.Name, ($fi.Length / 1MB) }

# --- 5) publicar
if ($Publish) {
  # Notas por arquivo: o PowerShell 5.1 nao escapa aspas embutidas ao chamar um exe.
  Invoke-Native { gh release create $tag @files --title "ISPer $version" --notes-file $notesFile }
  if ($LASTEXITCODE -ne 0) { throw "gh release create falhou (codigo $LASTEXITCODE)" }
  "Release $tag publicada. Os ISPers instalados veem a versao nova na proxima checagem; o workflow release.yml valida a tag."
} else {
  ""
  "Nada foi publicado. Para publicar: .\scripts\release.ps1 -Publish (ou -Publish -SkipBuild para reaproveitar dist\$tag)"
}
