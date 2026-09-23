# E2E da versao portatil (fase 7.6) no app real, via CDP: uma pasta montada
# como o zip da release (o exe, as DLLs que o instalador leva e o marcador
# portable.txt) abre, o Diagnostico diz "versao portatil" e o atualizador
# RECUSA instalar por cima (instalaria uma segunda copia em Programs). Sem o
# marcador, a mesma pasta volta a ser uma copia comum. Nao precisa de audio,
# GPU nem rede.
#
#   .\tools\e2e\portable.ps1 -Exe <caminho do exe>
#
# A pasta de teste fica em %TEMP% e e apagada no fim.
# ASCII puro de proposito: o PowerShell 5.1 le .ps1 sem BOM como ANSI.
param([string]$Exe)
. "$PSScriptRoot\common.ps1"

$exe = Get-IsperExe $Exe
"portable: $exe"
$src = Split-Path $exe
$marker = Join-Path $PSScriptRoot '..\..\packaging\portable\portable.txt'
$dir = Join-Path $env:TEMP 'isper-portable-e2e'

Stop-Isper
if (Test-Path $dir) { Remove-Item -Recurse -Force $dir }
New-Item -ItemType Directory -Force $dir | Out-Null
Copy-Item (Join-Path $src 'isper-app.exe') $dir
$dlls = @(Get-ChildItem $src -Filter *.dll)
$dlls | Copy-Item -Destination $dir
Copy-Item $marker $dir
# As cinco do sherpa-onnx sao obrigatorias (sem elas o exe nem abre); as do
# runtime do VC++ podem estar no sistema, como no runner do CI.
$sherpa = @($dlls | Where-Object { $_.Name -like 'sherpa-onnx*' -or $_.Name -like 'onnxruntime*' -or $_.Name -eq 'cargs.dll' })
Check ($sherpa.Count -ge 5) "pasta portatil montada ($($dlls.Count) DLLs + exe + portable.txt)"
$pexe = Join-Path $dir 'isper-app.exe'

Check (Start-Isper -Exe $pexe) "a copia portatil abre (CDP)"
$d = Invoke-Isper 'diagnostics'
Check ($d.portable -eq $true -and $d.exe_path -like "$dir*") "o Diagnostico reconhece a copia portatil ($($d.exe_path))"
$r = Invoke-Isper 'install_update'
Check ($null -ne $r.__error -and $r.__error -match 'port') "o atualizador nao instala por cima ('$($r.__error)')"

Invoke-Isper 'open_settings_window' | Out-Null
Wait-IsperWindow 'settings.html' | Out-Null
Start-Sleep -Seconds 1
$row = EvJson 'settings.html' '(async () => { await new Promise(r => setTimeout(r, 800)); const label = I18N.t("settings.diag.install"); const dt = [...document.querySelectorAll("#diag dt")].find(e => e.textContent === label); return JSON.stringify({ found: !!dt, value: dt && dt.nextElementSibling ? dt.nextElementSibling.textContent : null, expected: I18N.t("settings.diag.install-portable") }); })()'
Check ($row.found -and $row.value -eq $row.expected) "Configuracoes -> Diagnostico mostra a linha da instalacao ('$($row.value)')"
$errs = Get-JsErrors 'settings.html'
Check (@($errs).Count -eq 0) "settings.html sem erros de JS$(Format-JsErrors $errs)"

# ---- sem o marcador, a mesma pasta e uma copia comum
Stop-Isper
Remove-Item (Join-Path $dir 'portable.txt')
Check (Start-Isper -Exe $pexe) "a mesma pasta sem portable.txt abre"
$d = Invoke-Isper 'diagnostics'
Check ($d.portable -eq $false) "sem o marcador, nao e portatil"

Stop-Isper
Remove-Item -Recurse -Force $dir -ErrorAction SilentlyContinue
Check (-not (Test-Path $dir)) "pasta de teste apagada"
Restart-IsperClean -Exe $exe
Check (-not (Test-Cdp)) "relancado sem porta CDP"
Finish-E2E 'portable'
