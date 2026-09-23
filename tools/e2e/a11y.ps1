# E2E de acessibilidade (fase 7.5) no app real, via CDP, nas janelas Inicio,
# Biblioteca (reuniao aberta e aba Ditados), Configuracoes e primeira
# configuracao, nos temas escuro e claro:
#  - todo controle visivel tem nome acessivel;
#  - todo texto visivel passa no contraste WCAG AA (4,5:1; 3:1 texto grande);
#  - teclado de verdade (Input.dispatchKeyEvent): a volta de Tab alcanca todo
#    controle, cada foco aparece (:focus-visible com anel) e o foco nao fica
#    preso; as abas da Biblioteca trocam pelas setas; Enter abre a reuniao e
#    renomeia o falante.
# Nao precisa de audio nem GPU. O indicador flutuante fica de fora da volta de
# Tab de proposito: ele nunca rouba o foco (tudo nele tambem esta na bandeja,
# no Inicio e nos atalhos); o nome e o contraste dele entram na conta.
#
#   .\tools\e2e\a11y.ps1 -Exe <caminho do exe>
#
# O roteiro manual com o NVDA esta em docs/TESTES.md.
# ASCII puro de proposito: o PowerShell 5.1 le .ps1 sem BOM como ANSI.
param([string]$Exe)
. "$PSScriptRoot\common.ps1"

$exe = Get-IsperExe $Exe
"a11y: $exe"
$probe = Get-Content -Raw (Join-Path $PSScriptRoot 'a11y-probe.js')
$tabJs = Get-Content -Raw (Join-Path $PSScriptRoot 'a11y-tab.js')
$keys = Join-Path $PSScriptRoot 'cdp-keys.mjs'

Stop-Isper
Check (Start-Isper -Exe $exe) "app abriu com a tela Inicio (CDP)"
Invoke-Isper 'open_library_window' '{ meeting: null }' | Out-Null
Wait-IsperWindow 'library.html' | Out-Null
Invoke-Isper 'open_settings_window' | Out-Null
Wait-IsperWindow 'settings.html' | Out-Null
Invoke-Isper 'open_onboarding_window' | Out-Null
Wait-IsperWindow 'onboarding.html' | Out-Null
Start-Sleep -Seconds 1
$hasMeeting = [bool](EvJson 'library.html' 'JSON.stringify(!!document.querySelector("#list .item"))')
if ($hasMeeting) {
  Ev 'library.html' '(async () => { document.querySelector("#list .item").click(); await new Promise(r => setTimeout(r, 900)); return "ok"; })()' | Out-Null
}

function Test-Page([string]$Target, [string]$Label) {
  $r = EvJson $Target $probe
  if ($null -eq $r) { Check $false "$Label`: verificador rodou"; return }
  $n = @($r.names); $c = @($r.contrast)
  Check ($r.lang -in 'pt-BR', 'en') "$Label`: lang no documento ($($r.lang))"
  Check ($n.Count -eq 0) "$Label`: todo controle com nome acessivel$(if ($n.Count) { ' -> ' + (($n | Select-Object -First 5) -join ' | ') })"
  Check ($c.Count -eq 0) "$Label`: contraste AA em todo texto$(if ($c.Count) { ' -> ' + $c.Count + ': ' + (($c | Select-Object -First 4) -join ' | ') })"
}

# ---- nome e contraste, nos dois temas
$origTheme = (Invoke-Isper 'get_settings' 'undefined' 'settings.html').theme
foreach ($theme in 'dark', 'light') {
  Invoke-Isper 'set_ui_theme' "{ theme: '$theme' }" 'settings.html' | Out-Null
  Start-Sleep -Milliseconds 900
  Test-Page 'home.html' "Inicio ($theme)"
  Test-Page 'library.html' "Biblioteca$(if ($hasMeeting) { ' com reuniao aberta' }) ($theme)"
  Test-Page 'settings.html' "Configuracoes ($theme)"
  Test-Page 'onboarding.html' "Primeira configuracao ($theme)"
}
Invoke-Isper 'set_ui_theme' "{ theme: '$origTheme' }" 'settings.html' | Out-Null
Test-Page 'http://tauri.localhost/' 'Indicador'

# ---- reflow (WCAG 1.4.10): nenhuma janela abre rolagem horizontal no tamanho padrao
foreach ($w in @('home.html', 'library.html', 'settings.html', 'onboarding.html')) {
  $rf = EvJson $w 'JSON.stringify({ sw: document.documentElement.scrollWidth, cw: document.documentElement.clientWidth })'
  Check ($rf.sw -le $rf.cw + 1) "$w sem rolagem horizontal ($($rf.sw) de $($rf.cw) px)"
}

# ---- volta de Tab com teclado de verdade
function Test-Keyboard([string]$Target, [string]$Label, [string]$Media = '') {
  $prep = EvJson $Target $tabJs
  if ($null -eq $prep -or -not $prep.total) { Check $false "$Label`: preparacao da volta de Tab"; return }
  [Environment]::SetEnvironmentVariable('CDP_MEDIA', $(if ($Media) { $Media } else { $null }), 'Process')
  try { & node $keys $Target ($prep.total + 12) 'Tab' | Out-Null }
  finally { [Environment]::SetEnvironmentVariable('CDP_MEDIA', $null, 'Process') }
  $rep = EvJson $Target 'window.__a11yReport()'
  $missed = @($rep.missed); $noRing = @($rep.noRing)
  Check ($missed.Count -eq 0) "$Label`: Tab alcanca os $($rep.total) controles ($($rep.reached))$(if ($missed.Count) { ' -> faltam: ' + (($missed | Select-Object -First 5) -join ' | ') })"
  Check ($noRing.Count -eq 0) "$Label`: todo foco aparece$(if ($noRing.Count) { ' -> sem anel: ' + (($noRing | Select-Object -First 5) -join ' | ') })"
  Check ([bool]$rep.wrapped) "$Label`: o foco da a volta (nada prende o Tab)"
}
Test-Keyboard 'home.html' 'Inicio'
Test-Keyboard 'library.html' 'Biblioteca'
Test-Keyboard 'settings.html' 'Configuracoes'
# "Avancado" (details) abre pelo teclado e o que ele mostra entra na volta.
Ev 'settings.html' 'document.querySelector("details summary").focus(), "ok"' | Out-Null
& node $keys 'settings.html' 1 'Enter' | Out-Null
Start-Sleep -Milliseconds 400
Check ([bool](EvJson 'settings.html' 'JSON.stringify(document.querySelector("details").open)')) "Enter no resumo abre o Avancado"
Test-Keyboard 'settings.html' 'Configuracoes com o Avancado aberto'
Test-Keyboard 'onboarding.html' 'Primeira configuracao'
# Tema de contraste do Windows: o navegador descarta box-shadow; o foco tem de
# continuar visivel (contorno de verdade, base.css).
Test-Keyboard 'home.html' 'Inicio num tema de contraste do Windows' 'forced-colors:active'

# ---- abas da Biblioteca pelas setas (padrao WAI-ARIA)
$tabs = EvJson 'library.html' 'JSON.stringify({ role: document.getElementById("tabs").getAttribute("role"), tabs: [...document.querySelectorAll("#tabs .tab")].map(t => ({ role: t.getAttribute("role"), sel: t.getAttribute("aria-selected"), ti: t.tabIndex })) })'
Check ($tabs.role -eq 'tablist' -and @($tabs.tabs | Where-Object { $_.role -eq 'tab' }).Count -eq 2) "abas com role tablist/tab"
Check (@($tabs.tabs | Where-Object { $_.sel -eq 'true' -and $_.ti -eq 0 }).Count -eq 1 -and @($tabs.tabs | Where-Object { $_.sel -eq 'false' -and $_.ti -eq -1 }).Count -eq 1) "so a aba ativa entra no Tab (foco itinerante)"
Ev 'library.html' 'document.querySelector("#tabs .tab.on").focus(), "ok"' | Out-Null
& node $keys 'library.html' 1 'ArrowRight' | Out-Null
Start-Sleep -Milliseconds 700
$d = EvJson 'library.html' 'JSON.stringify({ sel: (document.querySelector("#tabs .tab[aria-selected=true]") || {}).dataset.tab, focus: document.activeElement && document.activeElement.dataset.tab, dict: document.getElementById("main").classList.contains("dictations") })'
Check ($d.sel -eq 'dictations' -and $d.focus -eq 'dictations' -and $d.dict) "seta para a direita abre a aba Ditados e leva o foco"
Test-Page 'library.html' 'Biblioteca, aba Ditados'
& node $keys 'library.html' 1 'ArrowLeft' | Out-Null
Start-Sleep -Milliseconds 900
$d = EvJson 'library.html' 'JSON.stringify((document.querySelector("#tabs .tab[aria-selected=true]") || {}).dataset.tab)'
Check ($d -eq 'meetings') "seta para a esquerda volta a Reunioes"

# ---- Enter abre a reuniao; Enter no falante abre o campo de renomear; Escape desiste
if ($hasMeeting) {
  Ev 'library.html' '(async () => { const i = document.querySelectorAll("#list .item")[1] || document.querySelector("#list .item"); i.focus(); return "ok"; })()' | Out-Null
  & node $keys 'library.html' 1 'Enter' | Out-Null
  Start-Sleep -Milliseconds 1200
  $o = EvJson 'library.html' 'JSON.stringify({ cur: !!document.querySelector("#list .item[aria-current=true]"), title: (document.querySelector("#detail h2") || {}).textContent || null, sp: document.querySelectorAll("#detail .seg .sp[role=button]").length })'
  Check ($o.cur -and $o.title) "Enter na lista abre a reuniao (marcada com aria-current)"
  if ($o.sp -gt 0) {
    $before = EvJson 'library.html' 'JSON.stringify(document.querySelector("#detail .seg .sp").textContent)'
    Ev 'library.html' 'document.querySelector("#detail .seg .sp").focus(), "ok"' | Out-Null
    & node $keys 'library.html' 1 'Enter' | Out-Null
    Start-Sleep -Milliseconds 400
    $in = EvJson 'library.html' 'JSON.stringify({ input: !!document.querySelector("#detail .seg .sp input"), focus: document.activeElement && document.activeElement.tagName })'
    Check ($in.input -and $in.focus -eq 'INPUT') "Enter no nome do falante abre o campo de renomear, com foco"
    & node $keys 'library.html' 1 'Escape' | Out-Null
    Start-Sleep -Milliseconds 400
    $after = EvJson 'library.html' 'JSON.stringify(document.querySelector("#detail .seg .sp").textContent)'
    Check ($after -eq $before) "Escape desiste sem renomear ('$after')"
  }
}

EvJson 'onboarding.html' '(async () => { await window.__TAURI__.window.getCurrentWindow().close(); return "ok"; })()' | Out-Null
Start-Sleep -Seconds 1
foreach ($w in @('home.html', 'library.html', 'settings.html')) {
  $errs = Get-JsErrors $w
  Check (@($errs).Count -eq 0) "$w sem erros de JS$(Format-JsErrors $errs)"
}

Restart-IsperClean -Exe $exe
Check (-not (Test-Cdp)) "relancado sem porta CDP"
Finish-E2E 'a11y'
