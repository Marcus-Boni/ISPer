# E2E do tema da interface (fase 7.5) no app real, via CDP: claro, escuro e
# "seguir o Windows", aplicados na hora em Inicio, Biblioteca e Configuracoes;
# o indicador flutuante continua escuro. Nao precisa de audio nem GPU.
#
#   .\tools\e2e\theme.ps1 -Exe <caminho do exe> [-Shots <pasta>]
#
# Volta ao tema que estava no fim. Com -Shots, grava um PNG de cada janela em
# cada tema (conferencia visual; nao entra no repositorio).
#
# ASCII puro de proposito: o PowerShell 5.1 le .ps1 sem BOM como ANSI.
param([string]$Exe, [string]$Shots)
. "$PSScriptRoot\common.ps1"

$exe = Get-IsperExe $Exe
"theme: $exe"

Stop-Isper
Check (Start-Isper -Exe $exe) "app abriu com a tela Inicio (CDP)"
Invoke-Isper 'open_library_window' '{ meeting: null }' | Out-Null
Wait-IsperWindow 'library.html' | Out-Null
Invoke-Isper 'open_settings_window' | Out-Null
Wait-IsperWindow 'settings.html' | Out-Null
Start-Sleep -Seconds 2

$windows = @('home.html', 'library.html', 'settings.html')
$probe = 'JSON.stringify({ theme: document.documentElement.dataset.theme, bg: getComputedStyle(document.body).backgroundColor, dark: matchMedia("(prefers-color-scheme: dark)").matches })'
# Fundo do tema: --bg claro (#f6f1ec) e escuro (#161311).
$light = 'rgb(246, 241, 236)'
$dark = 'rgb(22, 19, 17)'

$orig = (Invoke-Isper 'get_settings' 'undefined' 'settings.html').theme
Check ($orig -in 'system', 'light', 'dark') "tema atual lido da config: $orig"
foreach ($w in $windows) {
  $p = EvJson $w $probe
  Check ($null -ne $p -and $p.theme -eq $orig) "$w nasce com data-theme=$orig ($($p.theme))"
}
$sel = EvJson 'settings.html' 'JSON.stringify({ options: Array.from(document.querySelectorAll("#theme option")).map(o => o.value), value: document.getElementById("theme").value })'
Check ($null -ne $sel -and ($sel.options -join ',') -eq 'system,light,dark' -and $sel.value -eq $orig) "seletor de Aparencia com system/light/dark e o valor salvo"

foreach ($t in @('light', 'dark')) {
  $r = Invoke-Isper 'set_ui_theme' "{ theme: '$t' }" 'settings.html'
  Check ($r -eq $t) "set_ui_theme('$t') aceito ($r)"
  Start-Sleep -Milliseconds 600
  $want = if ($t -eq 'light') { $light } else { $dark }
  foreach ($w in $windows) {
    $p = EvJson $w $probe
    Check ($null -ne $p -and $p.theme -eq $t -and $p.bg -eq $want) "$w em '$t' na hora (fundo $($p.bg))"
    if ($Shots) {
      New-Item -ItemType Directory -Force $Shots | Out-Null
      & node (Join-Path $PSScriptRoot 'cdp-shot.mjs') $w (Join-Path $Shots ("$t-" + ($w -replace '\.html$', '') + '.png')) | Out-Null
    }
  }
  $ov = EvJson 'http://tauri.localhost/' 'JSON.stringify({ theme: document.documentElement.dataset.theme || null, bg: getComputedStyle(document.documentElement).getPropertyValue("--bg").trim() })'
  Check ($null -ne $ov -and $ov.bg -eq '#161311') "indicador continua escuro com o tema '$t' (--bg $($ov.bg))"
}

# "Seguir o Windows": o data-theme vira system e o fundo acompanha o prefers-color-scheme.
$r = Invoke-Isper 'set_ui_theme' "{ theme: 'system' }" 'settings.html'
Start-Sleep -Milliseconds 600
$p = EvJson 'settings.html' $probe
$want = if ($p.dark) { $dark } else { $light }
Check ($r -eq 'system' -and $p.theme -eq 'system' -and $p.bg -eq $want) "system segue o Windows (escuro=$($p.dark), fundo $($p.bg))"

# Valor invalido volta ao padrao (normalize).
$r = Invoke-Isper 'set_ui_theme' "{ theme: 'sepia' }" 'settings.html'
Check ($r -eq 'system') "tema invalido vira 'system' ($r)"

# Um tema persistido vale no nascimento da janela (script de inicializacao).
Invoke-Isper 'set_ui_theme' "{ theme: 'light' }" 'settings.html' | Out-Null
# Fecha como o usuario fecha (o X da janela), pela API de janela do Tauri.
EvJson 'settings.html' '(async () => { await window.__TAURI__.window.getCurrentWindow().close(); return "ok"; })()' | Out-Null
Start-Sleep -Seconds 2
Invoke-Isper 'open_settings_window' | Out-Null
Wait-IsperWindow 'settings.html' | Out-Null
$p = EvJson 'settings.html' $probe
Check ($null -ne $p -and $p.theme -eq 'light' -and $p.bg -eq $light) "Configuracoes reabertas ja nascem no tema claro (sem piscar)"

Invoke-Isper 'set_ui_theme' "{ theme: '$orig' }" 'settings.html' | Out-Null
Check (((Invoke-Isper 'get_settings' 'undefined' 'settings.html').theme) -eq $orig) "tema de volta a '$orig'"
foreach ($w in $windows) {
  $errs = Get-JsErrors $w
  Check (@($errs).Count -eq 0) "$w sem erros de JS$(Format-JsErrors $errs)"
}

Restart-IsperClean -Exe $exe
Check (-not (Test-Cdp)) "relancado sem porta CDP"
Finish-E2E 'theme'
