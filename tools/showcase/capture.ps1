# Capturas da vitrine (GIF do README e imagem de previa social) no app real,
# via CDP. SO telas sem dado pessoal: a primeira configuracao (sem o passo do
# microfone, que mostraria o nome do dispositivo) e o Copilot preenchido com a
# reuniao FICTICIA de fake-copilot.js. Nunca capture Inicio, Biblioteca ou
# Configuracoes daqui: elas mostram reunioes e pastas de verdade.
#
#   .\tools\showcase\capture.ps1 -Exe <caminho do exe> [-Out <pasta>]
#
# Grava em target\showcase\shots (padrao). Idioma e tema voltam ao que
# estavam no fim. Depois: .\tools\showcase\compose.ps1
# ASCII puro de proposito: o PowerShell 5.1 le .ps1 sem BOM como ANSI.
param([string]$Exe, [string]$Out)
. "$PSScriptRoot\..\e2e\common.ps1"

$exe = Get-IsperExe $Exe
if (-not $Out) { $Out = Join-Path $PSScriptRoot '..\..\target\showcase\shots' }
New-Item -ItemType Directory -Force $Out | Out-Null
$Out = (Resolve-Path $Out).Path
"showcase: $exe -> $Out"

function Shot([string]$target, [string]$name) {
  & node (Join-Path $PSScriptRoot '..\e2e\cdp-shot.mjs') $target (Join-Path $Out $name) | Out-Null
  Check (Test-Path (Join-Path $Out $name)) "captura $name"
}
function CloseWindow([string]$target) {
  EvJson $target '(async () => { await window.__TAURI__.window.getCurrentWindow().close(); return "ok"; })()' | Out-Null
  Start-Sleep -Seconds 1
}

Stop-Isper
Check (Start-Isper -Exe $exe) "app abriu (CDP)"
$s = Invoke-Isper 'get_settings'
$origLang = $s.ui_lang; $origTheme = $s.theme
Invoke-Isper 'set_ui_lang' "{ lang: 'en' }" | Out-Null
Invoke-Isper 'set_ui_theme' "{ theme: 'dark' }" | Out-Null
Start-Sleep -Milliseconds 800

# ---- primeira configuracao: boas-vindas no escuro e no claro, depois o atalho
Invoke-Isper 'open_onboarding_window' | Out-Null
Wait-IsperWindow 'onboarding.html' | Out-Null
Start-Sleep -Seconds 1
Shot 'onboarding.html' '01-welcome-dark.png'
# O claro pelo proprio seletor da pagina, como o usuario faria.
EvJson 'onboarding.html' '(() => { document.querySelector("input[name=theme][value=light]").click(); return "ok"; })()' | Out-Null
Start-Sleep -Milliseconds 900
Shot 'onboarding.html' '02-welcome-light.png'
EvJson 'onboarding.html' '(() => { document.querySelector("input[name=theme][value=dark]").click(); return "ok"; })()' | Out-Null
Start-Sleep -Milliseconds 600
# Tres "Continuar": microfone e modelo passam direto, sem capturar.
EvJson 'onboarding.html' '(async () => { for (let i = 0; i < 3; i++) { document.getElementById("next").click(); await new Promise(r => setTimeout(r, 700)); } return "ok"; })()' | Out-Null
Start-Sleep -Milliseconds 800
$step = EvJson 'onboarding.html' 'JSON.stringify({ keys: document.querySelectorAll(".keys kbd").length })'
Check ($null -ne $step -and $step.keys -ge 2) "passo do atalho na tela ($($step.keys) teclas)"
Shot 'onboarding.html' '03-shortcut.png'
CloseWindow 'onboarding.html'

# ---- Copilot com a reuniao ficticia
Invoke-Isper 'open_copilot_window' | Out-Null
Wait-IsperWindow 'copilot.html' | Out-Null
Start-Sleep -Seconds 1
$fake = Get-Content -Raw -Encoding UTF8 (Join-Path $PSScriptRoot 'fake-copilot.js')
$r = EvJson 'copilot.html' $fake
Start-Sleep -Milliseconds 900
$n = EvJson 'copilot.html' 'JSON.stringify({ cards: document.querySelectorAll(".card-item").length })'
Check ($null -ne $n -and $n.cards -ge 3) "Copilot com a reuniao ficticia ($($n.cards) cartoes)"
Shot 'copilot.html' '04-copilot.png'
CloseWindow 'copilot.html'

Invoke-Isper 'set_ui_lang' "{ lang: '$origLang' }" | Out-Null
Invoke-Isper 'set_ui_theme' "{ theme: '$origTheme' }" | Out-Null
$s = Invoke-Isper 'get_settings'
Check ($s.ui_lang -eq $origLang -and $s.theme -eq $origTheme) "idioma e tema de volta ($origLang / $origTheme)"
Restart-IsperClean -Exe $exe
Finish-E2E 'showcase'
