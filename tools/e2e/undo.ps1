# E2E do "Desfazer" (fase 7.5) pela interface real da Biblioteca, via CDP:
# excluir um ditado some com ele na hora e mostra o toast com Desfazer; o
# botao Desfazer e o Ctrl+Z trazem de volta. Nada e apagado: cada exclusao
# do roteiro e desfeita. Sem ditados no historico (runner do CI), pula.
# A exclusao de reuniao (inclusive a que expira) e coberta pelo meeting.ps1.
#
#   .\tools\e2e\undo.ps1 -Exe <caminho do exe>
#
# ASCII puro de proposito: o PowerShell 5.1 le .ps1 sem BOM como ANSI.
param([string]$Exe)
. "$PSScriptRoot\common.ps1"

$exe = Get-IsperExe $Exe
"undo: $exe"

Stop-Isper
Check (Start-Isper -Exe $exe) "app abriu com a tela Inicio (CDP)"
Invoke-Isper 'open_library_window' '{ meeting: null }' | Out-Null
Wait-IsperWindow 'library.html' | Out-Null
Start-Sleep -Seconds 1
EvJson 'library.html' 'JSON.stringify((() => { document.querySelector(".tab[data-tab=\"dictations\"]").click(); return true; })())' | Out-Null
Start-Sleep -Seconds 2

$before = @(Invoke-Isper 'list_dictations' '{ query: null }' 'library.html')
if ($before.Count -eq 0) {
  "  (sem ditados no historico - roteiro pulado)"
} else {
  $count = 'JSON.stringify({ rows: document.querySelectorAll("#list .dict").length, toast: !!document.querySelector(".toast.has-action .toast-act"), label: (document.querySelector(".toast.has-action .toast-act") || {}).textContent || null })'
  $ui0 = EvJson 'library.html' $count
  Check ($ui0.rows -gt 0) "Biblioteca lista $($ui0.rows) ditados"

  # 1) Excluir pelo botao da linha -> some na hora + toast com Desfazer.
  EvJson 'library.html' 'JSON.stringify((() => { document.querySelector("#list .dict .btn-danger").click(); return true; })())' | Out-Null
  Start-Sleep -Milliseconds 900
  $ui1 = EvJson 'library.html' $count
  Check ($ui1.rows -eq $ui0.rows - 1) "ditado some da lista na hora ($($ui0.rows) -> $($ui1.rows))"
  Check ($ui1.toast -and $ui1.label -eq 'Desfazer') "toast com o botao '$($ui1.label)'"
  Check (@(Invoke-Isper 'list_dictations' '{ query: null }' 'library.html').Count -eq $before.Count - 1) "o app ja esconde o ditado das listas"

  # 2) Desfazer pelo botao.
  EvJson 'library.html' 'JSON.stringify((() => { document.querySelector(".toast.has-action .toast-act").click(); return true; })())' | Out-Null
  Start-Sleep -Milliseconds 900
  $ui2 = EvJson 'library.html' $count
  Check ($ui2.rows -eq $ui0.rows) "Desfazer traz o ditado de volta ($($ui2.rows))"
  Check (@(Invoke-Isper 'list_dictations' '{ query: null }' 'library.html').Count -eq $before.Count) "o app devolve o ditado as listas"

  # 3) Excluir de novo e desfazer com Ctrl+Z.
  EvJson 'library.html' 'JSON.stringify((() => { document.querySelector("#list .dict .btn-danger").click(); return true; })())' | Out-Null
  Start-Sleep -Milliseconds 900
  EvJson 'library.html' 'JSON.stringify((() => { document.body.dispatchEvent(new KeyboardEvent("keydown", { key: "z", ctrlKey: true, bubbles: true })); return true; })())' | Out-Null
  Start-Sleep -Milliseconds 900
  $ui3 = EvJson 'library.html' $count
  Check ($ui3.rows -eq $ui0.rows -and -not $ui3.toast) "Ctrl+Z desfaz e fecha o toast ($($ui3.rows) ditados)"
  Check (@(Invoke-Isper 'list_dictations' '{ query: null }' 'library.html').Count -eq $before.Count) "nenhum ditado perdido pelo roteiro"
}

$errs = Get-JsErrors 'library.html'
Check (@($errs).Count -eq 0) "Biblioteca sem erros de JS$(Format-JsErrors $errs)"
Restart-IsperClean -Exe $exe
Check (-not (Test-Cdp)) "relancado sem porta CDP"
Finish-E2E 'undo'
