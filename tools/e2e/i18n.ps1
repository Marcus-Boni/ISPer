# E2E do idioma da interface (fase 7.5) no app real, via CDP: trocar para
# ingles e de volta vale na hora nas janelas abertas (HTML estatico e texto
# montado por script), o indicador acompanha, nenhuma chave falta e a janela
# reaberta ja nasce no idioma salvo. Nao precisa de audio nem GPU.
#
#   .\tools\e2e\i18n.ps1 -Exe <caminho do exe> [-Shots <pasta>]
#
# Volta ao idioma que estava no fim.
# ASCII puro de proposito: o PowerShell 5.1 le .ps1 sem BOM como ANSI.
param([string]$Exe, [string]$Shots)
. "$PSScriptRoot\common.ps1"

$exe = Get-IsperExe $Exe
"i18n: $exe"

Stop-Isper
Check (Start-Isper -Exe $exe) "app abriu com a tela Inicio (CDP)"
Invoke-Isper 'open_settings_window' | Out-Null
Wait-IsperWindow 'settings.html' | Out-Null
Start-Sleep -Seconds 2

$probe = 'JSON.stringify({ lang: document.documentElement.lang, h1: document.querySelector("h1").textContent.trim(), save: document.getElementById("save").textContent.trim(), gpu: document.getElementById("gpuhint").textContent.trim(), diag: (document.querySelector("#diag dt") || {}).textContent || null, ph: document.getElementById("model").getAttribute("placeholder"), missing: (window.__isperErrors || []).filter(e => e.indexOf("i18n:") === 0) })'
$ovProbe = 'JSON.stringify({ lang: document.documentElement.lang, markTitle: document.getElementById("mark").getAttribute("title"), status: document.getElementById("status").textContent.trim() })'

$orig = (Invoke-Isper 'get_settings' 'undefined' 'settings.html').ui_lang
Check ($orig -in 'auto', 'pt-BR', 'en') "idioma atual lido da config: $orig"
$sel = EvJson 'settings.html' 'JSON.stringify(Array.from(document.querySelectorAll("#uilang option")).map(o => o.value))'
Check (($sel -join ',') -eq 'auto,pt-BR,en') "seletor de idioma com auto/pt-BR/en"
$prefs = Invoke-Isper 'ui_prefs' 'undefined' 'settings.html'
Check ($null -ne $prefs -and $prefs.lang -in 'pt-BR', 'en' -and $prefs.strings.'common.undo') "ui_prefs devolve idioma resolvido ($($prefs.lang)) e dicionario"

# ---- ingles na hora
$r = Invoke-Isper 'set_ui_lang' "{ lang: 'en' }" 'settings.html'
Check ($r -eq 'en') "set_ui_lang('en') aceito ($r)"
Start-Sleep -Milliseconds 1200
$p = EvJson 'settings.html' $probe
Check ($p.lang -eq 'en' -and $p.h1 -eq 'Settings' -and $p.save -eq 'Save') "Configuracoes em ingles na hora (h1 '$($p.h1)', botao '$($p.save)')"
Check ($p.gpu -like '*build*' -and $p.gpu -notlike '*prefira*' -and $p.gpu -notlike '*rodam*') "texto montado por script refeito em ingles ('$($p.gpu)')"
Check ($p.diag -eq 'Version') "Diagnostico em ingles (primeira linha '$($p.diag)')"
Check ($p.ph -eq 'e.g. openai/gpt-oss-120b') "placeholder traduzido ('$($p.ph)')"
Check (@($p.missing).Count -eq 0) "nenhuma chave ausente$(Format-JsErrors $p.missing)"
$ov = EvJson 'http://tauri.localhost/' $ovProbe
Check ($ov.lang -eq 'en' -and $ov.markTitle -eq 'mark this moment of the meeting' -and $ov.status -like 'ready*' -and $ov.status -notmatch 'Espa') "indicador em ingles, atalho com 'Space' (status '$($ov.status)')"
if ($Shots) {
  New-Item -ItemType Directory -Force $Shots | Out-Null
  & node (Join-Path $PSScriptRoot 'cdp-shot.mjs') 'settings.html' (Join-Path $Shots 'en-settings.png') | Out-Null
}

# ---- janela reaberta ja nasce em ingles
EvJson 'settings.html' '(async () => { await window.__TAURI__.window.getCurrentWindow().close(); return "ok"; })()' | Out-Null
Start-Sleep -Seconds 2
Invoke-Isper 'open_settings_window' | Out-Null
Wait-IsperWindow 'settings.html' | Out-Null
$p = EvJson 'settings.html' $probe
Check ($p.lang -eq 'en' -and $p.h1 -eq 'Settings') "Configuracoes reabertas ja nascem em ingles"

# ---- portugues de volta
$r = Invoke-Isper 'set_ui_lang' "{ lang: 'pt-BR' }" 'settings.html'
Start-Sleep -Milliseconds 1200
$p = EvJson 'settings.html' $probe
Check ($r -eq 'pt-BR' -and $p.lang -eq 'pt-BR' -and $p.save -eq 'Salvar' -and $p.diag -like 'Vers*') "de volta ao portugues na hora (botao '$($p.save)')"
$ov = EvJson 'http://tauri.localhost/' $ovProbe
Check ($ov.lang -eq 'pt-BR' -and $ov.status -like 'pronto*') "indicador de volta ao portugues ('$($ov.status)')"

# ---- valor invalido e restauracao
$r = Invoke-Isper 'set_ui_lang' "{ lang: 'klingon' }" 'settings.html'
Check ($r -eq 'auto') "idioma invalido vira 'auto' ($r)"
Invoke-Isper 'set_ui_lang' "{ lang: '$orig' }" 'settings.html' | Out-Null
Check (((Invoke-Isper 'get_settings' 'undefined' 'settings.html').ui_lang) -eq $orig) "idioma de volta a '$orig'"
foreach ($w in @('home.html', 'settings.html')) {
  $errs = Get-JsErrors $w
  Check (@($errs).Count -eq 0) "$w sem erros de JS$(Format-JsErrors $errs)"
}

Restart-IsperClean -Exe $exe
Check (-not (Test-Cdp)) "relancado sem porta CDP"
Finish-E2E 'i18n'
