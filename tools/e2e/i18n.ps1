# E2E do idioma da interface (fase 7.5) no app real, via CDP: trocar para
# ingles e de volta vale na hora nas janelas abertas (Configuracoes, Inicio,
# Biblioteca, Copilot: HTML estatico e texto montado por script), o indicador
# acompanha, nenhuma chave falta e a janela reaberta ja nasce no idioma
# salvo. Nao precisa de audio nem GPU.
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
Invoke-Isper 'open_library_window' '{ meeting: null }' | Out-Null
Wait-IsperWindow 'library.html' | Out-Null
Invoke-Isper 'open_copilot_window' | Out-Null
Wait-IsperWindow 'copilot.html' | Out-Null
Invoke-Isper 'open_settings_window' | Out-Null
Wait-IsperWindow 'settings.html' | Out-Null
Start-Sleep -Seconds 2

$probe = 'JSON.stringify({ lang: document.documentElement.lang, h1: document.querySelector("h1").textContent.trim(), save: document.getElementById("save").textContent.trim(), gpu: document.getElementById("gpuhint").textContent.trim(), diag: (document.querySelector("#diag dt") || {}).textContent || null, ph: document.getElementById("model").getAttribute("placeholder"), missing: (window.__isperErrors || []).filter(e => e.indexOf("i18n:") === 0) })'
$homeProbe = 'JSON.stringify({ lang: document.documentElement.lang, lib: document.querySelector("#b-library span").textContent.trim(), lead: document.getElementById("m-lead").textContent.trim(), meet: document.getElementById("b-meeting").textContent.trim(), todo: document.getElementById("todo-title").textContent.trim(), stat: (document.querySelector(".stat[data-key=dictations] .l") || {}).textContent || null, state: document.getElementById("statetext").textContent.trim(), aria: document.getElementById("stats").getAttribute("aria-label"), missing: (window.__isperErrors || []).filter(e => e.indexOf("i18n:") === 0) })'
$libProbe = 'JSON.stringify({ lang: document.documentElement.lang, crumb: document.querySelector(".crumb").textContent.trim(), tab: document.querySelector("#tabs .tab").textContent.trim(), ph: document.getElementById("search").getAttribute("placeholder"), sel: (document.querySelector("#detail .placeholder span") || {}).textContent || null, items: document.querySelectorAll("#list .item").length, count: document.getElementById("count").textContent.trim(), missing: (window.__isperErrors || []).filter(e => e.indexOf("i18n:") === 0) })'
$copFake = '(() => { const cards = [{ id: "c1", kind: "decision", title: "Entrega no dia 30", description: "Sem o modulo de relatorios", urgency: "high", at_secs: 45, status: "proposed" },{ id: "c2", kind: "action", title: "Enviar proposta", description: "", owner: "Carlos", urgency: "medium", at_secs: 60, status: "confirmed" },{ id: "c3", kind: "risk", title: "SLA de fim de semana", description: "", urgency: "low", at_secs: 70, status: "proposed" },{ id: "c4", kind: "question", title: "Qual o orcamento?", description: "", urgency: "low", at_secs: 80, status: "discarded" }]; render({ meeting_active: true, configured: true, running: true, error: null, last_trigger: "decision", last_updated: "10:00:00", active_topic: "", cards, memories: [{ id: "mem-1", meeting_id: 1, title: "Reuniao anterior", started_at: "2026-09-01 10:00", at_secs: 30, snippet: "prazo do MRP", score: 0.8 }], dynamics_note: null, me_talk_secs: 30, others_talk_secs: 30, monologue: false, elapsed_secs: 100, scratchpad: "" }); return JSON.stringify({ lang: document.documentElement.lang, tags: [...document.querySelectorAll(".card-kind-tag")].map(e => e.textContent.trim()), urgent: (document.querySelector(".urgency-high") || {}).textContent || null, chip: (document.querySelector(".card-item .chip-ok") || {}).textContent || null, acts: [...new Set([...document.querySelectorAll(".card-act-btn")].map(e => e.textContent.trim()))], status: document.getElementById("status-text").textContent, rec: document.getElementById("meeting-status-label").textContent, disc: document.getElementById("btn-show-discarded").textContent, topic: document.getElementById("topic-text").textContent, missing: (window.__isperErrors || []).filter(e => e.indexOf("i18n:") === 0) }); })()'
$copProbe = 'JSON.stringify({ lang: document.documentElement.lang, h1: document.querySelector("h1").textContent.trim(), tab: document.querySelector("#tab-cards span").textContent.trim(), all: document.querySelector(".filter-chip span").textContent.trim(), topic: document.getElementById("topic-text").textContent, missing: (window.__isperErrors || []).filter(e => e.indexOf("i18n:") === 0) })'
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

# ---- Inicio e Biblioteca, abertos durante a troca, refeitos em ingles
$h = EvJson 'home.html' $homeProbe
Check ($h.lang -eq 'en' -and $h.lib -eq 'Library' -and $h.lead -eq 'Record, transcribe and summarize.' -and $h.meet -eq 'Start recording a meeting') "Inicio em ingles na hora (botao '$($h.meet)')"
Check ($h.todo -in 'Needs attention', 'To get the most out of it' -and $h.stat -eq 'dictations' -and $h.aria -eq 'totals') "texto montado pelo script do Inicio em ingles (checklist '$($h.todo)', totais '$($h.stat)')"
Check ($h.state -notmatch 'pronto|carregando|nenhum|ditando|gravando') "estado do motor em ingles ('$($h.state)')"
Check (@($h.missing).Count -eq 0) "Inicio sem chave ausente$(Format-JsErrors $h.missing)"
$l = EvJson 'library.html' $libProbe
Check ($l.lang -eq 'en' -and $l.crumb -eq 'Library' -and $l.tab -eq 'Meetings' -and $l.ph -like 'search titles*') "Biblioteca em ingles na hora (placeholder '$($l.ph)')"
Check ($l.sel -eq 'Select a meeting on the left.' -and ($l.items -eq 0 -or $l.count -like '*meeting*')) "lista e detalhe da Biblioteca refeitos ('$($l.sel)', contador '$($l.count)')"
Check (@($l.missing).Count -eq 0) "Biblioteca sem chave ausente$(Format-JsErrors $l.missing)"
if ($l.items -gt 0) {
  # Reuniao aberta: meta, botoes e falantes (dado em pt-BR no banco, exibido em ingles).
  EvJson 'library.html' '(async () => { document.querySelector("#list .item").click(); await new Promise(r => setTimeout(r, 900)); return "ok"; })()' | Out-Null
  $d = EvJson 'library.html' 'JSON.stringify({ meta: (document.querySelector("#detail .meta") || {}).textContent || "", del: (document.querySelector("#detail .btn-danger") || {}).textContent || "", sp: Array.from(document.querySelectorAll("#detail .seg .sp")).map(e => e.textContent) })'
  $ptSpeaker = @($d.sp | Where-Object { $_ -eq 'Eu' -or $_ -eq 'Participantes' -or $_ -like 'Participante *' })
  Check ($d.meta -like '*click a speaker*' -and $d.del -eq 'Delete from history') "reuniao aberta em ingles (excluir '$($d.del)')"
  Check ($ptSpeaker.Count -eq 0) "falantes exibidos em ingles ($(@($d.sp | Select-Object -Unique) -join ', '))"
}
# ---- Copilot: estatico, estado ocioso e uma reuniao sintetica pelo proprio render()
$c = EvJson 'copilot.html' $copProbe
Check ($c.lang -eq 'en' -and $c.tab -eq 'Decisions' -and $c.all -eq 'All') "Copilot em ingles na hora (aba '$($c.tab)', filtro '$($c.all)')"
Check ($c.topic -like 'Waiting*') "topico inicial vem do idioma da interface ('$($c.topic)')"
$f = EvJson 'copilot.html' $copFake
Check ((@($f.tags) -join ',') -eq 'Memory,Decision,Action,Risk' -and $f.urgent -eq 'urgent' -and $f.chip -eq 'in the minutes') "cards em ingles ($(@($f.tags) -join ', '); '$($f.urgent)'; '$($f.chip)')"
Check ((@($f.acts) -join ',') -like '*Confirm*' -and (@($f.acts) -join ',') -like '*Undo*' -and (@($f.acts) -join ',') -like '*Open meeting*') "acoes dos cards em ingles ($(@($f.acts) -join ', '))"
Check ($f.status -eq 'Analyzing - agreement detected...'.Replace(' - ', [string][char]0x2014).Replace('...', [string][char]0x2026) -or ($f.status -like 'Analyzing*agreement detected*')) "gatilho da analise traduzido pelo nome estavel ('$($f.status)')"
Check ($f.rec -eq 'Recording' -and $f.disc -eq 'Show discarded (1)' -and $f.topic -eq 'In progress') "estado, descartados e topico em ingles ('$($f.rec)', '$($f.disc)', '$($f.topic)')"
Check (@($f.missing).Count -eq 0) "Copilot sem chave ausente$(Format-JsErrors $f.missing)"
if ($Shots) { & node (Join-Path $PSScriptRoot 'cdp-shot.mjs') 'copilot.html' (Join-Path $Shots 'en-copilot.png') | Out-Null }
EvJson 'copilot.html' '(async () => { render(await window.__TAURI__.core.invoke("copilot_get_state")); return "ok"; })()' | Out-Null
# reaberto em ingles: o dicionario chega no nascimento (script de inicializacao)
EvJson 'copilot.html' '(async () => { await window.__TAURI__.window.getCurrentWindow().close(); return "ok"; })()' | Out-Null
Start-Sleep -Seconds 2
Invoke-Isper 'open_copilot_window' | Out-Null
Wait-IsperWindow 'copilot.html' | Out-Null
$c = EvJson 'copilot.html' $copProbe
Check ($c.lang -eq 'en' -and $c.tab -eq 'Decisions') "Copilot reaberto ja nasce em ingles"
if ($Shots) {
  & node (Join-Path $PSScriptRoot 'cdp-shot.mjs') 'home.html' (Join-Path $Shots 'en-home.png') | Out-Null
  & node (Join-Path $PSScriptRoot 'cdp-shot.mjs') 'library.html' (Join-Path $Shots 'en-library.png') | Out-Null
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
$h = EvJson 'home.html' $homeProbe
$l = EvJson 'library.html' $libProbe
Check ($h.lang -eq 'pt-BR' -and $h.lib -eq 'Biblioteca' -and $l.crumb -eq 'Biblioteca' -and $l.tab -like 'Reuni*') "Inicio e Biblioteca de volta ao portugues"
$c = EvJson 'copilot.html' $copProbe
Check ($c.lang -eq 'pt-BR' -and $c.tab -like 'Decis*' -and $c.all -eq 'Tudo') "Copilot de volta ao portugues ('$($c.tab)')"

# ---- valor invalido e restauracao
$r = Invoke-Isper 'set_ui_lang' "{ lang: 'klingon' }" 'settings.html'
Check ($r -eq 'auto') "idioma invalido vira 'auto' ($r)"
Invoke-Isper 'set_ui_lang' "{ lang: '$orig' }" 'settings.html' | Out-Null
Check (((Invoke-Isper 'get_settings' 'undefined' 'settings.html').ui_lang) -eq $orig) "idioma de volta a '$orig'"
foreach ($w in @('home.html', 'library.html', 'settings.html', 'copilot.html')) {
  $errs = Get-JsErrors $w
  Check (@($errs).Count -eq 0) "$w sem erros de JS$(Format-JsErrors $errs)"
}

Restart-IsperClean -Exe $exe
Check (-not (Test-Cdp)) "relancado sem porta CDP"
Finish-E2E 'i18n'
