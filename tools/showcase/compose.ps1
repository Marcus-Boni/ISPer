# Monta a vitrine a partir das capturas do capture.ps1: a imagem de previa
# social (1280x640) e o GIF de demonstracao do README, com as fontes do app.
# Renderiza frame.html e social.html no Edge headless e junta os quadros com
# o ffmpeg (transicao em fade, paleta por GIF).
#
#   .\tools\showcase\compose.ps1 [-Shots <pasta>] [-Out <pasta>] [-Ffmpeg <exe>]
#
# Grava docs\media\isper-demo.gif e docs\media\social-preview.png (padrao).
# A previa social sobe a mao: GitHub -> Settings -> Social preview.
# ASCII puro de proposito: o PowerShell 5.1 le .ps1 sem BOM como ANSI.
param([string]$Shots, [string]$Out, [string]$Ffmpeg, [int]$Width = 960)
$ErrorActionPreference = 'Stop'

$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
if (-not $Shots) { $Shots = Join-Path $root 'target\showcase\shots' }
if (-not $Out) { $Out = Join-Path $root 'docs\media' }
$work = Join-Path $root 'target\showcase\frames'
New-Item -ItemType Directory -Force $Out, $work | Out-Null

$edge = @(
  "${env:ProgramFiles(x86)}\Microsoft\Edge\Application\msedge.exe",
  "$env:ProgramFiles\Microsoft\Edge\Application\msedge.exe"
) | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $edge) { throw 'Edge nao encontrado' }
if (-not $Ffmpeg) {
  $cmd = Get-Command ffmpeg -ErrorAction SilentlyContinue
  if ($cmd) { $Ffmpeg = $cmd.Source }
  else {
    $Ffmpeg = Get-ChildItem "$env:LOCALAPPDATA\Microsoft\WinGet\Packages\Gyan.FFmpeg*" -Recurse -Filter ffmpeg.exe -ErrorAction SilentlyContinue |
      Select-Object -First 1 -ExpandProperty FullName
  }
}
if (-not $Ffmpeg) { throw 'ffmpeg nao encontrado (winget install Gyan.FFmpeg)' }

function FileUrl([string]$path) { ([uri](Resolve-Path $path).Path).AbsoluteUri }
function Render([string]$page, [hashtable]$query, [int]$w, [int]$h, [string]$png) {
  $qs = ($query.GetEnumerator() | ForEach-Object { $_.Key + '=' + [uri]::EscapeDataString($_.Value) }) -join '&'
  $url = (FileUrl (Join-Path $PSScriptRoot $page)) + '?' + $qs
  $profileDir = Join-Path $work 'edge-profile'
  $edgeArgs = @('--headless=new', '--disable-gpu', '--hide-scrollbars', '--allow-file-access-from-files',
    '--force-device-scale-factor=1', '--virtual-time-budget=4000', "--user-data-dir=$profileDir",
    "--window-size=$w,$h", "--screenshot=$png", $url)
  Start-Process -FilePath $edge -ArgumentList $edgeArgs -Wait -NoNewWindow -RedirectStandardError (Join-Path $work 'edge.err') | Out-Null
  if (-not (Test-Path $png)) { throw "o Edge nao gerou $png" }
  "  $([IO.Path]::GetFileName($png))"
}

# ---- previa social
$social = Join-Path $Out 'social-preview.png'
Remove-Item $social -ErrorAction SilentlyContinue
Render 'social.html' @{ shot = (FileUrl (Join-Path $Shots '04-copilot.png')) } 1280 640 $social

# ---- quadros do GIF: captura + legenda
$frames = @(
  @{ shot = '01-welcome-dark.png'; step = '1/4'; cap = 'Ready in four short steps' },
  @{ shot = '02-welcome-light.png'; step = '2/4'; cap = 'Light, dark or follow Windows' },
  @{ shot = '03-shortcut.png'; step = '3/4'; cap = 'Hold, speak, release, in any app' },
  @{ shot = '04-copilot.png'; step = '4/4'; cap = 'Optional meeting Copilot: decisions and actions, live' }
)
$pngs = @()
for ($i = 0; $i -lt $frames.Count; $i++) {
  $f = $frames[$i]
  $png = Join-Path $work ('frame-{0}.png' -f $i)
  Remove-Item $png -ErrorAction SilentlyContinue
  Render 'frame.html' @{ shot = (FileUrl (Join-Path $Shots $f.shot)); step = $f.step; cap = $f.cap } 1080 800 $png
  $pngs += $png
}

# ---- GIF: cada quadro parado $hold s, fade de $fade s entre eles
$hold = 2.8; $fade = 0.4; $fps = 10
$inputs = @()
foreach ($p in $pngs) { $inputs += @('-loop', '1', '-framerate', "$fps", '-t', "$($hold + $fade)", '-i', $p) }
$chain = ''; $prev = '0'; $offset = 0.0
for ($i = 1; $i -lt $pngs.Count; $i++) {
  $offset += $hold
  $label = if ($i -eq $pngs.Count - 1) { 'v' } else { "x$i" }
  $chain += "[$prev][$i]xfade=transition=fade:duration=$($fade):offset=$($offset.ToString([Globalization.CultureInfo]::InvariantCulture))[$label];"
  $prev = $label
}
$filter = $chain + "[v]fps=$fps,scale=$($Width):-1:flags=lanczos,split[a][b];[a]palettegen=max_colors=192:stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=4:diff_mode=rectangle"
$gif = Join-Path $Out 'isper-demo.gif'
& $Ffmpeg -y -loglevel error @inputs -filter_complex $filter -loop 0 $gif
if ($LASTEXITCODE -ne 0) { throw "ffmpeg falhou ($LASTEXITCODE)" }
'{0}  {1:N0} KB' -f $gif, ((Get-Item $gif).Length / 1KB)
'{0}  {1:N0} KB' -f $social, ((Get-Item $social).Length / 1KB)
