# E2E da primeira configuracao (fase 7.5) no app real, via CDP: com
# onboarding_done = false no config.toml o ISPer abre nela (e nao no Inicio);
# os cinco passos montam (microfone com medidor, modelos, atalho, IA, resumo),
# o idioma troca na hora, Enter avanca, "Abrir o ISPer" fecha, grava
# onboarding_done = true e abre o Inicio; Configuracoes -> Sistema a reabre e
# o X da janela tambem conta como concluida. Nao precisa de GPU; sem
# microfone (runner do CI), o passo do microfone mostra o aviso em vez do nivel.
#
#   .\tools\e2e\onboarding.ps1 -Exe <caminho do exe> [-Shots <pasta>]
#
# ASCII puro de proposito: o PowerShell 5.1 le .ps1 sem BOM como ANSI.
param([string]$Exe, [string]$Shots)
. "$PSScriptRoot\common.ps1"

$exe = Get-IsperExe $Exe
"onboarding: $exe"

$cfgPath = Join-Path $env:APPDATA 'ISPer\config.toml'
# CONFIG_VERSION de apps/isper-app/src-tauri/src/config.rs.
$ConfigVersion = 2
$utf8 = New-Object System.Text.UTF8Encoding $false
function Set-OnboardingPending {
  # Simula a primeira execucao sem apagar nada: so o campo muda. Leitura e
  # escrita em UTF-8 explicito (o Get-Content do 5.1 leria como ANSI e
  # estragaria acentos de nomes de microfone e termos do dicionario).
  if (-not (Test-Path $cfgPath)) { return }
  $lines = @([IO.File]::ReadAllLines($cfgPath, $utf8) | Where-Object { $_ -notmatch '^\s*(onboarding_done|config_version)\s*=' })
  # Campos soltos no topo do TOML (antes de qualquer [tabela]). A versao vai
  # junto: num arquivo da versao 1 a migracao 1 -> 2 marcaria a configuracao
  # como feita (quem ja usava o ISPer nao a ve) e o teste nao a veria abrir.
  $lines = @('onboarding_done = false', "config_version = $ConfigVersion") + $lines
  [IO.File]::WriteAllLines($cfgPath, [string[]]$lines, $utf8)
}
function Get-OnboardingDone {
  if (-not (Test-Path $cfgPath)) { return $null }
  return [bool](@([IO.File]::ReadAllLines($cfgPath, $utf8)) -match '^\s*onboarding_done\s*=\s*true')
}

Stop-Isper
Set-OnboardingPending
Check (Start-Isper -Exe $exe -KeepOnboarding) "primeira execucao abre a primeira configuracao (CDP)"
$targets = @(Invoke-RestMethod "http://127.0.0.1:$script:CdpPort/json" -TimeoutSec 3)
Check (-not ($targets | Where-Object { $_.url -like '*home.html*' })) "o Inicio nao abre junto"

$probe = 'JSON.stringify({ lang: document.documentElement.lang, step: (document.querySelector(".step:not([hidden])") || {}).dataset.step, h1: (document.querySelector(".step:not([hidden]) h1") || {}).textContent, label: document.getElementById("steplabel").textContent, next: document.getElementById("next").textContent, back: !document.getElementById("back").hidden, focus: document.activeElement && document.activeElement.id, missing: (window.__isperErrors || []).filter(e => e.indexOf("i18n:") === 0) })'
$next = '(async () => { document.getElementById("next").click(); await new Promise(r => setTimeout(r, 900)); return "ok"; })()'

$p = EvJson 'onboarding.html' $probe
Check ($p.step -eq 'welcome' -and $p.label -like '*1*5*' -and -not $p.back) "passo 1 de 5 (boas-vindas) sem Voltar ('$($p.h1)')"
Check ($p.focus -eq 'h-welcome') "foco no titulo do passo (leitor de tela anuncia)"
Check (@($p.missing).Count -eq 0) "nenhuma chave ausente$(Format-JsErrors $p.missing)"
$w = EvJson 'onboarding.html' 'JSON.stringify({ lang: document.getElementById("uilang").value, theme: (document.querySelector("input[name=theme]:checked") || {}).value || null })'
Check ($w.lang -in 'auto', 'pt-BR', 'en' -and $w.theme -in 'system', 'light', 'dark') "idioma ($($w.lang)) e tema ($($w.theme)) atuais marcados"
if ($Shots) {
  New-Item -ItemType Directory -Force $Shots | Out-Null
  & node (Join-Path $PSScriptRoot 'cdp-shot.mjs') 'onboarding.html' (Join-Path $Shots 'onb-1-welcome.png') | Out-Null
}

# ---- 2. microfone: medidor ou aviso claro (sem microfone no runner)
EvJson 'onboarding.html' $next | Out-Null
$p = EvJson 'onboarding.html' $probe
Check ($p.step -eq 'mic' -and $p.back -and $p.focus -eq 'h-mic') "passo 2 (microfone), com Voltar e foco no titulo"
$m = $null
for ($i = 0; $i -lt 16; $i++) {
  Start-Sleep -Milliseconds 500
  $m = EvJson 'onboarding.html' 'JSON.stringify({ status: document.getElementById("micstatus").textContent, cls: document.getElementById("micstatus").className, now: Number(document.getElementById("meter").getAttribute("aria-valuenow")), fill: document.getElementById("meterfill").style.width, opts: document.querySelectorAll("#mic option").length })'
  if ($m.cls -match 'warn' -or $m.fill -and $m.fill -ne '0%' -and $m.fill -ne '0.0%') { break }
}
$measuring = $m.fill -and $m.fill -ne '0%' -and $m.fill -ne '0.0%'
Check ($measuring -or $m.cls -match 'warn') "medidor recebe nivel do microfone ou mostra aviso (barra $($m.fill); '$($m.status)')"
Check ($m.opts -ge 1) "lista de microfones com o padrao do Windows ($($m.opts) opcoes)"

# ---- 3. modelo
EvJson 'onboarding.html' $next | Out-Null
Start-Sleep -Milliseconds 600
$md = EvJson 'onboarding.html' 'JSON.stringify({ step: document.querySelector(".step:not([hidden])").dataset.step, radios: document.querySelectorAll("#models input[type=radio]").length, checked: (document.querySelector("#models input:checked") || {}).value || null, rec: document.querySelectorAll("#models .chip-accent").length, btn: document.getElementById("getmodel").textContent, engine: document.getElementById("engine").textContent, lead: document.getElementById("modellead").textContent })'
Check ($md.step -eq 'model' -and $md.radios -ge 2 -and $md.checked -and $md.rec -eq 1) "passo 3 (modelo): $($md.radios) modelos, um recomendado, '$($md.checked)' marcado"
Check ($md.btn -and $md.engine -and $md.lead) "botao '$($md.btn)' e estado do motor ('$($md.engine)')"
$inUse = EvJson 'onboarding.html' 'JSON.stringify((() => { const c = document.querySelector("#models input:checked"); const b = document.getElementById("getmodel"); return { checked: c ? c.value : null, disabled: b.disabled }; })())'
$active = (Invoke-Isper 'home_status' 'undefined' 'onboarding.html').engine
if ($active.kind -eq 'ready' -and $inUse.checked -eq $active.file) { Check $inUse.disabled "modelo carregado ja vem marcado e com o botao desativado (em uso)" }
if ($Shots) { & node (Join-Path $PSScriptRoot 'cdp-shot.mjs') 'onboarding.html' (Join-Path $Shots 'onb-3-model.png') | Out-Null }

# ---- 4. atalho: teclas, ditado de teste e trocar para o mesmo atalho (idempotente)
EvJson 'onboarding.html' $next | Out-Null
Start-Sleep -Milliseconds 600
$sc = EvJson 'onboarding.html' 'JSON.stringify({ step: document.querySelector(".step:not([hidden])").dataset.step, kbd: document.querySelectorAll("#keys kbd").length, status: document.getElementById("trystatus").textContent, sel: document.getElementById("shortcut").value })'
Check ($sc.step -eq 'shortcut' -and $sc.kbd -ge 1 -and $sc.status) "passo 4 (atalho): $($sc.kbd) teclas, '$($sc.status)'"
$st = Invoke-Isper 'onboarding_state' 'undefined' 'onboarding.html'
$pref = if ($st.shortcut) { "'$($st.shortcut)'" } else { 'null' }
$label = Invoke-Isper 'onboarding_set_shortcut' "{ shortcut: $pref }" 'onboarding.html'
Check ($label -and $label -eq $st.active_shortcut) "reaplicar o atalho atual devolve o mesmo rotulo ($label)"
if ($Shots) { & node (Join-Path $PSScriptRoot 'cdp-shot.mjs') 'onboarding.html' (Join-Path $Shots 'onb-4-shortcut.png') | Out-Null }

# ---- 5. IA
EvJson 'onboarding.html' $next | Out-Null
$ai = EvJson 'onboarding.html' 'JSON.stringify({ step: document.querySelector(".step:not([hidden])").dataset.step, radios: document.querySelectorAll("input[name=provider]").length, checked: (document.querySelector("input[name=provider]:checked") || {}).value || null, next: document.getElementById("next").textContent })'
$expected = if ($st.llm_provider) { $st.llm_provider } else { 'none' }
Check ($ai.step -eq 'ai' -and $ai.radios -eq 4 -and $ai.checked -eq $expected) "passo 5 (IA): provedor atual marcado ($($ai.checked))"

# ---- pronto, idioma na hora e Enter
EvJson 'onboarding.html' $next | Out-Null
$p = EvJson 'onboarding.html' $probe
$sum = EvJson 'onboarding.html' 'JSON.stringify(document.querySelectorAll("#summary li").length)'
Check ($p.step -eq 'done' -and $sum -eq 4 -and -not $p.back -and $p.label -eq '') "resumo final com 4 itens, sem Voltar"
$origLang = (Invoke-Isper 'get_settings' 'undefined' 'onboarding.html').ui_lang
Invoke-Isper 'set_ui_lang' "{ lang: 'en' }" 'onboarding.html' | Out-Null
Start-Sleep -Milliseconds 1200
$p = EvJson 'onboarding.html' $probe
Check ($p.lang -eq 'en' -and $p.h1 -eq 'ISPer is ready.' -and $p.next -eq 'Open ISPer') "idioma trocado na hora ('$($p.h1)' / '$($p.next)')"
Check (@($p.missing).Count -eq 0) "nenhuma chave ausente em ingles$(Format-JsErrors $p.missing)"
if ($Shots) { & node (Join-Path $PSScriptRoot 'cdp-shot.mjs') 'onboarding.html' (Join-Path $Shots 'onb-5-done-en.png') | Out-Null }
Invoke-Isper 'set_ui_lang' "{ lang: '$origLang' }" 'onboarding.html' | Out-Null
EvJson 'onboarding.html' '(async () => { document.getElementById("back").hidden = false; document.getElementById("back").click(); await new Promise(r => setTimeout(r, 700)); document.getElementById("h-ai").focus(); document.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true })); await new Promise(r => setTimeout(r, 700)); return "ok"; })()' | Out-Null
$p = EvJson 'onboarding.html' $probe
Check ($p.step -eq 'done') "Enter com foco no titulo avanca o passo"
$errs = Get-JsErrors 'onboarding.html'
Check (@($errs).Count -eq 0) "onboarding.html sem erros de JS$(Format-JsErrors $errs)"

# ---- concluir: fecha, grava e abre o Inicio
EvJson 'onboarding.html' 'document.getElementById("next").click(), "ok"' | Out-Null
Check (Wait-IsperWindow 'home.html') "Abrir o ISPer abre a tela Inicio"
Start-Sleep -Seconds 1
$targets = @(Invoke-RestMethod "http://127.0.0.1:$script:CdpPort/json" -TimeoutSec 3)
Check (-not ($targets | Where-Object { $_.url -like '*onboarding.html*' })) "a primeira configuracao fechou"
Check ((Get-OnboardingDone) -eq $true) "config.toml gravou onboarding_done = true"

# ---- reabrir pelas Configuracoes e fechar pelo X
Invoke-Isper 'open_settings_window' | Out-Null
Wait-IsperWindow 'settings.html' | Out-Null
EvJson 'settings.html' 'document.getElementById("onboarding").click(), "ok"' | Out-Null
Check (Wait-IsperWindow 'onboarding.html') "Configuracoes -> Sistema reabre a primeira configuracao"
$p = EvJson 'onboarding.html' $probe
Check ($p.step -eq 'welcome') "reaberta comeca do passo 1"
EvJson 'onboarding.html' '(async () => { await window.__TAURI__.window.getCurrentWindow().close(); return "ok"; })()' | Out-Null
Start-Sleep -Seconds 2
$targets = @(Invoke-RestMethod "http://127.0.0.1:$script:CdpPort/json" -TimeoutSec 3)
Check (-not ($targets | Where-Object { $_.url -like '*onboarding.html*' }) -and ($targets | Where-Object { $_.url -like '*home.html*' })) "o X fecha e o Inicio continua aberto"
Check ((Get-OnboardingDone) -eq $true) "onboarding_done continua true"
foreach ($win in @('home.html', 'settings.html')) {
  $errs = Get-JsErrors $win
  Check (@($errs).Count -eq 0) "$win sem erros de JS$(Format-JsErrors $errs)"
}

Restart-IsperClean -Exe $exe
Check (-not (Test-Cdp)) "relancado sem porta CDP"
Finish-E2E 'onboarding'
