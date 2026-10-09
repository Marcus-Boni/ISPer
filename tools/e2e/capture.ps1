# E2E da captura de tarefas (Fase 10.1) pela janela real, via CDP. A fala
# entra como texto (capture_text, o mesmo caminho da voz depois do Whisper):
# no perfil de teste nao ha IA configurada, entao vale o recuo local, em que a
# fala inteira vira um cartao com as datas que o parser entende. Confere a
# janela dos cartoes, a edicao, o Enter que salva na tela Hoje, o Esc que
# descarta e o atalho de anotar nas Configuracoes. Nao toca audio.
#
#   .\tools\e2e\capture.ps1 -Exe <caminho do exe>
#
# ASCII puro de proposito: o PowerShell 5.1 le .ps1 sem BOM como ANSI.
param([string]$Exe)
. "$PSScriptRoot\common.ps1"

$exe = Get-IsperExe $Exe
"capture: $exe"

$tomorrow = (Get-Date).AddDays(1).ToString('yyyy-MM-dd')
$state = 'JSON.stringify({ stage: document.body.dataset.stage, cards: [...document.querySelectorAll("#cards .card-task")].map(li => ({ title: li.querySelector(".title").value, day: li.querySelector("input[type=date]").value, time: li.querySelector("input[type=time]").value })), msg: document.getElementById("msg").hidden ? null : document.getElementById("msg").textContent, save: document.getElementById("save-label").textContent })'

function Wait-Capture {
  param([Parameter(Mandatory)][scriptblock]$Ok, [int]$Seconds = 10)
  $sw = [Diagnostics.Stopwatch]::StartNew()
  do {
    $s = EvJson 'capture.html' $state
    if ($s -and (& $Ok $s)) { return $s }
    Start-Sleep -Milliseconds 250
  } while ($sw.Elapsed.TotalSeconds -lt $Seconds)
  $s
}

Stop-Isper
Check (Start-Isper -Exe $exe) "app abriu com a tela Inicio (CDP)"
Invoke-Isper 'set_ui_lang' "{ lang: 'pt-BR' }" | Out-Null

$st = Invoke-Isper 'shell_status'
Check ($st.capture_shortcut -like 'Ctrl+Alt+*') "atalho de anotar registrado ($($st.capture_shortcut))"

# 1) A fala vira um cartao, com dia e hora.
Invoke-Isper 'capture_text' "{ text: 'amanha as 15h ligar pro contador sobre o IR' }" | Out-Null
Check (Wait-IsperWindow 'capture.html') "a janela dos cartoes abre"
$s1 = Wait-Capture { param($s) $s.stage -eq 'review' -and @($s.cards).Count -eq 1 }
Check ($s1.stage -eq 'review' -and @($s1.cards).Count -eq 1) "a fala vira um cartao para revisar ($(@($s1.cards).Count))"
Check (@($s1.cards)[0].title -eq 'Ligar pro contador sobre o IR') "o titulo sai sem a data ('$(@($s1.cards)[0].title)')"
Check ((@($s1.cards)[0].day -eq $tomorrow) -and (@($s1.cards)[0].time -eq '15:00')) "dia e hora entendidos ($(@($s1.cards)[0].day) $(@($s1.cards)[0].time))"
Check ($s1.msg -like '*IA*') "o aviso diz que a IA ficou de fora ('$($s1.msg)')"
Check ($s1.save -like 'Salvar 1*') "o botao diz quantas salva ('$($s1.save)')"

# 2) Editar o titulo e salvar com Enter.
EvJson 'capture.html' 'JSON.stringify((() => { const i = document.querySelector("#cards .title"); i.value = "Ligar pro contador sobre o IR 2026"; i.dispatchEvent(new Event("input", { bubbles: true })); i.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true })); return true; })())' | Out-Null
$saved = $null
for ($i = 0; $i -lt 20; $i++) {
  $day = Invoke-Isper 'today_load'
  $saved = @(@($day.later) + @($day.planned)) | Where-Object { $_.title -eq 'Ligar pro contador sobre o IR 2026' } | Select-Object -First 1
  if ($saved) { break }
  Start-Sleep -Milliseconds 250
}
Check ($null -ne $saved) "Enter salva a tarefa editada"
Check ($saved.source_kind -eq 'voice' -and $saved.planned_time -eq '15:00') "a tarefa sabe que veio da voz ($($saved.source_kind), $($saved.planned_time))"
$v = Invoke-Isper 'capture_state'
Check ($v.stage -eq 'idle' -and @($v.cards).Count -eq 0) "depois de salvar, a captura zera ($($v.stage))"

# 3) Esc descarta sem salvar.
Invoke-Isper 'capture_text' "{ text: 'revisar o deploy de sexta' }" | Out-Null
$s3 = Wait-Capture { param($s) $s.stage -eq 'review' }
Check ($s3.stage -eq 'review') "segunda captura abre de novo"
EvJson 'capture.html' 'JSON.stringify((() => { document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })); return true; })())' | Out-Null
Start-Sleep -Milliseconds 800
$v3 = Invoke-Isper 'capture_state'
$day3 = Invoke-Isper 'today_load'
$found = @(@($day3.later) + @($day3.planned)) | Where-Object { $_.title -like 'Revisar o deploy*' }
Check ($v3.stage -eq 'idle' -and -not $found) "Esc descarta e nada e salvo"

# 4) O atalho aparece nas Configuracoes e na tela Hoje.
Invoke-Isper 'open_settings_window' | Out-Null
Wait-IsperWindow 'settings.html' | Out-Null
$set = EvJson 'settings.html' 'JSON.stringify({ select: !!document.getElementById("captureshortcut"), active: document.getElementById("aactive").textContent })'
Check ($set.select -and $set.active -like 'Ctrl+Alt+*') "Configuracoes mostram o atalho de anotar ($($set.active))"
Invoke-Isper 'open_today_window' | Out-Null
Wait-IsperWindow 'today.html' | Out-Null
Start-Sleep -Milliseconds 800
$hint = EvJson 'today.html' 'JSON.stringify({ hidden: document.getElementById("voice-hint").hidden, text: document.getElementById("voice-hint").textContent })'
Check ((-not $hint.hidden) -and $hint.text -like '*Ctrl+Alt+*') "a tela Hoje lembra do atalho de voz ('$($hint.text)')"

$errs = Get-JsErrors 'capture.html'
Check (@($errs).Count -eq 0) "janela de captura sem erros de JS$(Format-JsErrors $errs)"

Restart-IsperClean -Exe $exe
Check (-not (Test-Cdp)) "relancado sem porta CDP"
Finish-E2E 'capture'
