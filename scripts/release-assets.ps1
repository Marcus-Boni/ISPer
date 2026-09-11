<#
.SYNOPSIS
  Fecha os artefatos de uma release do ISPer a partir dos instaladores ja gerados.

.DESCRIPTION
  Recebe a pasta com ISPer_<v>_x64-setup.exe (GPU) e/ou ISPer_<v>_x64-cpu-setup.exe (CPU) e
  produz, ao lado deles:
    - <instalador>.sig            assinatura minisign do atualizador (chave privada do mantenedor)
    - latest.json / latest-cpu.json  manifests que o app consulta para se atualizar
    - ISPer_<v>_sbom.cdx.json     SBOM CycloneDX do app (cargo cyclonedx), salvo -SkipSbom
    - SHA256SUMS.txt              somas SHA-256 de todos os arquivos publicados
    - release-notes.md            a secao ## [<v>] do CHANGELOG.md (notas da release)
  E a MESMA logica para o fluxo local (scripts/release.ps1) e para o job "publish" do
  release.yml no GitHub Actions: o que muda e so de onde vem a chave (arquivo em
  %USERPROFILE%\.tauri ou a variavel TAURI_SIGNING_PRIVATE_KEY, no CI).

  A assinatura e feita AQUI, por ultimo, e nao no `tauri build`: quando a SignPath assinar o
  instalador (Authenticode), os bytes mudam e uma assinatura minisign feita antes deixaria de
  valer - o atualizador recusaria o pacote.

.EXAMPLE
  .\scripts\release-assets.ps1 -Dist .\dist\v0.14.0
  .\scripts\release-assets.ps1 -Dist .\dist\v0.14.0 -Sbom .\ISPer_0.14.0_sbom.cdx.json
#>
param(
  [Parameter(Mandatory)][string]$Dist,
  [string]$Version,
  [string]$KeyPath = "$env:USERPROFILE\.tauri\isper.key",
  [string]$Sbom,
  [switch]$SkipSbom
)
$ErrorActionPreference = 'Stop'

$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$tauriDir = Join-Path $root 'apps\isper-app\src-tauri'
$Dist = (Resolve-Path $Dist).Path

# Comandos nativos (npx, cargo) escrevem no stderr; com ErrorActionPreference=Stop isso vira
# erro terminante no PowerShell 5.1. Roda-os com 'Continue' e julga pelo codigo de saida.
function Invoke-Native([scriptblock]$Block) {
  $prev = $ErrorActionPreference; $ErrorActionPreference = 'Continue'
  try { & $Block } finally { $ErrorActionPreference = $prev }
}

# --- versao e notas
if (-not $Version) {
  $Version = (Select-String -Path (Join-Path $tauriDir 'Cargo.toml') -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
}
$changelog = Get-Content (Join-Path $root 'CHANGELOG.md') -Raw -Encoding UTF8
$section = [regex]::Match($changelog, "(?ms)^## \[" + [regex]::Escape($Version) + "\][^\n]*\n(.*?)(?=^## \[|\z)")
if (-not $section.Success) { throw "CHANGELOG.md nao tem a secao '## [$Version]'" }
$notes = $section.Groups[1].Value.Trim()
if (-not $notes) { throw "a secao [$Version] do CHANGELOG.md esta vazia" }
$utf8 = New-Object System.Text.UTF8Encoding $false
[System.IO.File]::WriteAllText((Join-Path $Dist 'release-notes.md'), $notes, $utf8)

# --- instaladores presentes
$gpuSetup = Join-Path $Dist "ISPer_${Version}_x64-setup.exe"
$cpuSetup = Join-Path $Dist "ISPer_${Version}_x64-cpu-setup.exe"
$variants = @()
if (Test-Path $gpuSetup) { $variants += @{ setup = $gpuSetup; manifest = 'latest.json' } }
if (Test-Path $cpuSetup) { $variants += @{ setup = $cpuSetup; manifest = 'latest-cpu.json' } }
if ($variants.Count -eq 0) { throw "nenhum instalador ISPer_${Version}_x64-setup.exe ou ISPer_${Version}_x64-cpu-setup.exe em $Dist" }

# --- assinatura minisign (atualizador)
$keyArgs = @()
if ($env:TAURI_SIGNING_PRIVATE_KEY) {
  $keyArgs = @() # a CLI le a variavel de ambiente (equivale a -k), sem expor a chave na linha de comando
} elseif (Test-Path $KeyPath) {
  $keyArgs = @('-f', $KeyPath)
} else {
  throw "chave de assinatura ausente: defina TAURI_SIGNING_PRIVATE_KEY ou passe -KeyPath (padrao $KeyPath)"
}
# `signer sign` PROMPTA pela senha quando ela nao vem por -p, e a chave do ISPer nao tem senha:
# e preciso passar -p "" (string vazia). O PowerShell 5.1 DESCARTA argumentos vazios ao chamar um
# exe (o -p engoliria o caminho do arquivo), entao a chamada vai pelo cmd.exe, que os preserva;
# o stdin e fechado para a CLI nunca ficar presa num prompt. Com a chave na variavel
# TAURI_SIGNING_PRIVATE_KEY (CI), a CLI a le sozinha e $keyArgs fica vazio.
function Sign-File([string]$file, [string[]]$keyArgs) {
  $quotedKey = ($keyArgs | ForEach-Object { if ($_ -match '\s') { '"' + $_ + '"' } else { $_ } }) -join ' '
  $psi = New-Object System.Diagnostics.ProcessStartInfo
  $psi.FileName = 'cmd.exe'
  # ^^ porque ^ e o caractere de escape do cmd; a CLI recebe @tauri-apps/cli@^2.
  $psi.Arguments = "/d /c npx --yes @tauri-apps/cli@^^2 signer sign $quotedKey -p `"`" `"$file`""
  $psi.WorkingDirectory = Join-Path $root 'apps\isper-app'
  $psi.UseShellExecute = $false
  $psi.RedirectStandardInput = $true
  $psi.RedirectStandardOutput = $true
  $psi.RedirectStandardError = $true
  $p = [System.Diagnostics.Process]::Start($psi)
  $p.StandardInput.Close()
  $errTask = $p.StandardError.ReadToEndAsync()
  $out = $p.StandardOutput.ReadToEnd()
  $p.WaitForExit()
  $out += $errTask.Result
  if ($p.ExitCode -ne 0 -or -not (Test-Path "$file.sig")) {
    throw "assinatura minisign falhou para $(Split-Path $file -Leaf) (codigo $($p.ExitCode)):`n$out"
  }
}
if ($env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD) { throw "chave com senha nao e suportada por este script (a do ISPer nao tem senha)" }
foreach ($v in $variants) { Sign-File $v.setup $keyArgs }

# --- manifests do atualizador
$tag = "v$Version"
$pubDate = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')
foreach ($v in $variants) {
  $name = Split-Path $v.setup -Leaf
  $latest = [ordered]@{
    version   = $Version
    notes     = $notes
    pub_date  = $pubDate
    platforms = [ordered]@{
      'windows-x86_64' = [ordered]@{
        signature = (Get-Content "$($v.setup).sig" -Raw).Trim()
        url       = "https://github.com/Marcus-Boni/ISPer/releases/download/$tag/$name"
      }
    }
  }
  [System.IO.File]::WriteAllText((Join-Path $Dist $v.manifest), ($latest | ConvertTo-Json -Depth 5), $utf8)
}

# --- SBOM (CycloneDX) do app: uma lista de tudo que entra no binario, para quem audita
$sbomName = "ISPer_${Version}_sbom.cdx.json"
if ($Sbom) {
  Copy-Item $Sbom (Join-Path $Dist $sbomName) -Force
} elseif (-not $SkipSbom) {
  Invoke-Native { cargo cyclonedx --version 2>$null | Out-Null }
  if ($LASTEXITCODE -ne 0) { throw "cargo-cyclonedx nao instalado: cargo install cargo-cyclonedx --locked (ou use -SkipSbom)" }
  Push-Location $root
  try {
    # O cargo-cyclonedx escreve um SBOM por crate do workspace, cada um na sua pasta; o do app
    # (que ja inclui core, llm, models e diarize como dependencias) e o que vai para a release.
    $base = "ISPer_${Version}_sbom.cdx"
    Invoke-Native { cargo cyclonedx --format json --spec-version 1.5 --target x86_64-pc-windows-msvc --manifest-path (Join-Path $tauriDir 'Cargo.toml') --override-filename $base }
    if ($LASTEXITCODE -ne 0) { throw "cargo cyclonedx falhou (codigo $LASTEXITCODE)" }
    $appSbom = Join-Path $tauriDir "$base.json"
    if (-not (Test-Path $appSbom)) { throw "SBOM do app nao encontrado em $appSbom" }
    Move-Item $appSbom (Join-Path $Dist $sbomName) -Force
    Get-ChildItem (Join-Path $root 'crates') -Recurse -Filter "$base.json" | Remove-Item -Force
  } finally { Pop-Location }
}

# --- somas SHA-256 (formato do sha256sum: "<hash>  <nome>")
$sumsPath = Join-Path $Dist 'SHA256SUMS.txt'
$lines = Get-ChildItem $Dist -File |
  Where-Object { $_.Name -notin 'SHA256SUMS.txt', 'release-notes.md' } |
  Sort-Object Name |
  ForEach-Object { "{0}  {1}" -f (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower(), $_.Name }
[System.IO.File]::WriteAllText($sumsPath, (($lines -join "`n") + "`n"), $utf8)

""
"Artefatos em ${Dist}:"
foreach ($f in Get-ChildItem $Dist -File | Sort-Object Name) { "  {0,-40} {1,10:N1} KB" -f $f.Name, ($f.Length / 1KB) }
