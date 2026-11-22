# E2E do "Otimizar" (Fase 11.3) pela janela real, via CDP, com a IA de
# mentira compativel com OpenAI (fake-llm.mjs): sem IA, o botao diz onde
# configurar; com ela, a reuniao ganha a versao limpa (hesitacoes fora, a fala
# que era so hesitacao some), o alternador Otimizada/Original troca a tela, a
# original continua no banco intacta, e exportar leva a versao da tela num
# arquivo -otimizada. O ingles tem as chaves.
#
#   .\tools\e2e\optimize.ps1 -Exe <caminho do exe>
#
# Nada sai do PC: a IA e um processo node local, encerrado no fim.
#
# ASCII puro de proposito: o PowerShell 5.1 le .ps1 sem BOM como ANSI.
param([string]$Exe)
. "$PSScriptRoot\common.ps1"

$exe = Get-IsperExe $Exe
"optimize: $exe"

$l = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
$l.Start(); $llmPort = $l.LocalEndpoint.Port; $l.Stop()
$llmBase = "http://127.0.0.1:$llmPort/v1"
$fakeLlm = Start-Process node -ArgumentList @("`"$PSScriptRoot\fake-llm.mjs`"", "$llmPort") -PassThru -WindowStyle Hidden
$up = $false
for ($i = 0; $i -lt 40 -and -not $up; $i++) {
  try { Invoke-RestMethod "http://127.0.0.1:$llmPort/requests" -TimeoutSec 5 | Out-Null; $up = $true } catch { Start-Sleep -Milliseconds 250 }
}
Check $up "a IA de mentira responde ($llmBase)"

$lastToast = '(q => (q.pop() || {}).textContent || null)([...document.querySelectorAll(".toast")])'
$libJs = 'JSON.stringify({ btn: (b => b ? { text: b.textContent, disabled: b.disabled } : null)(document.getElementById("optimize")), view: (b => b ? b.dataset.v : null)(document.querySelector(".seg-toggle button[aria-checked=true]")), lines: [...document.querySelectorAll("#detail .seg")].map(x => x.textContent), toast: ' + $lastToast + ' })'
function Wait-Lib {
  param([scriptblock]$Ok, [int]$Seconds = 15)
  $sw = [Diagnostics.Stopwatch]::StartNew()
  do {
    $s = EvJson 'library.html' $libJs
    if ($s -and (& $Ok $s)) { return $s }
    Start-Sleep -Milliseconds 300
  } while ($sw.Elapsed.TotalSeconds -lt $Seconds)
  $s
}
function Click-Lib([string]$Js) { EvJson 'library.html' ('JSON.stringify((() => { const b = ' + $Js + '; if (!b) return false; b.click(); return true; })())') }

$llmToml = $null
try {
  Stop-Isper
  Check (Start-Isper -Exe $exe) "app abriu com a tela Inicio (CDP)"
  Invoke-Isper 'set_ui_lang' "{ lang: 'pt-BR' }" | Out-Null
  $llmToml = Get-E2EDataPath 'llm.toml'
  Remove-Item $llmToml -ErrorAction SilentlyContinue

  # Uma reuniao com quatro falas, direto no banco do perfil de teste. Sem
  # aspas duplas no codigo: o PowerShell 5.1 as perde nos argumentos de exe.
  $db = Get-E2EDataPath 'isper.db'
  $py = "import sqlite3,sys; c=sqlite3.connect(sys.argv[1]); c.execute('INSERT INTO meetings (id, title, started_at, duration_secs, started_ts) VALUES (9101, ?, ?, 600, 1763643600000)', ('Alinhamento do portal', '20/11/2026 10:00')); c.executemany('INSERT INTO segments (meeting_id, speaker, start_secs, end_secs, text) VALUES (9101, ?, ?, ?, ?)', [('Eu', 0, 5, 'hum, vamos fechar o escopo na sexta'), ('Participante 1', 6, 8, 'hum'), ('Eu', 30, 35, 'tipo, eu mando a proposta amanha'), ('Participante 1', 70, 75, 'fechado entao')]); c.commit(); print(c.total_changes)"
  $n = & python -c $py $db
  Check ("$n".Trim() -eq '5') "a reuniao de teste esta no banco ($n)"

  Invoke-Isper 'open_library_window' '{ meeting: null }' | Out-Null
  Wait-IsperWindow 'library.html' | Out-Null
  Click-Lib '[...document.querySelectorAll("#list .item")].find(x => x.querySelector(".t").textContent === "Alinhamento do portal")' | Out-Null
  $s0 = Wait-Lib { param($s) $s.btn }
  Check ($s0.btn.text -eq 'Otimizar' -and -not $s0.view -and @($s0.lines).Count -ge 3) "a reuniao abre com 'Otimizar' e sem alternador"

  # 1) Sem IA: o aviso diz onde configurar, e nada muda.
  Click-Lib 'document.getElementById("optimize")' | Out-Null
  $s1 = Wait-Lib { param($s) $s.toast -like '*Configura*' }
  Check ($s1.toast -like '*Intelig*' -and -not $s1.view) "sem IA, o aviso manda para Configuracoes -> Inteligencia ('$($s1.toast)')"

  # 2) Com a IA de mentira: otimiza, mostra a versao limpa.
  Set-Content -Path $llmToml -Encoding ASCII -Value @("provider = `"openai`"", "model = `"fake-agente`"", "base_url = `"$llmBase`"")
  Click-Lib 'document.getElementById("optimize")' | Out-Null
  $s2 = Wait-Lib { param($s) $s.view -eq 'clean' }
  $text = @($s2.lines) -join ' | '
  Check ($s2.view -eq 'clean' -and $s2.toast -like '*otimizada*') "otimizada, o alternador fica em Otimizada ('$($s2.toast)')"
  Check ($text -like '*Vamos fechar o escopo na sexta*' -and $text -like '*Eu mando a proposta*' -and $text -notlike '*hum*' -and $text -notlike '*tipo,*') "as falas aparecem limpas ($text)"
  Check ($s2.btn.text -eq 'Otimizar de novo' -and -not $s2.btn.disabled) "o botao vira 'Otimizar de novo'"
  $plain = @(Invoke-RestMethod "http://127.0.0.1:$llmPort/plain" -TimeoutSec 5 | ForEach-Object { $_ })
  $sent = ($plain | Where-Object { $_.text -like '*Falas:*' } | Select-Object -Last 1).text
  Check ($sent -like '*: hum, vamos fechar*' -and $sent -like '*Participante 1: fechado*') "so o texto das falas foi para a IA, com falante"

  # 3) Original: a tela volta, e o banco nunca perdeu a original.
  Click-Lib '[...document.querySelectorAll(".seg-toggle button")].find(b => b.dataset.v === "orig")' | Out-Null
  $s3 = Wait-Lib { param($s) $s.view -eq 'orig' }
  Check ((@($s3.lines) -join ' ') -like '*hum, vamos fechar*') "Original mostra a transcricao como foi"
  $py2 = "import sqlite3,sys; c=sqlite3.connect(sys.argv[1]); print(c.execute('SELECT count(*) FROM segments WHERE meeting_id = 9101 AND text LIKE ?', ('hum%',)).fetchone()[0], c.execute('SELECT count(*) FROM segments WHERE meeting_id = 9101 AND clean_text = ?', ('',)).fetchone()[0])"
  $counts = ("" + (& python -c $py2 $db)).Trim()
  Check ($counts -eq '2 1') "a original fica no banco e a fala so de hesitacao tem versao vazia ($counts)"

  # 4) Exportar segue o alternador.
  Click-Lib '[...document.querySelectorAll(".seg-toggle button")].find(b => b.dataset.v === "clean")' | Out-Null
  Wait-Lib { param($s) $s.view -eq 'clean' } | Out-Null
  $path = Invoke-Isper 'export_meeting' "{ id: 9101, format: 'srt', clean: true }" 'library.html'
  Check ("$path" -like '*-otimizada.srt' -and (Test-Path "$path")) "exportar a otimizada grava um -otimizada.srt ($path)"
  if (Test-Path "$path") {
    $srt = Get-Content "$path" -Raw -Encoding UTF8
    Check ($srt -like '*Vamos fechar o escopo*' -and $srt -notlike '*hum*') "o SRT leva as falas limpas"
  }

  # 4b) Pronta para agente: o texto do "Copiar para agente" e o arquivo de
  # nome padronizado, na versao da tela.
  $hasBtn = EvJson 'library.html' 'JSON.stringify(!!document.getElementById("copy-agent"))'
  Check $hasBtn "a reuniao tem 'Copiar para agente'"
  $md = Invoke-Isper 'meeting_agent_markdown' "{ id: 9101, clean: true }" 'library.html'
  Check ("$md".StartsWith("---`ntipo: reuniao`n") -and "$md" -like '*data: "2026-11-20"*' -and "$md" -like '*transcricao: otimizada*' -and "$md" -like '*Participante 1*' -and "$md" -like '*Vamos fechar o escopo*' -and "$md" -notlike '*hum,*') "o Markdown para agente tem o cabecalho e as falas limpas"
  $orig = Invoke-Isper 'meeting_agent_markdown' "{ id: 9101, clean: false }" 'library.html'
  Check ("$orig" -like '*transcricao: original*' -and "$orig" -like '*hum, vamos fechar*') "na Original, vai a transcricao como foi"
  $apath = Invoke-Isper 'export_meeting' "{ id: 9101, format: 'agente', clean: true }" 'library.html'
  Check ("$apath" -like '*2026-11-20 10h00 - Alinhamento do portal.md' -and (Test-Path -LiteralPath "$apath")) "exportar para agente grava com o nome padronizado ($apath)"

  # 5) Em ingles, as chaves existem.
  Invoke-Isper 'set_ui_lang' "{ lang: 'en' }" | Out-Null
  Start-Sleep -Milliseconds 800
  $en = Wait-Lib { param($s) $s.btn -and $s.btn.text -eq 'Optimize again' } 6
  Check ($en.btn.text -eq 'Optimize again') "em ingles: '$($en.btn.text)'"
  Invoke-Isper 'set_ui_lang' "{ lang: 'pt-BR' }" | Out-Null

  $errs = @(Get-JsErrors 'library.html')
  Check (@($errs | Where-Object { $_ }).Count -eq 0) "Biblioteca sem erros de JS$(Format-JsErrors $errs)"
} finally {
  if ($llmToml) { Remove-Item $llmToml -ErrorAction SilentlyContinue }
  if ($fakeLlm -and -not $fakeLlm.HasExited) { Stop-Process -Id $fakeLlm.Id -Force -ErrorAction SilentlyContinue }
}

Restart-IsperClean -Exe $exe
Check (-not (Test-Cdp)) "relancado sem porta CDP"
Finish-E2E 'optimize'
