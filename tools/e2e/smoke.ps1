<#
.SYNOPSIS
  Smoke test do ISPer real: Início, Biblioteca, Configurações e indicador sem erros de JS.
.DESCRIPTION
  Fecha qualquer ISPer aberto, sobe o exe com a porta CDP, confere estado e janelas,
  troca o modo do indicador e volta ao que estava, consulta o atualizador e relança
  o app limpo. Leva ~40 s. Não grava nada.
.EXAMPLE
  .\tools\e2e\smoke.ps1                                   # exe instalado (ou target\release)
  .\tools\e2e\smoke.ps1 -Exe .\target\release\isper-app.exe
#>
param([string]$Exe)
. "$PSScriptRoot\common.ps1"
$exe = Get-IsperExe $Exe
"smoke: $exe"

Stop-Isper
Check (Start-Isper -Exe $exe) "app abriu com a tela Inicio (CDP)"

$st = Invoke-Isper 'home_status'
Check ($st.version -match '^\d+\.\d+\.\d+$') "home_status.version = $($st.version)"
Check ($st.engine.kind -in 'ready', 'loading', 'missing') "motor: $($st.engine.kind)"
Check (-not $st.meeting_active) "sem reuniao ativa"
$errs = Get-JsErrors 'home.html'
Check (@($errs).Count -eq 0) "Inicio sem erros de JS$(Format-JsErrors $errs)"

Invoke-Isper 'open_library_window' '{ meeting: null }' | Out-Null
Start-Sleep -Seconds 4
$lib = EvJson 'library.html' 'JSON.stringify({ items: document.querySelectorAll(".item").length, errors: window.__isperErrors || [] })'
Check ($null -ne $lib -and @($lib.errors).Count -eq 0) "Biblioteca abriu sem erros de JS ($($lib.items) reunioes listadas)$(Format-JsErrors $lib.errors)"

Invoke-Isper 'open_settings_window' | Out-Null
Start-Sleep -Seconds 4
$s = Invoke-Isper 'get_settings' 'undefined' 'settings.html'
Check ($s.version -eq $st.version) "Configuracoes: versao $($s.version) (igual ao Inicio)"
$errs = Get-JsErrors 'settings.html'
Check (@($errs).Count -eq 0) "Configuracoes sem erros de JS$(Format-JsErrors $errs)"

# Indicador: mostra, troca para legendas, volta ao modo original.
$original = if ($st.overlay_captions) { 'captions' } elseif ((Invoke-Isper 'overlay_prefs').mini) { 'mini' } else { 'normal' }
Invoke-Isper 'show_indicator_cmd' | Out-Null
Start-Sleep -Seconds 1
$ov = EvJson 'http://tauri.localhost/' 'JSON.stringify({ w: innerWidth, h: innerHeight, cls: document.body.className })'
Check ($ov.w -gt 0) "indicador visivel ($($ov.w)x$($ov.h), modo '$($ov.cls)' -> original '$original')"
Invoke-Isper 'overlay_set_mode' '{ mode: "captions" }' | Out-Null
Start-Sleep -Seconds 1
Check ((EvJson 'http://tauri.localhost/' 'JSON.stringify({ cls: document.body.className })').cls -eq 'captions') "modo legendas (760 px)"
Invoke-Isper 'overlay_set_mode' "{ mode: `"$original`" }" | Out-Null
Start-Sleep -Seconds 1
$back = (EvJson 'http://tauri.localhost/' 'JSON.stringify({ cls: document.body.className })').cls
Check (($original -eq 'normal' -and $back -eq '') -or $back -eq $original) "indicador de volta ao modo original"

# Novidades de 10/09: deteccao de chamada, insights e busca semantica respondem;
# o indicador alterna mostrar/ocultar e o estado original e restaurado ao fim.
Check (($st.PSObject.Properties.Name -contains 'call_detect') -and ($null -ne $st.insights)) "home_status traz call_detect='$($st.call_detect)' e insights (enabled=$($st.insights.enabled), configured=$($st.insights.configured))"
$emb = Invoke-Isper 'embeddings_status'
Check (($null -ne $emb) -and ($emb.PSObject.Properties.Name -contains 'configured')) "busca semantica responde (configured=$($emb.configured), provider='$($emb.provider)')"
$wasPinned = [bool]$st.overlay_pinned
$t1 = Invoke-Isper 'overlay_toggle_pin'
Start-Sleep -Milliseconds 800
Check ((Invoke-Isper 'home_status').overlay_visible -eq $t1) "indicador alternado pelo botao do Inicio (visivel=$t1)"
$t2 = Invoke-Isper 'overlay_toggle_pin'
Start-Sleep -Milliseconds 800
Check (((Invoke-Isper 'home_status').overlay_visible -eq $t2) -and ($t2 -ne $t1)) "indicador alternado de volta (visivel=$t2)"
if ($wasPinned) { Invoke-Isper 'show_indicator_cmd' | Out-Null } else { Invoke-Isper 'overlay_hide' | Out-Null }
Start-Sleep -Milliseconds 500
Check ([bool](Invoke-Isper 'home_status').overlay_pinned -eq $wasPinned) "preferencia do indicador restaurada (fixo=$wasPinned)"
$errs = Get-JsErrors 'home.html'
Check (@($errs).Count -eq 0) "Inicio segue sem erros de JS apos as novidades$(Format-JsErrors $errs)"

$upd = Invoke-Isper 'check_update'
$updText = if ($null -eq $upd) { 'na ultima versao' } elseif ($upd.__error) { "erro amigavel: $($upd.__error)" } else { "versao nova $($upd.version)" }
Check ($null -eq $upd -or $upd.__error -or $upd.version) "atualizador respondeu ($updText)"

Restart-IsperClean $exe
Check (-not (Test-Cdp)) "relancado sem porta CDP"
Finish-E2E 'smoke'
