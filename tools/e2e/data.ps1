# E2E da fase 7.4 (dados com responsabilidade) no app real, via CDP:
# retencao configuravel, backup do banco, pacote de diagnostico e metricas
# no Diagnostico. Nao precisa de audio nem GPU - roda no runner do CI.
#
#   .\tools\e2e\data.ps1                       # usa target\release\isper-app.exe
#   .\tools\e2e\data.ps1 -Exe <caminho do exe>
#
# O que ele muda na maquina: grava um backup em Documentos\ISPer\Backups e um
# zip em Documentos\ISPer, e apaga os dois no fim. A retencao e testada com o
# prazo mais longo (1 ano) e volta ao valor anterior - nada do usuario e
# apagado por este roteiro.
#
# Este arquivo e ASCII puro de proposito: o PowerShell 5.1 le um .ps1 sem BOM
# como ANSI, e um acento no script viraria comparacao errada.
param([string]$Exe)
. "$PSScriptRoot\common.ps1"

$exe = Get-IsperExe $Exe
"data: $exe"

Stop-Isper
Check (Start-Isper -Exe $exe) "app abriu com a tela Inicio (CDP)"

Invoke-Isper 'open_settings_window' | Out-Null
Wait-IsperWindow 'settings.html' | Out-Null
Start-Sleep -Seconds 2

# ---- diagnostico: schema do banco, retencao e metricas presentes
$d = Invoke-Isper 'diagnostics' 'undefined' 'settings.html'
Check ($null -ne $d -and $d.db_schema -ge 2) "diagnostics.db_schema = $($d.db_schema) (banco migrado por user_version)"
Check ($d.db_bytes -gt 0) "diagnostics.db_bytes = $($d.db_bytes)"
Check ($d.PSObject.Properties.Name -contains 'metrics') "diagnostics traz metrics ($(@($d.metrics).Count) tipos de evento)"
Check ($d.PSObject.Properties.Name -contains 'retention_days') "diagnostics traz retention_days = $($d.retention_days)"
$diagUi = EvJson 'settings.html' 'JSON.stringify({ dts: Array.from(document.querySelectorAll("#diag dt")).map(e => e.textContent) })'
# Compara pelo prefixo: o rotulo na tela tem acento e este script nao pode ter.
Check ($null -ne $diagUi -and @($diagUi.dts | Where-Object { $_ -like 'Reten*' }).Count -eq 1) "Diagnostico na tela mostra a linha Retencao"

# ---- retencao: o campo existe, salva e volta
$s = Invoke-Isper 'get_settings' 'undefined' 'settings.html'
$before = [int]$s.retention_days
$sel = EvJson 'settings.html' 'JSON.stringify({ options: Array.from(document.querySelectorAll("#retention option")).map(o => o.value), value: document.getElementById("retention").value })'
Check ($null -ne $sel -and ($sel.options -join ',') -eq '0,30,90,180,365') "select de retencao com as opcoes 0/30/90/180/365"
Check ([string]$sel.value -eq [string]$before) "select de retencao reflete a config ($before dias)"
# Escolher um prazo na tela pede confirmacao; cancelar volta ao valor salvo.
$ui = EvJson 'settings.html' 'JSON.stringify((() => { const s = document.getElementById("retention"); s.value = "30"; s.dispatchEvent(new Event("change")); return { shown: !document.getElementById("retconfirm").hidden, text: document.getElementById("retconfirmtext").textContent }; })())'
Check ($null -ne $ui -and $ui.shown -and $ui.text -like '*30 dias*') "escolher 30 dias na tela mostra o aviso de confirmacao"
$ui2 = EvJson 'settings.html' 'JSON.stringify((() => { document.getElementById("retno").click(); return { shown: !document.getElementById("retconfirm").hidden, value: document.getElementById("retention").value }; })())'
Check ($null -ne $ui2 -and -not $ui2.shown -and [string]$ui2.value -eq [string]$before) "Cancelar esconde o aviso e volta o select para $before"
$patch = @{
  shortcut = $s.shortcut; lang = $s.lang; dictionary = $s.dictionary; model = $s.model
  meeting_source = $s.meeting_source; llm_provider = $s.llm_provider; llm_model = $s.llm_model
  autostart = [bool]$s.autostart; show_home_on_launch = [bool]$s.show_home_on_launch
  input_device = $s.input_device; meeting_shortcut = $s.meeting_shortcut; mark_shortcut = $s.mark_shortcut
  polish = [bool]$s.polish; polish_style = $s.polish_style; after_meeting = $s.after_meeting
  voice_commands = [bool]$s.voice_commands; auto_update_check = [bool]$s.auto_update_check
  call_detect = $s.call_detect; live_insights = [bool]$s.live_insights; insights_interval_min = $s.insights_interval_min
  emb_provider = $s.emb_provider; emb_model = $s.emb_model; emb_base_url = $s.emb_base_url
  retention_days = 365
}
$json = ($patch | ConvertTo-Json -Compress -Depth 4)
$r = Invoke-Isper 'apply_settings' "{ patch: $json }" 'settings.html'
Check ($null -ne $r -and -not $r.__error) "apply_settings com retention_days = 365 aceito"
$s2 = Invoke-Isper 'get_settings' 'undefined' 'settings.html'
Check ([int]$s2.retention_days -eq 365) "retention_days persistido = $($s2.retention_days)"
$d2 = Invoke-Isper 'diagnostics' 'undefined' 'settings.html'
Check ([int]$d2.retention_days -eq 365) "diagnostics acompanha a retencao (365)"
$patch.retention_days = $before
$json = ($patch | ConvertTo-Json -Compress -Depth 4)
$r = Invoke-Isper 'apply_settings' "{ patch: $json }" 'settings.html'
Check ($null -ne $r -and -not $r.__error -and ([int](Invoke-Isper 'get_settings' 'undefined' 'settings.html').retention_days -eq $before)) "retencao de volta a $before dias"

# ---- backup do banco
$bk = Invoke-Isper 'backup_database' 'undefined' 'settings.html'
Check ($null -ne $bk -and -not $bk.__error -and (Test-Path ([string]$bk))) "backup gravado: $bk"
if ($bk -and -not $bk.__error -and (Test-Path ([string]$bk))) {
  $bytes = [IO.File]::ReadAllBytes([string]$bk)
  $magic = [Text.Encoding]::ASCII.GetString($bytes, 0, 15)
  Check ($magic -eq 'SQLite format 3') "backup e um arquivo SQLite ($([math]::Round($bytes.Length / 1KB)) KB)"
  # user_version fica no offset 60 do cabecalho (big-endian, 4 bytes).
  $uv = ($bytes[60] -shl 24) -bor ($bytes[61] -shl 16) -bor ($bytes[62] -shl 8) -bor $bytes[63]
  Check ($uv -eq $d.db_schema) "backup carrega o mesmo user_version ($uv)"
  Remove-Item ([string]$bk) -Force -ErrorAction SilentlyContinue
}

# ---- pacote de diagnostico
$zipPath = Invoke-Isper 'export_diagnostics' 'undefined' 'settings.html'
Check ($null -ne $zipPath -and -not $zipPath.__error -and (Test-Path ([string]$zipPath))) "diagnostico exportado: $zipPath"
if ($zipPath -and -not $zipPath.__error -and (Test-Path ([string]$zipPath))) {
  Add-Type -AssemblyName System.IO.Compression.FileSystem
  $zip = [IO.Compression.ZipFile]::OpenRead([string]$zipPath)
  try {
    $names = @($zip.Entries | ForEach-Object { $_.FullName })
    Check ($names -contains 'diagnostico.json') "zip traz diagnostico.json"
    Check ($names -contains 'versoes.txt') "zip traz versoes.txt"
    Check (@($names | Where-Object { $_ -like 'logs/isper.log*' }).Count -ge 1) "zip traz pelo menos um log ($(@($names | Where-Object { $_ -like 'logs/*' }).Count))"
    $logEntry = $zip.Entries | Where-Object { $_.FullName -like 'logs/*' } | Select-Object -First 1
    if ($logEntry) {
      $reader = New-Object IO.StreamReader($logEntry.Open())
      try { $logText = $reader.ReadToEnd() } finally { $reader.Dispose() }
      Check (-not ($logText -match 'transcrito:')) "log do zip sem linhas de texto ditado"
      $firstLine = ($logText -split "`n" | Where-Object { $_.Trim() } | Select-Object -First 1)
      Check ($firstLine.TrimStart().StartsWith('{') -or $firstLine.StartsWith('[linha')) "log em JSON Lines (primeira linha comeca com '{')"
    }
    $ver = $zip.Entries | Where-Object { $_.FullName -eq 'versoes.txt' }
    if ($ver) {
      $reader = New-Object IO.StreamReader($ver.Open())
      try { $verText = $reader.ReadToEnd() } finally { $reader.Dispose() }
      Check ($verText -match 'ISPer \d+\.\d+\.\d+' -and $verText -match 'WebView2') "versoes.txt traz versao do ISPer e do WebView2"
    }
  } finally { $zip.Dispose() }
  Remove-Item ([string]$zipPath) -Force -ErrorAction SilentlyContinue
}

# ---- log do dia em JSON Lines
$log = Get-TodayLog
if (Test-Path $log) {
  $line = Get-Content $log -Tail 1
  $parsed = $null
  try { $parsed = $line | ConvertFrom-Json -ErrorAction Stop } catch {}
  Check ($null -ne $parsed -and $parsed.level -and $parsed.message) "log do dia em JSON Lines (level=$($parsed.level))"
} else {
  Check $false "log do dia existe em $log"
}

$errs = Get-JsErrors 'settings.html'
Check (@($errs).Count -eq 0) "Configuracoes sem erros de JS$(Format-JsErrors $errs)"

Restart-IsperClean -Exe $exe
Check (-not (Test-Cdp)) "relancado sem porta CDP"
Finish-E2E 'data'
