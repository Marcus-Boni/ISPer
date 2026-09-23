<#
.SYNOPSIS
  Gera os manifestos do winget (pacote MarcusBoni.ISPer) para uma versão publicada.

.DESCRIPTION
  O pacote do winget usa o instalador CPU (ISPer_<v>_x64-cpu-setup.exe): funciona em
  qualquer PC x64 e tem 9 MB. O instalador GPU (NVIDIA/CUDA) fica no site — os dois
  gravam o mesmo registro de desinstalação ("ISPer"), e dois pacotes no winget não
  teriam como saber qual está instalado (ver docs/adr/0012).

  O SHA-256 vem do SHA256SUMS.txt da própria release (baixado com o gh), então o
  manifesto só pode ser gerado depois que a versão foi publicada. O resultado segue o
  layout do repositório microsoft/winget-pkgs:

      <Out>/manifests/m/MarcusBoni/ISPer/<versão>/MarcusBoni.ISPer*.yaml

  e é validado com `winget validate` quando o winget existe na máquina.

  Gravado em UTF-8 COM BOM de propósito: o PowerShell 5.1 lê .ps1 sem BOM como ANSI,
  e as descrições em português precisam chegar acentuadas aos manifestos.

.EXAMPLE
  .\scripts\winget-manifests.ps1 -Version 0.19.0
#>
param(
  [Parameter(Mandatory)][string]$Version,
  [string]$Out = (Join-Path $PSScriptRoot '..\packaging\winget'),
  [string]$Sums,
  [string]$ReleaseDate
)
$ErrorActionPreference = 'Stop'

$id = 'MarcusBoni.ISPer'
$repo = 'Marcus-Boni/ISPer'
$tag = "v$Version"
$setup = "ISPer_${Version}_x64-cpu-setup.exe"
# O que fica e o que sai da máquina, no portal (SECURITY.md é sobre vulnerabilidades).
$privacy = 'https://isper.pages.dev/docs/reunioes-e-sistema/privacidade/'
# O schema que o modelo de PR do winget-pkgs pede.
$schema = '1.12.0'

if (-not $Sums) {
  $tmp = Join-Path ([IO.Path]::GetTempPath()) "isper-winget-$Version"
  New-Item -ItemType Directory -Force $tmp | Out-Null
  gh release download $tag --repo $repo --pattern 'SHA256SUMS.txt' --dir $tmp --clobber
  if ($LASTEXITCODE -ne 0) { throw "nao consegui baixar o SHA256SUMS.txt da release $tag" }
  $Sums = Join-Path $tmp 'SHA256SUMS.txt'
}
$line = Get-Content $Sums | Where-Object { $_ -match ('\s' + [regex]::Escape($setup) + '$') } | Select-Object -First 1
if (-not $line) { throw "$setup nao aparece em $Sums" }
$sha = ($line -split '\s+')[0].ToUpper()
if ($sha -notmatch '^[0-9A-F]{64}$') { throw "SHA-256 invalido para ${setup}: $sha" }
if (-not $ReleaseDate) {
  $published = gh release view $tag --repo $repo --json publishedAt -q .publishedAt
  $ReleaseDate = if ($LASTEXITCODE -eq 0 -and $published) { ([datetime]$published).ToString('yyyy-MM-dd') } else { (Get-Date).ToString('yyyy-MM-dd') }
}

$dir = Join-Path $Out "manifests\m\MarcusBoni\ISPer\$Version"
New-Item -ItemType Directory -Force $dir | Out-Null
$utf8 = New-Object System.Text.UTF8Encoding $false
function Write-Manifest([string]$name, [string]$kind, [string]$body) {
  # Os comentários vão em inglês: os manifestos são lidos no microsoft/winget-pkgs.
  $header = "# Created with scripts/winget-manifests.ps1 (https://github.com/$repo)`n# yaml-language-server: `$schema=https://aka.ms/winget-manifest.$kind.$schema.schema.json`n`n"
  [IO.File]::WriteAllText((Join-Path $dir $name), ($header + $body.Trim() + "`n"), $utf8)
}

Write-Manifest "$id.yaml" 'version' @"
PackageIdentifier: $id
PackageVersion: $Version
DefaultLocale: pt-BR
ManifestType: version
ManifestVersion: $schema
"@

Write-Manifest "$id.installer.yaml" 'installer' @"
PackageIdentifier: $id
PackageVersion: $Version
InstallerLocale: pt-BR
Platform:
- Windows.Desktop
# Per-process audio capture (Teams only) needs Windows 10 2004.
MinimumOSVersion: 10.0.19041.0
InstallerType: nullsoft
Scope: user
InstallModes:
- interactive
- silent
- silentWithProgress
UpgradeBehavior: install
ReleaseDate: $ReleaseDate
# What the Tauri NSIS installer writes under HKCU\...\Uninstall\ISPer.
AppsAndFeaturesEntries:
- DisplayName: ISPer
  Publisher: isper
  ProductCode: ISPer
Installers:
- Architecture: x64
  InstallerUrl: https://github.com/$repo/releases/download/$tag/$setup
  InstallerSha256: $sha
ManifestType: installer
ManifestVersion: $schema
"@

$tags = @('ditado', 'transcrição', 'speech-to-text', 'whisper', 'teams', 'reuniões', 'offline', 'privacidade')
$tagsYaml = ($tags | ForEach-Object { "- $_" }) -join "`n"
Write-Manifest "$id.locale.pt-BR.yaml" 'defaultLocale' @"
PackageIdentifier: $id
PackageVersion: $Version
PackageLocale: pt-BR
Publisher: Marcus Boni
PublisherUrl: https://github.com/Marcus-Boni
PublisherSupportUrl: https://github.com/$repo/issues
PrivacyUrl: $privacy
Author: Marcus Boni
PackageName: ISPer
PackageUrl: https://isper.pages.dev
License: MIT
LicenseUrl: https://github.com/$repo/blob/main/LICENSE
ShortDescription: Ditado por voz e transcrição de reuniões do Teams, 100% no seu computador.
Description: |-
  Segure um atalho, fale e o texto aparece colado em qualquer app. Nas reuniões do Microsoft
  Teams, o ISPer grava o seu microfone e o áudio dos participantes, transcreve com o Whisper
  (whisper.cpp) no próprio PC, identifica quem falou e, se você quiser, gera resumo e decisões
  com um provedor de IA escolhido por você — só o texto sai da máquina, o áudio nunca.
  Este pacote é a versão para CPU, que funciona em qualquer PC x64. Com placa de vídeo NVIDIA,
  a versão com CUDA (muito mais rápida) está em https://isper.pages.dev/download/.
Moniker: isper
Tags:
$tagsYaml
ReleaseNotesUrl: https://github.com/$repo/releases/tag/$tag
ManifestType: defaultLocale
ManifestVersion: $schema
"@

$tagsEn = @('dictation', 'transcription', 'speech-to-text', 'whisper', 'teams', 'meeting-notes', 'offline', 'privacy')
$tagsEnYaml = ($tagsEn | ForEach-Object { "- $_" }) -join "`n"
Write-Manifest "$id.locale.en-US.yaml" 'locale' @"
PackageIdentifier: $id
PackageVersion: $Version
PackageLocale: en-US
Publisher: Marcus Boni
PublisherUrl: https://github.com/Marcus-Boni
PublisherSupportUrl: https://github.com/$repo/issues
PrivacyUrl: $privacy
Author: Marcus Boni
PackageName: ISPer
PackageUrl: https://isper.pages.dev
License: MIT
LicenseUrl: https://github.com/$repo/blob/main/LICENSE
ShortDescription: Voice dictation and Microsoft Teams meeting transcription, 100% on your computer.
Description: |-
  Hold a shortcut, speak, and the text is pasted into any app. In Microsoft Teams meetings,
  ISPer records your microphone and the participants' audio, transcribes it with Whisper
  (whisper.cpp) on your own PC, identifies who spoke and, if you want, writes a summary and
  decisions with an AI provider you choose — only text leaves the machine, never audio.
  This package is the CPU build, which works on any x64 PC. With an NVIDIA graphics card,
  the much faster CUDA build is at https://isper.pages.dev/download/.
Tags:
$tagsEnYaml
ReleaseNotesUrl: https://github.com/$repo/releases/tag/$tag
ManifestType: locale
ManifestVersion: $schema
"@

"Manifestos em $dir"
Get-ChildItem $dir | ForEach-Object { "  $($_.Name)" }
if (Get-Command winget -ErrorAction SilentlyContinue) {
  winget validate --manifest $dir
  if ($LASTEXITCODE -ne 0) { throw "winget validate recusou os manifestos ($LASTEXITCODE)" }
} else {
  'winget nao encontrado: validacao pulada.'
}
