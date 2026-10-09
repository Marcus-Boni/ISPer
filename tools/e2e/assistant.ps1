# E2E do assistente (Fase 10.4) pela janela real, via CDP, contra uma IA de
# mentira compativel com OpenAI (fake-llm.mjs) e o OptTime de mentira
# (fake-opttime.mjs), os dois em 127.0.0.1: sem IA configurada o erro leva as
# Configuracoes; com ela, a pergunta volta com a fonte citada e clicavel, criar
# tarefa espera o cartao (Fazer cria, Agora nao e uma pergunta nova nao), a
# ferramenta bloqueada nas permissoes nem chega a IA, e o fechamento do dia le
# as horas no OptTime sem perguntar.
#
#   .\tools\e2e\assistant.ps1 -Exe <caminho do exe>
#
# Nada sai do PC: a IA e o OptTime sao processos node locais, encerrados no fim.
#
# ASCII puro de proposito: o PowerShell 5.1 le .ps1 sem BOM como ANSI.
param([string]$Exe)
. "$PSScriptRoot\common.ps1"

$exe = Get-IsperExe $Exe
"assistant: $exe"

function Get-FreePort {
  $l = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
  $l.Start()
  $p = $l.LocalEndpoint.Port
  $l.Stop()
  $p
}
# O Invoke-RestMethod do PowerShell 5.1 devolve o array JSON como um objeto so:
# passar pela variavel e pelo pipeline desmonta o array em itens.
function Get-List([string]$Url) {
  $list = Invoke-RestMethod $Url -TimeoutSec 5
  @($list | ForEach-Object { $_ })
}

$token = 'opt_tok_e2e_' + [guid]::NewGuid().ToString('N').Substring(0, 8)
$otPort = Get-FreePort
$otUrl = "http://127.0.0.1:$otPort/api/mcp"
$llmPort = Get-FreePort
$llmBase = "http://127.0.0.1:$llmPort/v1"
$fakeOt = Start-Process node -ArgumentList @("`"$PSScriptRoot\fake-opttime.mjs`"", "$otPort", $token) -PassThru -WindowStyle Hidden
$fakeLlm = Start-Process node -ArgumentList @("`"$PSScriptRoot\fake-llm.mjs`"", "$llmPort") -PassThru -WindowStyle Hidden
$up = $false
for ($i = 0; $i -lt 40 -and -not $up; $i++) {
  try { Get-List "http://127.0.0.1:$otPort/calls" | Out-Null; Get-List "http://127.0.0.1:$llmPort/requests" | Out-Null; $up = $true } catch { Start-Sleep -Milliseconds 250 }
}
Check $up "a IA e o OptTime de mentira respondem ($llmBase, $otUrl)"

$assistJs = 'JSON.stringify((() => { const th = document.getElementById("thread"); return { hint: !document.getElementById("assist-hint").hidden, newBtn: !document.getElementById("assist-new").hidden, busy: !!th.querySelector("[aria-busy=true]"), turns: [...th.children].map(li => ({ cls: li.className, text: li.textContent, cites: [...li.querySelectorAll(".cite")].map(b => b.textContent), sources: [...li.querySelectorAll(".sources .chip")].map(c => ({ text: c.textContent, link: c.tagName === "BUTTON" })), buttons: [...li.querySelectorAll(".turn-acts button")].map(b => b.textContent), boxes: li.querySelectorAll("input[type=checkbox]").length })), planned: [...document.querySelectorAll("#list-planned > li .title")].map(x => x.textContent), ping: [...document.querySelectorAll(".tasks > li.ping .title")].map(x => x.textContent) }; })())'
function Get-Assist { EvJson 'today.html' $assistJs }
function Wait-Assist {
  param([scriptblock]$Ok, [int]$Seconds = 20)
  $sw = [Diagnostics.Stopwatch]::StartNew()
  do {
    $s = Get-Assist
    if ($s -and -not $s.busy -and (& $Ok $s)) { return $s }
    Start-Sleep -Milliseconds 300
  } while ($sw.Elapsed.TotalSeconds -lt $Seconds)
  $s
}
function Last-Turn($s) { @($s.turns)[-1] }
function Ask([string]$Question) {
  $q = ConvertTo-Json $Question
  EvJson 'today.html' ('JSON.stringify((() => { const i = document.getElementById("ask-q"); i.value = ' + $q + '; document.getElementById("ask").requestSubmit(); return true; })())') | Out-Null
}
function Click-Today([string]$Js) {
  EvJson 'today.html' ('JSON.stringify((() => { const b = ' + $Js + '; if (!b) return false; b.click(); return true; })())')
}
# O botao do cartao aberto (o ultimo da conversa) pelo comeco do texto.
function Click-Card([string]$Prefix) {
  Click-Today ('[...document.querySelectorAll("#thread > li.confirm:last-child .turn-acts button")].find(b => b.textContent.startsWith("' + $Prefix + '"))')
}
function Count-Like($items, [string]$pattern) { @($items | Where-Object { $_ -like $pattern }).Count }

$day = (Get-Date).ToString('yyyy-MM-dd')
$llmToml = $null
try {
  Stop-Isper
  Check (Start-Isper -Exe $exe) "app abriu com a tela Inicio (CDP)"
  Invoke-Isper 'set_ui_lang' "{ lang: 'pt-BR' }" | Out-Null
  Invoke-Isper 'opttime_clear_token' | Out-Null
  Invoke-Isper 'opttime_set_url' "{ url: null }" | Out-Null
  $llmToml = Get-E2EDataPath 'llm.toml'
  Remove-Item $llmToml -ErrorAction SilentlyContinue
  Invoke-Isper 'task_add' "{ task: { title: 'Revisar o contrato da Marca', planned_on: '$day' } }" | Out-Null
  Invoke-Isper 'open_today_window' | Out-Null
  Wait-IsperWindow 'today.html' | Out-Null

  # 1) Sem IA configurada, o erro diz o que fazer e leva as Configuracoes.
  $s0 = Get-Assist
  Check ($s0.hint -and -not $s0.newBtn -and @($s0.turns).Count -eq 0) "a barra do assistente aparece vazia, com a dica"
  Ask 'O que eu tenho para hoje?'
  $s1 = Wait-Assist { param($s) @($s.turns).Count -ge 2 }
  $t1 = Last-Turn $s1
  Check ($t1.cls -like '*err*' -and $t1.text -like '*Escolha uma IA*') "sem IA, o erro pede para escolher uma ('$($t1.text)')"
  Check ((Count-Like $t1.buttons 'Abrir Configura*') -eq 1) "e oferece abrir as Configuracoes"

  # 2) Com a IA de mentira (compativel com OpenAI): a fonte vem citada.
  Set-Content -Path $llmToml -Encoding ASCII -Value @("provider = `"openai`"", "model = `"fake-agente`"", "base_url = `"$llmBase`"")
  Click-Today 'document.getElementById("assist-new")' | Out-Null
  Ask 'O que eu tenho para hoje?'
  $s2 = Wait-Assist { param($s) @($s.turns).Count -ge 2 -and (Last-Turn $s).cls -notlike '*wait*' }
  $t2 = Last-Turn $s2
  Check (@($s2.turns).Count -eq 2 -and $t2.text -like '*Revisar o contrato da Marca*') "a resposta traz a tarefa do dia ('$($t2.text)')"
  Check ((@($t2.cites) -contains '1') -and @($t2.sources).Count -eq 1) "a citacao vira o numero 1 e a fonte aparece embaixo"
  $src = @($t2.sources)[0]
  Check ($src.text -like '1Revisar o contrato*' -and $src.link) "a fonte e clicavel ('$($src.text)')"
  Check ((-not $s2.hint) -and $s2.newBtn) "com conversa, a dica sai e aparece Nova conversa"
  Click-Today 'document.querySelector("#thread .sources .chip")' | Out-Null
  $sPing = Wait-Assist { param($s) @($s.ping).Count -ge 1 } -Seconds 4
  Check ((Count-Like $sPing.ping 'Revisar o contrato*') -eq 1) "clicar na fonte destaca a tarefa na lista"

  # 3) Criar tarefa espera o toque; Fazer cria.
  Ask 'cria: ligar pro Joao hoje as 15h'
  $s3 = Wait-Assist { param($s) (Last-Turn $s).cls -like '*confirm*' }
  $t3 = Last-Turn $s3
  Check ($t3.boxes -eq 1 -and $t3.text -like '*Ligar pro Jo*' -and $t3.text -like '*15:00*') "o cartao descreve a tarefa a criar ('$($t3.text)')"
  Check ((Count-Like $t3.buttons 'Fazer') -eq 1 -and (Count-Like $t3.buttons 'Agora n*') -eq 1) "com Fazer e Agora nao"
  Check ((Count-Like $s3.planned 'Ligar pro Jo*') -eq 0) "nada criado antes do toque"
  Click-Card 'Fazer' | Out-Null
  $s4 = Wait-Assist { param($s) (Last-Turn $s).text -like '*Criei a tarefa*' -and (Count-Like $s.planned 'Ligar pro Jo*') -ge 1 }
  Check ((Last-Turn $s4).text -like '*Criei a tarefa*') "depois do toque, a IA diz que criou"
  Check ((Count-Like $s4.planned 'Ligar pro Jo*') -eq 1) "e a tarefa aparece em Para hoje"
  $card = @($s4.turns) | Where-Object { $_.cls -like '*confirm*' } | Select-Object -Last 1
  Check ($card.cls -like '*decided*' -and $card.text -like '*Confirmado (1)*' -and @($card.buttons).Count -eq 0) "o cartao fica decidido, sem botoes"

  # 4) Agora nao: nada criado.
  Ask 'cria: ligar pro Joao hoje as 15h'
  Wait-Assist { param($s) (Last-Turn $s).cls -like '*confirm*' -and (Last-Turn $s).cls -notlike '*decided*' } | Out-Null
  Click-Card 'Agora n' | Out-Null
  $s5 = Wait-Assist { param($s) (Last-Turn $s).text -like '*Tudo bem*' }
  Check ((Last-Turn $s5).text -like '*Tudo bem*') "recusado, a IA nao faz ('$((Last-Turn $s5).text)')"
  Start-Sleep -Milliseconds 800
  Check ((Count-Like (Get-Assist).planned 'Ligar pro Jo*') -eq 1) "e continua uma tarefa so"

  # 5) Cartao aberto e uma pergunta nova: conta como nao, e nada roda.
  Ask 'cria: ligar pro Joao hoje as 15h'
  Wait-Assist { param($s) (Last-Turn $s).cls -like '*confirm*' -and (Last-Turn $s).cls -notlike '*decided*' } | Out-Null
  Ask 'O que eu tenho para hoje?'
  $s6 = Wait-Assist { param($s) (Last-Turn $s).text -like '*Encontrei*' }
  $skipped = @($s6.turns) | Where-Object { $_.cls -like '*confirm*' } | Select-Object -Last 1
  Check ($skipped.text -like '*Ficou sem resposta*') "o cartao esquecido fica 'sem resposta'"
  Check ((Count-Like $s6.planned 'Ligar pro Jo*') -eq 1) "e a tarefa nao foi criada de novo"
  $reqs = Get-List "http://127.0.0.1:$llmPort/requests"
  $afterSkip = $reqs | Where-Object { $_.question -like 'O que eu tenho*' } | Select-Object -Last 1
  Check ($null -ne $afterSkip) "a pergunta nova chegou a IA"

  # 6) Permissoes: o OptTime conectado aparece; bloquear tira a ferramenta da IA.
  Invoke-Isper 'opttime_set_url' "{ url: '$otUrl' }" | Out-Null
  Invoke-Isper 'opttime_set_token' "{ token: '$token' }" | Out-Null
  Invoke-Isper 'open_settings_window' "{ section: 'assistente' }" | Out-Null
  Wait-IsperWindow 'settings.html' | Out-Null
  $permJs = 'JSON.stringify((() => { const rows = (id) => [...document.querySelectorAll("#" + id + " .perm")].map(r => { const c = r.querySelector("[aria-checked=true]"); return { name: r.querySelector("code").textContent, title: r.querySelector(".perm-name span").textContent, on: c ? c.dataset.p : null, allowDisabled: r.querySelector("button[data-p=allow]").disabled }; }); return { isper: rows("agisper"), opttime: rows("agopttime"), note: document.getElementById("agotnote").hidden ? null : document.getElementById("agotnote").textContent }; })())'
  $p = $null
  for ($i = 0; $i -lt 30; $i++) {
    $p = EvJson 'settings.html' $permJs
    if ($p -and @($p.opttime).Count -ge 4) { break }
    Start-Sleep -Milliseconds 300
  }
  Check (@($p.isper).Count -eq 9) "Configuracoes: as 9 ferramentas do ISPer ($(@($p.isper).Count))"
  $create = @($p.isper) | Where-Object { $_.name -eq 'criar_tarefa' }
  $tasks = @($p.isper) | Where-Object { $_.name -eq 'tarefas_do_dia' }
  Check ($create.on -eq 'ask' -and $tasks.on -eq 'allow' -and $create.title -eq 'Criar tarefa') "criar pergunta, ler e livre, com nome traduzido"
  Check (@($p.opttime).Count -eq 4) "as 4 do OptTime de mentira ($(@($p.opttime).Count); nota: '$($p.note)')"
  $del = @($p.opttime) | Where-Object { $_.name -eq 'opt_time_delete_entry' }
  $log = @($p.opttime) | Where-Object { $_.name -eq 'opt_time_log_time' }
  $sum = @($p.opttime) | Where-Object { $_.name -eq 'opt_time_get_today_summary' }
  Check ($del.allowDisabled -and $del.on -eq 'ask') "a que apaga nao pode ficar livre"
  Check ($log.on -eq 'ask' -and $sum.on -eq 'allow' -and $sum.title -eq 'Resumo do dia') "lancar horas pergunta; o resumo do dia e livre, com o titulo do servidor"
  EvJson 'settings.html' 'JSON.stringify((() => { const r = [...document.querySelectorAll("#agisper .perm")].find(x => x.querySelector("code").textContent === "criar_tarefa"); r.querySelector("button[data-p=never]").click(); return true; })())' | Out-Null
  $blocked = $false
  for ($i = 0; $i -lt 20 -and -not $blocked; $i++) {
    $v = Invoke-Isper 'assistant_tools'
    $blocked = (@($v.tools) | Where-Object { $_.name -eq 'criar_tarefa' }).effective -eq 'never'
    if (-not $blocked) { Start-Sleep -Milliseconds 250 }
  }
  Check $blocked "Bloqueada fica guardada"

  Invoke-Isper 'open_today_window' | Out-Null
  Wait-IsperWindow 'today.html' | Out-Null
  Click-Today 'document.getElementById("assist-new")' | Out-Null
  Ask 'cria: ligar pro Joao hoje as 15h'
  $s7 = Wait-Assist { param($s) @($s.turns).Count -ge 2 -and (Last-Turn $s).cls -notlike '*wait*' }
  Check ((Last-Turn $s7).text -like '*bloqueada*' -and (Last-Turn $s7).cls -notlike '*confirm*') "bloqueada, nem vira cartao ('$((Last-Turn $s7).text)')"
  $lastReq = Get-List "http://127.0.0.1:$llmPort/requests" | Select-Object -Last 1
  Check (-not (@($lastReq.tools) -contains 'criar_tarefa') -and (@($lastReq.tools) -contains 'opt_time_get_today_summary')) "a IA nem recebe a ferramenta bloqueada, e ja ve as do OptTime"
  Invoke-Isper 'assistant_set_permission' "{ name: 'criar_tarefa', permission: null }" | Out-Null

  # 7) Fechar o dia: le as horas no OptTime sem perguntar.
  $before = @(Get-List "http://127.0.0.1:$otPort/calls" | Where-Object { $_.name -eq 'opt_time_get_today_summary' }).Count
  Click-Today 'document.getElementById("assist-closing")' | Out-Null
  $s8 = Wait-Assist { param($s) @($s.turns).Count -eq 2 -and (Last-Turn $s).text -like '*Encontrei*' } -Seconds 25
  Check (@($s8.turns)[0].text -eq 'Fechar o dia' -and @($s8.turns).Count -eq 2) "Fechar o dia comeca uma conversa nova"
  $t8 = Last-Turn $s8
  Check ($t8.text -like '*4h40*') "o fechamento traz as horas do OptTime ('$($t8.text)')"
  $ot = @($t8.sources) | Where-Object { $_.text -like '*OptTime*' } | Select-Object -First 1
  Check ($null -ne $ot -and -not $ot.link) "a fonte do OptTime aparece, sem link ('$($ot.text)')"
  $after = @(Get-List "http://127.0.0.1:$otPort/calls" | Where-Object { $_.name -eq 'opt_time_get_today_summary' }).Count
  Check ($after -gt $before) "o resumo do dia foi lido no OptTime ($before -> $after)"
  $closingReq = Get-List "http://127.0.0.1:$llmPort/requests" | Where-Object { $_.question -like 'Feche o meu dia*' } | Select-Object -Last 1
  Check ($closingReq.system -like '*opt_time_*') "com o OptTime conectado, as instrucoes falam dele"

  # 8) Resumo da manha.
  Click-Today 'document.getElementById("assist-morning")' | Out-Null
  $s9 = Wait-Assist { param($s) @($s.turns).Count -eq 2 -and (Last-Turn $s).text -like '*Encontrei*' } -Seconds 25
  Check (@($s9.turns)[0].text -like 'Resumo da manh*' -and (Last-Turn $s9).text -like '*Revisar o contrato*') "o resumo da manha responde com o dia"
  $morningReq = Get-List "http://127.0.0.1:$llmPort/requests" | Select-Object -Last 1
  Check ($morningReq.question -like 'Monte o resumo da minha manh*') "e manda o pedido pronto a IA"

  $errs = @(Get-JsErrors 'today.html') + @(Get-JsErrors 'settings.html')
  Check (@($errs | Where-Object { $_ }).Count -eq 0) "Hoje e Configuracoes sem erros de JS$(Format-JsErrors $errs)"
} finally {
  Invoke-Isper 'assistant_set_permission' "{ name: 'criar_tarefa', permission: null }" | Out-Null
  Invoke-Isper 'opttime_clear_token' | Out-Null
  Invoke-Isper 'opttime_set_url' "{ url: null }" | Out-Null
  if ($llmToml) { Remove-Item $llmToml -ErrorAction SilentlyContinue }
  foreach ($p in @($fakeOt, $fakeLlm)) {
    if ($p -and -not $p.HasExited) { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue }
  }
}

Restart-IsperClean -Exe $exe
Check (-not (Test-Cdp)) "relancado sem porta CDP"
Finish-E2E 'assistant'
