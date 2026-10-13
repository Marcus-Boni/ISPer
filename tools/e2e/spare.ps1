# E2E das listas "Quando der tempo" (Fase 11.0) pela janela real, via CDP:
# sem lista, o convite para criar; "+ Nova lista" cria pelo nome; colar varias
# linhas vira um item por linha (sem os marcadores) e o item digitado leva a
# estimativa escolhida; nada disso entra no dia nem no numero da barra. O selo
# da estimativa troca no clique, o item muda de lista, vai para hoje e volta
# para a lista. O item sem mudanca ha 60 dias fica "parado" e "Manter" o tira
# de la. Com o OptTime de mentira (agenda relativa a hora do teste), o cartao
# "Tempo livre" sugere um item que cabe, "Outra" troca e "Fazer agora" poe no
# dia. Arquivar a lista tem Desfazer; renomear grava; o ingles tem as chaves.
#
#   .\tools\e2e\spare.ps1 -Exe <caminho do exe> [-Shots <pasta>]
#
# O tempo livre usa o fim do expediente do teste (23:59): roda ate umas 22h.
# Nada sai do PC: o OptTime e um processo node local, encerrado no fim.
#
# ASCII puro de proposito: o PowerShell 5.1 le .ps1 sem BOM como ANSI.
param([string]$Exe, [string]$Shots)
. "$PSScriptRoot\common.ps1"

$exe = Get-IsperExe $Exe
"spare: $exe"

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

$spareJs = 'JSON.stringify((() => { const q = (s) => [...document.querySelectorAll(s)]; return { open: document.getElementById("spare-box").open, count: document.getElementById("c-spare").textContent, tabs: q("#spare-tabs .pick").map(b => ({ name: b.title, text: b.textContent, on: b.getAttribute("aria-checked") === "true" })), form: !document.getElementById("ladd").hidden, empty: (document.querySelector("#list-spare .empty") || {}).textContent || null, review: document.getElementById("spare-review").hidden ? null : document.getElementById("spare-review").textContent, items: q("#list-spare > li.task").map(li => ({ title: li.querySelector(".title").textContent, chips: [...li.querySelectorAll(".meta .chip")].map(c => c.textContent), buttons: [...li.querySelectorAll(".acts button")].map(b => b.textContent), move: !!li.querySelector(".acts select") })), planned: q("#list-planned > li.task").map(li => ({ title: li.querySelector(".title").textContent, chips: [...li.querySelectorAll(".meta .chip")].map(c => c.textContent), buttons: [...li.querySelectorAll(".acts button")].map(b => b.textContent) })), later: q("#list-later > li.task .title").map(x => x.textContent), tip: document.getElementById("sec-spare-tip").hidden ? null : { title: (document.querySelector("#spare-tip .sug-title") || {}).textContent, why: (document.querySelector("#spare-tip .sug-why") || {}).textContent, buttons: q("#spare-tip button").map(b => b.textContent) }, toast: (q(".toast").pop() || {}).textContent || null }; })())'
function Get-Spare { EvJson 'today.html' $spareJs }
function Wait-Spare {
  param([scriptblock]$Ok, [int]$Seconds = 12)
  $sw = [Diagnostics.Stopwatch]::StartNew()
  do {
    $s = Get-Spare
    if ($s -and (& $Ok $s)) { return $s }
    Start-Sleep -Milliseconds 300
  } while ($sw.Elapsed.TotalSeconds -lt $Seconds)
  $s
}
function Find-Item($s, [string]$Title) { @($s.items) | Where-Object { $_.title -eq $Title } | Select-Object -First 1 }
# Clica no botao (pelo comeco do texto) da linha com o titulo, numa lista da tela.
function Click-Row([string]$List, [string]$Title, [string]$Prefix) {
  $t = ConvertTo-Json $Title
  EvJson 'today.html' ('JSON.stringify((() => { const li = [...document.querySelectorAll("#' + $List + ' > li.task")].find(x => x.querySelector(".title").textContent === ' + $t + '); if (!li) return false; const b = [...li.querySelectorAll("button")].find(x => x.textContent.startsWith("' + $Prefix + '")); if (!b) return false; b.click(); return true; })())')
}
# Escreve o nome no campo que aparece no lugar do botao e aperta Enter.
function Type-Name([string]$Name) {
  $n = ConvertTo-Json $Name
  EvJson 'today.html' ('JSON.stringify((() => { const i = document.querySelector(".lbar input.edit"); if (!i) return false; i.value = ' + $n + '; i.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true })); return true; })())')
}
function Add-Items([string]$Text) {
  $v = ConvertTo-Json $Text
  EvJson 'today.html' ('JSON.stringify((() => { document.getElementById("l-text").value = ' + $v + '; document.getElementById("ladd").requestSubmit(); return true; })())') | Out-Null
}
# A tela com a secao a vista, para conferir o desenho (so com -Shots).
function Shot([string]$Id, [string]$File) {
  if (-not $Shots) { return }
  New-Item -ItemType Directory -Force $Shots | Out-Null
  Ev 'today.html' ('document.getElementById("' + $Id + '").scrollIntoView({ block: "start" })') | Out-Null
  Start-Sleep -Milliseconds 400
  & node (Join-Path $PSScriptRoot 'cdp-shot.mjs') 'today.html' (Join-Path $Shots $File) | Out-Null
}
function Pick-Tab([string]$Name) {
  EvJson 'today.html' ('JSON.stringify((() => { const b = [...document.querySelectorAll("#spare-tabs .pick")].find(x => x.title === "' + $Name + '"); if (!b) return false; b.click(); return true; })())') | Out-Null
}

try {
  Stop-Isper
  Check (Start-Isper -Exe $exe) "app abriu com a tela Inicio (CDP)"
  Invoke-Isper 'set_ui_lang' "{ lang: 'pt-BR' }" | Out-Null
  Invoke-Isper 'opttime_clear_token' | Out-Null
  Invoke-Isper 'opttime_set_url' "{ url: null }" | Out-Null
  Invoke-Isper 'open_today_window' | Out-Null
  Wait-IsperWindow 'today.html' | Out-Null
  Ev 'today.html' 'document.getElementById("spare-box").open = true' | Out-Null

  # 1) Sem lista: o convite, sem o campo.
  $s0 = Wait-Spare { param($s) $s.open -and $s.empty }
  Check ($s0.empty -like 'Crie uma lista*' -and -not $s0.form -and @($s0.tabs).Count -eq 0) "sem lista, o convite para criar e nenhum campo ('$($s0.empty)')"

  # 2) "+ Nova lista": o nome no lugar do botao, Enter cria e seleciona.
  Ev 'today.html' 'document.getElementById("spare-new").click()' | Out-Null
  Check (Type-Name 'Estudar') "o campo do nome aparece no lugar do botao"
  $s1 = Wait-Spare { param($s) @($s.tabs).Count -eq 1 -and $s.form }
  Check (@($s1.tabs)[0].name -eq 'Estudar' -and @($s1.tabs)[0].on -and $s1.form) "a lista Estudar e criada, selecionada e com o campo"
  Ev 'today.html' 'document.getElementById("spare-new").click()' | Out-Null
  Type-Name 'Ler' | Out-Null
  $s1b = Wait-Spare { param($s) @($s.tabs).Count -eq 2 }
  Check ((@($s1b.tabs) | Where-Object { $_.on }).name -eq 'Ler') "e a segunda, Ler, fica selecionada"
  Pick-Tab 'Estudar'

  # 3) Colar varias linhas: um item por linha, sem os marcadores.
  Add-Items "- Ler sobre CRDT`n* [ ] Curso de Rust async`n`n1. Video do Jon Gjengset"
  $s2 = Wait-Spare { param($s) @($s.items).Count -ge 3 }
  $titles = @($s2.items | ForEach-Object { $_.title })
  Check ($titles.Count -eq 3 -and ($titles -contains 'Ler sobre CRDT') -and ($titles -contains 'Curso de Rust async') -and ($titles -contains 'Video do Jon Gjengset')) "3 linhas coladas viram 3 itens limpos ($($titles -join ' | '))"
  Check ($s2.toast -like '*3 itens na lista*') "o aviso conta os itens ('$($s2.toast)')"

  # 4) O item digitado leva a estimativa escolhida.
  Ev 'today.html' '[...document.querySelectorAll("#l-est .pick")].find(b => b.dataset.min === "30").click()' | Out-Null
  Add-Items 'Artigo de Raft'
  $s3 = Wait-Spare { param($s) $null -ne (Find-Item $s 'Artigo de Raft') }
  $raft = Find-Item $s3 'Artigo de Raft'
  Check (($raft.chips -contains '~30min') -and $s3.count -eq '4') "com o selo ~30min, e a secao conta 4 ($(@($raft.chips) -join ', '))"
  Ev 'today.html' '[...document.querySelectorAll("#l-est .pick")].find(b => b.dataset.min === "30").click()' | Out-Null
  Shot 'sec-spare' 'spare-lists.png'

  # 5) Fora do dia: nem Para hoje, nem Depois, nem o numero da barra.
  $v = Invoke-Isper 'today_load' 'undefined' 'today.html'
  Check (@($v.planned).Count -eq 0 -and @($v.later).Count -eq 0 -and @($v.inbox).Count -eq 0) "os itens nao entram em Para hoje, Depois nem na caixa de entrada"
  Check ((Invoke-Isper 'today_badge' 'undefined' 'today.html') -eq 0) "e o numero da barra continua 0"
  Check (@($v.lists).Count -eq 2 -and @(@($v.lists) | Where-Object { $_.name -eq 'Estudar' })[0].items.Count -eq 4) "today_load traz as 2 listas com os itens"

  # 6) O selo da estimativa troca no clique (sem -> 15min).
  Click-Row 'list-spare' 'Ler sobre CRDT' 'quanto' | Out-Null
  $s4 = Wait-Spare { param($s) (Find-Item $s 'Ler sobre CRDT').chips -contains '~15min' }
  Check ((Find-Item $s4 'Ler sobre CRDT').chips -contains '~15min') "clicar em 'quanto leva?' poe ~15min"

  # 7) Mudar de lista.
  Check ((Find-Item $s4 'Video do Jon Gjengset').move) "com duas listas, o item tem o seletor de lista"
  EvJson 'today.html' 'JSON.stringify((() => { const li = [...document.querySelectorAll("#list-spare > li.task")].find(x => x.querySelector(".title").textContent === "Video do Jon Gjengset"); const s = li.querySelector(".acts select"); s.value = [...s.options].find(o => o.textContent === "Ler").value; s.dispatchEvent(new Event("change")); return true; })())' | Out-Null
  $s5 = Wait-Spare { param($s) $null -eq (Find-Item $s 'Video do Jon Gjengset') }
  Check ($null -eq (Find-Item $s5 'Video do Jon Gjengset') -and $s5.toast -like '*Ler*') "o item sai de Estudar ('$($s5.toast)')"
  Pick-Tab 'Ler'
  $s5b = Wait-Spare { param($s) $null -ne (Find-Item $s 'Video do Jon Gjengset') }
  Check ($null -ne (Find-Item $s5b 'Video do Jon Gjengset')) "e aparece em Ler"
  Pick-Tab 'Estudar'

  # 8) Hoje e de volta para a lista.
  Click-Row 'list-spare' 'Curso de Rust async' 'Hoje' | Out-Null
  $s6 = Wait-Spare { param($s) @($s.planned).Count -ge 1 }
  $p = @($s6.planned) | Where-Object { $_.title -eq 'Curso de Rust async' }
  Check ($null -ne $p -and $null -eq (Find-Item $s6 'Curso de Rust async')) "Hoje leva o item para Para hoje"
  Check (($p.chips -contains 'Estudar') -and (@($p.buttons) | Where-Object { $_ -like 'Voltar*' }).Count -eq 1) "com o selo da lista e o Voltar a lista ($(@($p.buttons) -join ', '))"
  Check ((Invoke-Isper 'today_badge' 'undefined' 'today.html') -eq 1) "agora conta no numero da barra"
  Click-Row 'list-planned' 'Curso de Rust async' 'Voltar' | Out-Null
  $s7 = Wait-Spare { param($s) $null -ne (Find-Item $s 'Curso de Rust async') }
  Check ($null -ne (Find-Item $s7 'Curso de Rust async') -and @($s7.planned).Count -eq 0) "Voltar a lista devolve o item sem dia"

  # 9) Parado: 61 dias sem mudanca (o banco do perfil de teste, direto).
  $db = Get-E2EDataPath 'isper.db'
  # Sem aspas duplas no codigo: o PowerShell 5.1 as perde nos argumentos de exe.
  $py = "import sqlite3,sys; c=sqlite3.connect(sys.argv[1]); c.execute('UPDATE tasks SET updated_at = updated_at - 61*86400000 WHERE title = ?', (sys.argv[2],)); c.commit(); print(c.total_changes)"
  $n = & python -c $py $db 'Artigo de Raft'
  Check ("$n".Trim() -eq '1') "o item envelhece 61 dias no banco de teste"
  Ev 'today.html' 'sig = ""; refresh()' | Out-Null
  $s8 = Wait-Spare { param($s) $s.review }
  $old = Find-Item $s8 'Artigo de Raft'
  Check (($old.chips -contains 'parado') -and ($old.buttons -contains 'Manter')) "fica 'parado', com Manter ($(@($old.chips) -join ', '))"
  Check ($s8.review -eq 'Revisar parado (1)') "e aparece 'Revisar parado (1)' ('$($s8.review)')"
  Ev 'today.html' 'document.getElementById("spare-review").click()' | Out-Null
  $s9 = Wait-Spare { param($s) @($s.items).Count -eq 1 }
  Check (@($s9.items).Count -eq 1 -and @($s9.items)[0].title -eq 'Artigo de Raft' -and -not $s9.form) "Revisar mostra so o parado, sem o campo"
  Click-Row 'list-spare' 'Artigo de Raft' 'Manter' | Out-Null
  $s10 = Wait-Spare { param($s) -not $s.review -and @($s.items).Count -ge 3 }
  Check (-not $s10.review -and -not ((Find-Item $s10 'Artigo de Raft').chips -contains 'parado')) "Manter tira o selo e a revisao some"

  # 10) Tempo livre: o OptTime de mentira tem reunioes ate +90 min, livre ate +180.
  Invoke-Isper 'opttime_set_url' "{ url: '$url' }" | Out-Null
  Invoke-Isper 'opttime_set_token' "{ token: '$token' }" | Out-Null
  Ev 'today.html' 'spareDayEnd = "23:59:00"; loadTip()' | Out-Null
  $t1 = Wait-Spare { param($s) $s.tip }
  Check ($null -ne $t1.tip -and $t1.tip.why -like 'Livre das *' -and $t1.tip.why -like '*de Estudar*') "o cartao Tempo livre sugere um item de Estudar ('$($t1.tip.title)' - '$($t1.tip.why)')"
  Check ((@($t1.tip.buttons) -contains 'Fazer agora') -and (@($t1.tip.buttons) -contains 'Outra')) "com Fazer agora e Outra"
  $first = $t1.tip.title
  Shot 'sec-spare-tip' 'spare-tip.png'
  EvJson 'today.html' 'JSON.stringify((() => { [...document.querySelectorAll("#spare-tip button")].find(b => b.textContent === "Outra").click(); return true; })())' | Out-Null
  $t2 = Wait-Spare { param($s) $s.tip -and $s.tip.title -ne $first }
  Check ($t2.tip.title -and $t2.tip.title -ne $first) "Outra troca o item ('$first' -> '$($t2.tip.title)')"
  $second = $t2.tip.title
  EvJson 'today.html' 'JSON.stringify((() => { [...document.querySelectorAll("#spare-tip button")].find(b => b.textContent === "Fazer agora").click(); return true; })())' | Out-Null
  $t3 = Wait-Spare { param($s) -not $s.tip -and @($s.planned).Count -ge 1 }
  Check (-not $t3.tip -and (@($t3.planned) | Where-Object { $_.title -eq $second })) "Fazer agora poe o item em Para hoje e o cartao sai"

  # 11) Arquivar com Desfazer; renomear.
  Pick-Tab 'Ler'
  Ev 'today.html' 'document.getElementById("spare-archive").click()' | Out-Null
  $a1 = Wait-Spare { param($s) @($s.tabs).Count -eq 1 }
  Check (@($a1.tabs).Count -eq 1 -and $a1.toast -like '*arquivada*') "Arquivar tira a lista Ler ('$($a1.toast)')"
  EvJson 'today.html' 'JSON.stringify((() => { const b = [...document.querySelectorAll(".toast-act")].pop(); if (!b) return false; b.click(); return true; })())' | Out-Null
  $a2 = Wait-Spare { param($s) @($s.tabs).Count -eq 2 }
  # O item de Ler pode ter ido para o dia pelo "Fazer agora": confere a lista.
  Check (@(@($a2.tabs) | Where-Object { $_.name -eq 'Ler' -and $_.on }).Count -eq 1) "o Desfazer traz a lista de volta, selecionada ($((@($a2.tabs) | ForEach-Object { $_.name + '=' + $_.on }) -join ', '))"
  Pick-Tab 'Estudar'
  Ev 'today.html' 'document.getElementById("spare-rename").click()' | Out-Null
  Type-Name 'Estudos' | Out-Null
  $a3 = Wait-Spare { param($s) @(@($s.tabs) | Where-Object { $_.name -eq 'Estudos' }).Count -eq 1 }
  Check ((@($a3.tabs) | Where-Object { $_.name -eq 'Estudos' }).on) "Renomear grava e mantem a lista selecionada"

  # 12) Em ingles, as chaves existem.
  Invoke-Isper 'set_ui_lang' "{ lang: 'en' }" | Out-Null
  Start-Sleep -Milliseconds 800
  $en = EvJson 'today.html' 'JSON.stringify({ h: document.getElementById("h-spare").textContent, nw: document.getElementById("spare-new").textContent, ph: document.getElementById("l-text").placeholder })'
  Check ($en.h -eq "When there's time" -and $en.nw -eq '+ New list' -and $en.ph -like 'New item*') "em ingles: '$($en.h)', '$($en.nw)'"
  Invoke-Isper 'set_ui_lang' "{ lang: 'pt-BR' }" | Out-Null

  $errs = @(Get-JsErrors 'today.html')
  Check (@($errs | Where-Object { $_ }).Count -eq 0) "Hoje sem erros de JS$(Format-JsErrors $errs)"
} finally {
  if ($fake -and -not $fake.HasExited) { Stop-Process -Id $fake.Id -Force -ErrorAction SilentlyContinue }
}

Restart-IsperClean -Exe $exe
Check (-not (Test-Cdp)) "relancado sem porta CDP"
Finish-E2E 'spare'
