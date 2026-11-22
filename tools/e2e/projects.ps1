# E2E dos projetos (Fase 11.1) pela janela real, via CDP: em Configuracoes ->
# Projetos, criar com apelidos, renomear na linha, trazer do OptTime de
# mentira so os marcados (o de mesmo nome e ligado, nao duplicado; trazer de
# novo mostra "ja no ISPer"), arquivar com Desfazer. Na Biblioteca, a reuniao
# sem projeto chama a escolha, o seletor marca, o selo aparece na lista e o
# filtro separa por projeto e "Sem projeto" (o arquivado nao e oferecido). No
# Hoje, a tarefa ganha o projeto pelo seletor, o selo aparece e o filtro do
# dia esconde as de outro projeto. O ingles tem as chaves.
#
#   .\tools\e2e\projects.ps1 -Exe <caminho do exe>
#
# A sugestao pelo titulo (reuniao sem convite) e coberta pelos testes do
# isper-assist: aqui nao ha gravacao. Nada sai do PC: o OptTime e um processo
# node local, encerrado no fim.
#
# ASCII puro de proposito: o PowerShell 5.1 le .ps1 sem BOM como ANSI.
param([string]$Exe)
. "$PSScriptRoot\common.ps1"

$exe = Get-IsperExe $Exe
"projects: $exe"

$token = 'opt_tok_e2e_' + [guid]::NewGuid().ToString('N').Substring(0, 8)
$listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
$listener.Start()
$port = $listener.LocalEndpoint.Port
$listener.Stop()
$url = "http://127.0.0.1:$port/api/mcp"
$fake = Start-Process node -ArgumentList @("`"$PSScriptRoot\fake-opttime.mjs`"", "$port", $token) -PassThru -WindowStyle Hidden
$up = $false
for ($i = 0; $i -lt 40 -and -not $up; $i++) {
  try { Invoke-RestMethod "http://127.0.0.1:$port/calls" -TimeoutSec 5 | Out-Null; $up = $true } catch { Start-Sleep -Milliseconds 250 }
}
Check $up "o OptTime de mentira responde ($url)"

# Espera ate o estado lido por $Js passar em $Ok (ou o tempo acabar).
function Wait-Js {
  param([string]$Window, [string]$Js, [scriptblock]$Ok, [int]$Seconds = 12)
  $sw = [Diagnostics.Stopwatch]::StartNew()
  do {
    $s = EvJson $Window $Js
    if ($s -and (& $Ok $s)) { return $s }
    Start-Sleep -Milliseconds 300
  } while ($sw.Elapsed.TotalSeconds -lt $Seconds)
  $s
}
$lastToast = '(q => (q.pop() || {}).textContent || null)([...document.querySelectorAll(".toast")])'
$setJs = 'JSON.stringify({ rows: [...document.querySelectorAll("#projlist .proj")].map(r => ({ name: r.querySelector(".mem-text").textContent, aliases: r.querySelector(".proj-aliases").textContent, chips: [...r.querySelectorAll(".chip")].map(c => c.textContent), color: (r.querySelector(".proj-color") || {}).value || null })), archived: [...document.querySelectorAll("#projarch .proj .mem-text")].map(x => x.textContent), ot: !document.getElementById("projotrow").hidden, otbox: !document.getElementById("projotbox").hidden, otlist: [...document.querySelectorAll("#projotlist label")].map(l => ({ text: l.textContent, checked: l.querySelector("input").checked, disabled: l.querySelector("input").disabled })), status: document.getElementById("status").textContent, toast: ' + $lastToast + ' })'
function Get-Set { EvJson 'settings.html' $setJs }
function Find-Proj($s, [string]$Name) { @($s.rows) | Where-Object { $_.name -eq $Name } | Select-Object -First 1 }
function New-Proj([string]$Name, [string]$Aliases) {
  $n = ConvertTo-Json $Name; $a = ConvertTo-Json $Aliases
  EvJson 'settings.html' ('JSON.stringify((() => { document.getElementById("projname").value = ' + $n + '; document.getElementById("projaliases").value = ' + $a + '; document.getElementById("projadd").requestSubmit(); return true; })())') | Out-Null
}
# Clica no botao (pelo texto) da linha do projeto.
function Click-Proj([string]$Name, [string]$Label) {
  $n = ConvertTo-Json $Name
  EvJson 'settings.html' ('JSON.stringify((() => { const r = [...document.querySelectorAll(".proj")].find(x => x.querySelector(".mem-text").textContent === ' + $n + '); if (!r) return false; const b = [...r.querySelectorAll("button")].find(x => x.textContent === "' + $Label + '"); if (!b) return false; b.click(); return true; })())')
}

$libJs = 'JSON.stringify({ filter: document.getElementById("projfilter").hidden ? null : [...document.getElementById("projfilter").options].map(o => o.textContent), items: [...document.querySelectorAll("#list .item")].map(it => ({ title: it.querySelector(".t").textContent, proj: (it.querySelector(".badge.proj") || {}).textContent || null })), sel: (s => s ? { value: s.value, text: s.options[s.selectedIndex].textContent, unset: s.classList.contains("unset"), options: [...s.options].map(o => o.textContent) } : null)(document.getElementById("mproject")), chips: [...document.querySelectorAll(".dproj .chip")].map(c => c.textContent), toast: ' + $lastToast + ' })'
function Get-Lib { EvJson 'library.html' $libJs }
function Set-Select([string]$Window, [string]$Selector, [string]$OptionText) {
  $o = ConvertTo-Json $OptionText
  EvJson $Window ('JSON.stringify((() => { const s = document.querySelector("' + $Selector + '"); if (!s) return false; const o = [...s.options].find(x => x.textContent === ' + $o + '); if (!o) return false; s.value = o.value; s.dispatchEvent(new Event("change")); return true; })())')
}
function Pick-Meeting([string]$Title) {
  $t = ConvertTo-Json $Title
  EvJson 'library.html' ('JSON.stringify((() => { const it = [...document.querySelectorAll("#list .item")].find(x => x.querySelector(".t").textContent === ' + $t + '); if (!it) return false; it.click(); return true; })())') | Out-Null
}

$todayJs = 'JSON.stringify({ filter: document.getElementById("projfilter").hidden ? null : [...document.getElementById("projfilter").options].map(o => o.textContent), planned: [...document.querySelectorAll("#list-planned > li.task")].map(li => ({ title: li.querySelector(".title").textContent, proj: (li.querySelector(".chip.proj") || {}).textContent || null, select: !!li.querySelector(".acts select") })), toast: ' + $lastToast + ' })'
function Get-Today { EvJson 'today.html' $todayJs }

try {
  Stop-Isper
  Check (Start-Isper -Exe $exe) "app abriu com a tela Inicio (CDP)"
  Invoke-Isper 'set_ui_lang' "{ lang: 'pt-BR' }" | Out-Null
  Invoke-Isper 'opttime_clear_token' | Out-Null
  Invoke-Isper 'opttime_set_url' "{ url: null }" | Out-Null

  # Duas reunioes sem projeto, direto no banco do perfil de teste (aqui nao
  # se grava). Sem aspas duplas no codigo: o PowerShell 5.1 as perde.
  $db = Get-E2EDataPath 'isper.db'
  $py = "import sqlite3,sys; c=sqlite3.connect(sys.argv[1]); c.execute('DELETE FROM meetings WHERE id IN (9001, 9002)'); c.executemany('INSERT INTO meetings (id, title, started_at, duration_secs, started_ts) VALUES (?, ?, ?, 1800, ?)', [(9001, 'Revisao do backlog', '20/11/2026 10:00', 1763643600000), (9002, 'Conversa avulsa', '21/11/2026 15:00', 1763748000000)]); c.commit(); print(c.total_changes)"
  $n = & python -c $py $db
  Check ("$n".Trim() -eq '2') "duas reunioes de teste no banco ($n)"

  # 1) Configuracoes -> Projetos: vazio, criar com apelidos.
  Invoke-Isper 'open_settings_window' "{ section: 'projetos' }" | Out-Null
  Wait-IsperWindow 'settings.html' | Out-Null
  $s0 = Get-Set
  Check (@($s0.rows).Count -eq 0 -and -not $s0.ot) "sem projetos e, sem token, sem 'Trazer do OptTime'"
  New-Proj 'Portal' 'PRT, prt, Portal'
  $s1 = Wait-Js 'settings.html' $setJs { param($s) Find-Proj $s 'Portal' }
  $portal = Find-Proj $s1 'Portal'
  Check ($portal -and $portal.aliases -eq 'Apelidos: PRT' -and $portal.color -like '#*') "Portal criado com o apelido limpo ('$($portal.aliases)', cor $($portal.color))"
  New-Proj 'MRP' ''
  $s2 = Wait-Js 'settings.html' $setJs { param($s) Find-Proj $s 'MRP' }
  Check ((Find-Proj $s2 'MRP').aliases -like 'Sem apelidos*') "MRP sem apelidos convida a por"
  New-Proj 'portal' ''
  $s2b = Wait-Js 'settings.html' $setJs { param($s) $s.status -like '*existe*' }
  Check ($s2b.status -like '*existe*' -and @($s2b.rows).Count -eq 2) "o mesmo nome e recusado ('$($s2b.status)')"

  # 2) Renomear na linha: Portal -> Portal do Cliente.
  EvJson 'settings.html' 'JSON.stringify((() => { const r = [...document.querySelectorAll(".proj")].find(x => x.querySelector(".mem-text").textContent === "Portal"); r.querySelector(".mem-text").click(); const i = document.querySelector(".proj-edit"); i.value = "Portal do Cliente"; i.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true })); return true; })())' | Out-Null
  $s3 = Wait-Js 'settings.html' $setJs { param($s) Find-Proj $s 'Portal do Cliente' }
  Check ((Find-Proj $s3 'Portal do Cliente') -and -not (Find-Proj $s3 'Portal')) "renomear na linha grava"

  # 3) Trazer do OptTime: so os marcados; o de mesmo nome e ligado.
  Invoke-Isper 'opttime_set_url' "{ url: '$url' }" 'settings.html' | Out-Null
  Invoke-Isper 'opttime_set_token' "{ token: '$token' }" 'settings.html' | Out-Null
  $s4 = Wait-Js 'settings.html' $setJs { param($s) $s.ot }
  Check $s4.ot "com o token, aparece 'Trazer do OptTime'"
  Ev 'settings.html' 'document.getElementById("projot").click()' | Out-Null
  $s5 = Wait-Js 'settings.html' $setJs { param($s) $s.otbox -and @($s.otlist).Count -eq 2 }
  Check (@($s5.otlist).Count -eq 2 -and -not (@($s5.otlist) | Where-Object { $_.checked })) "a lista do OptTime vem com 2, nenhum marcado ($(@($s5.otlist | ForEach-Object { $_.text }) -join ' | '))"
  Ev 'settings.html' 'document.querySelectorAll("#projotlist input").forEach(c => { c.checked = true; }); document.getElementById("projotbring").click()' | Out-Null
  $s6 = Wait-Js 'settings.html' $setJs { param($s) -not $s.otbox -and @($s.rows).Count -eq 3 }
  $pc = Find-Proj $s6 'Portal do Cliente'
  Check (@($s6.rows).Count -eq 3 -and ($pc.chips -contains 'do OptTime') -and $pc.aliases -like '*PRT*') "Portal do Cliente e ligado (nao duplica) e Estoque e criado ($(@($s6.rows | ForEach-Object { $_.name }) -join ', '))"
  Check ((Find-Proj $s6 'Estoque').color -like '#*') "a cor que nao e #rrggbb vai para a paleta ($((Find-Proj $s6 'Estoque').color))"
  Ev 'settings.html' 'document.getElementById("projot").click()' | Out-Null
  $s7 = Wait-Js 'settings.html' $setJs { param($s) $s.otbox -and @($s.otlist).Count -eq 2 }
  Check (@(@($s7.otlist) | Where-Object { $_.checked -and $_.disabled -and $_.text -like '*no ISPer' }).Count -eq 2) "trazer de novo mostra os 2 como ja no ISPer"
  Ev 'settings.html' 'document.getElementById("projotcancel").click()' | Out-Null

  # 4) Arquivar com Desfazer, e arquivar de vez.
  Click-Proj 'Estoque' 'Arquivar' | Out-Null
  $s8 = Wait-Js 'settings.html' $setJs { param($s) @($s.archived).Count -eq 1 }
  Check (@($s8.archived) -contains 'Estoque' -and $s8.toast -like 'Projeto arquivado*') "Arquivar leva Estoque para os arquivados ('$($s8.toast)')"
  EvJson 'settings.html' 'JSON.stringify((() => { const b = [...document.querySelectorAll(".toast-act")].pop(); if (!b) return false; b.click(); return true; })())' | Out-Null
  $s9 = Wait-Js 'settings.html' $setJs { param($s) @($s.archived).Count -eq 0 -and @($s.rows).Count -eq 3 }
  Check (@($s9.archived).Count -eq 0 -and (Find-Proj $s9 'Estoque')) "o Desfazer traz Estoque de volta"
  Click-Proj 'Estoque' 'Arquivar' | Out-Null
  Wait-Js 'settings.html' $setJs { param($s) @($s.archived).Count -eq 1 } | Out-Null

  # 5) Biblioteca: a reuniao sem projeto chama a escolha; marcar; o selo.
  Invoke-Isper 'open_library_window' '{ meeting: null }' | Out-Null
  Wait-IsperWindow 'library.html' | Out-Null
  $l0 = Wait-Js 'library.html' $libJs { param($s) $s.filter -and @($s.items).Count -ge 2 }
  Check ((@($l0.filter) -join '|') -eq 'Todos os projetos|MRP|Portal do Cliente|Sem projeto') "o filtro tem os ativos e Sem projeto, sem o arquivado ($(@($l0.filter) -join ' | '))"
  Pick-Meeting 'Revisao do backlog'
  $l1 = Wait-Js 'library.html' $libJs { param($s) $s.sel }
  Check ($l1.sel.unset -and $l1.sel.text -eq 'Sem projeto: escolha qual' -and -not (@($l1.sel.options) -contains 'Estoque')) "sem projeto, o seletor chama a escolha ('$($l1.sel.text)')"
  Check (Set-Select 'library.html' '#mproject' 'MRP') "escolher MRP no seletor"
  $l2 = Wait-Js 'library.html' $libJs { param($s) (@($s.items) | Where-Object { $_.title -eq 'Revisao do backlog' }).proj -eq 'MRP' }
  Check ((@($l2.items) | Where-Object { $_.title -eq 'Revisao do backlog' }).proj -eq 'MRP' -and $l2.toast -like '*MRP*') "o selo MRP aparece na lista ('$($l2.toast)')"
  $mp = Invoke-Isper 'meeting_project_get' '{ meetingId: 9001 }' 'library.html'
  Check ($mp.project_id -and -not $mp.suggested) "marcado pela pessoa: nao e sugestao"
  Check ($l2.sel -and -not $l2.sel.unset -and -not (@($l2.chips) -contains 'sugerido')) "o seletor sai do destaque, sem 'sugerido'"

  # 6) O filtro: Sem projeto, um projeto, todos.
  Set-Select 'library.html' '#projfilter' 'Sem projeto' | Out-Null
  $f1 = Wait-Js 'library.html' $libJs { param($s) -not (@($s.items) | Where-Object { $_.title -eq 'Revisao do backlog' }) }
  Check ((@($f1.items) | Where-Object { $_.title -eq 'Conversa avulsa' }) -and -not (@($f1.items) | Where-Object { $_.title -eq 'Revisao do backlog' })) "Sem projeto mostra so as sem projeto"
  Set-Select 'library.html' '#projfilter' 'MRP' | Out-Null
  $f2 = Wait-Js 'library.html' $libJs { param($s) @($s.items).Count -eq 1 }
  Check (@($f2.items).Count -eq 1 -and @($f2.items)[0].title -eq 'Revisao do backlog') "MRP mostra so a dele"
  Set-Select 'library.html' '#projfilter' 'Todos os projetos' | Out-Null
  $f3 = Wait-Js 'library.html' $libJs { param($s) @($s.items).Count -ge 2 }
  Check (@($f3.items).Count -ge 2) "Todos volta a lista inteira"

  # 7) Hoje: a tarefa ganha o projeto, o selo e o filtro do dia.
  Invoke-Isper 'task_add_text' "{ text: 'Revisar backlog com o time', fallback: null }" 'library.html' | Out-Null
  Invoke-Isper 'task_add_text' "{ text: 'Planilha do estoque', fallback: null }" 'library.html' | Out-Null
  Invoke-Isper 'open_today_window' | Out-Null
  Wait-IsperWindow 'today.html' | Out-Null
  $t0 = Wait-Js 'today.html' $todayJs { param($s) @($s.planned).Count -ge 2 -and $s.filter }
  Check ((@($t0.planned) | Where-Object { $_.title -eq 'Revisar backlog com o time' }).select) "a tarefa tem o seletor de projeto"
  EvJson 'today.html' 'JSON.stringify((() => { const li = [...document.querySelectorAll("#list-planned > li.task")].find(x => x.querySelector(".title").textContent === "Revisar backlog com o time"); const s = li.querySelector(".acts select"); s.value = [...s.options].find(o => o.textContent === "Portal do Cliente").value; s.dispatchEvent(new Event("change")); return true; })())' | Out-Null
  $t1 = Wait-Js 'today.html' $todayJs { param($s) (@($s.planned) | Where-Object { $_.title -eq 'Revisar backlog com o time' }).proj }
  Check ((@($t1.planned) | Where-Object { $_.title -eq 'Revisar backlog com o time' }).proj -eq 'Portal do Cliente') "o selo Portal do Cliente aparece na tarefa ('$($t1.toast)')"
  Set-Select 'today.html' '#projfilter' 'MRP' | Out-Null
  $t2 = Wait-Js 'today.html' $todayJs { param($s) -not (@($s.planned) | Where-Object { $_.title -eq 'Revisar backlog com o time' }) }
  Check (-not (@($t2.planned) | Where-Object { $_.title -like 'Revisar*' -or $_.title -like 'Planilha*' })) "o filtro MRP esconde as de outro projeto e as sem projeto"
  Set-Select 'today.html' '#projfilter' 'Sem projeto' | Out-Null
  $t3 = Wait-Js 'today.html' $todayJs { param($s) (@($s.planned) | Where-Object { $_.title -eq 'Planilha do estoque' }) }
  Check ((@($t3.planned) | Where-Object { $_.title -eq 'Planilha do estoque' }) -and -not (@($t3.planned) | Where-Object { $_.title -like 'Revisar*' })) "Sem projeto mostra a Planilha e nao a do Portal"
  Set-Select 'today.html' '#projfilter' 'Todos os projetos' | Out-Null

  # 8) Em ingles, as chaves existem.
  Invoke-Isper 'set_ui_lang' "{ lang: 'en' }" | Out-Null
  Start-Sleep -Milliseconds 800
  $en = EvJson 'settings.html' 'JSON.stringify({ h: document.getElementById("h-projetos").textContent, ph: document.getElementById("projname").placeholder })'
  Check ($en.h -eq 'Projects' -and $en.ph -eq 'Project name') "em ingles: '$($en.h)', '$($en.ph)'"
  Invoke-Isper 'set_ui_lang' "{ lang: 'pt-BR' }" | Out-Null

  foreach ($w in 'settings.html', 'library.html', 'today.html') {
    $errs = @(Get-JsErrors $w)
    Check (@($errs | Where-Object { $_ }).Count -eq 0) "$w sem erros de JS$(Format-JsErrors $errs)"
  }
} finally {
  if ($fake -and -not $fake.HasExited) { Stop-Process -Id $fake.Id -Force -ErrorAction SilentlyContinue }
}

Restart-IsperClean -Exe $exe
Check (-not (Test-Cdp)) "relancado sem porta CDP"
Finish-E2E 'projects'
