<#
.SYNOPSIS
  e2e do filtro do Copilot (Jev): a tela, a configuracao salva e o teste de conexao.
.DESCRIPTION
  No perfil de teste (ISPER_PROFILE_DIR), sem tocar nos dados de quem roda:
  - Configuracoes -> Inteligencia tem o bloco do filtro, traduzido e sem erro de JS;
  - o filtro nasce desligado, liga e desliga pelo Salvar e fica salvo;
  - "Testar conexao" (test_typesafe): com -UseStoredKey, a chave guardada em
    typesafe.ISPer vai ao app por ISPER_TYPESAFE_API_KEY (nunca e impressa) e a
    chamada real precisa marcar o compromisso do exemplo (~US$ 0,00003); sem ela,
    o erro tem de dizer que falta a chave da typesafe;
  - a janela do Copilot abre sem erro de JS.
  Nao grava reuniao nem toca audio. Leva ~40 s.
.EXAMPLE
  .\tools\e2e\copilot-filter.ps1 -Exe .\target\release\isper-app.exe
  .\tools\e2e\copilot-filter.ps1 -Exe .\target\release\isper-app.exe -UseStoredKey
#>
param([string]$Exe, [switch]$UseStoredKey)
. "$PSScriptRoot\common.ps1"
$exe = Get-IsperExe $Exe
"copilot-filter: $exe"

function Read-StoredKey {
  # Credencial generica 'typesafe.ISPer' (o formato do crate keyring: UTF-16).
  Add-Type -Namespace IsperE2E -Name Cred -MemberDefinition @'
[DllImport("advapi32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
public static extern bool CredReadW(string target, int type, int flags, out IntPtr cred);
[DllImport("advapi32.dll")]
public static extern void CredFree(IntPtr cred);
'@ -ErrorAction SilentlyContinue
  $p = [IntPtr]::Zero
  if (-not [IsperE2E.Cred]::CredReadW('typesafe.ISPer', 1, 0, [ref]$p)) { return $null }
  try {
    # CREDENTIALW: Flags, Type, TargetName*, Comment*, LastWritten(8), BlobSize, Blob*
    $size = [Runtime.InteropServices.Marshal]::ReadInt32($p, 32)
    $blob = [Runtime.InteropServices.Marshal]::ReadIntPtr($p, 40)
    return [Runtime.InteropServices.Marshal]::PtrToStringUni($blob, [int]($size / 2))
  } finally { [IsperE2E.Cred]::CredFree($p) }
}

$envs = @{}
if ($UseStoredKey) {
  $key = Read-StoredKey
  Check ([bool]$key) "chave da TypeSafe encontrada no Credential Manager (typesafe.ISPer)"
  if ($key) { $envs['ISPER_TYPESAFE_API_KEY'] = $key }
}

Stop-Isper
Check (Start-Isper -Exe $exe -Env $envs) "app abriu com a tela Inicio (CDP)"

Invoke-Isper 'open_settings_window' | Out-Null
Check (Wait-IsperWindow 'settings.html') "Configuracoes abriram"
$ui = EvJson 'settings.html' 'JSON.stringify({ sw: !!document.getElementById("copfilter"), key: !!document.getElementById("tskey"), save: !!document.getElementById("savetskey"), test: !!document.getElementById("tstest"), title: [...document.querySelectorAll("h3")].map(h => h.textContent).filter(t => /Copilot/.test(t)), errors: window.__isperErrors || [] })'
Check ($ui -and $ui.sw -and $ui.key -and $ui.save -and $ui.test) "bloco do filtro na tela (interruptor, chave, guardar, testar)"
Check ($ui -and @($ui.title).Count -eq 1) "titulo do bloco traduzido: $(@($ui.title) -join ', ')"
Check ($ui -and @($ui.errors).Count -eq 0) "Configuracoes sem erro de JS $(Format-JsErrors $ui.errors)"

$s = Invoke-Isper 'get_settings' 'undefined' 'settings.html'
Check ($s -and $s.copilot_filter -eq 'off') "filtro nasce desligado (copilot_filter = $($s.copilot_filter))"
Check ($s -and [bool]$s.typesafe_key_present -eq [bool]$UseStoredKey) "presenca da chave: $($s.typesafe_key_present)"

# Liga pelo caminho do usuario: interruptor + Salvar.
Ev 'settings.html' 'document.getElementById("copfilter").checked = true; document.getElementById("save").click(); "ok"' | Out-Null
Start-Sleep -Seconds 2
$s = Invoke-Isper 'get_settings' 'undefined' 'settings.html'
Check ($s -and $s.copilot_filter -eq 'jev') "Salvar liga o filtro (copilot_filter = $($s.copilot_filter))"
$cfg = Join-Path (Get-E2EDataPath) 'config.toml'
Check ((Test-Path $cfg) -and ((Get-Content $cfg -Raw) -match 'copilot_filter = "jev"')) "config.toml guarda copilot_filter = jev"

$t = Invoke-Isper 'test_typesafe' 'undefined' 'settings.html'
if ($UseStoredKey) {
  Check ($t -and -not $t.__error -and "$t" -match 'jev-1\.13\.0' -and "$t" -match 'p\(card\) (0\.[5-9]|1\.0)') "teste de conexao real marca o compromisso: $t"
} else {
  Check ($t -and "$($t.__error)" -match 'typesafe') "sem chave, o teste diz o que falta: $($t.__error)"
}

Ev 'settings.html' 'document.getElementById("copfilter").checked = false; document.getElementById("save").click(); "ok"' | Out-Null
Start-Sleep -Seconds 2
$s = Invoke-Isper 'get_settings' 'undefined' 'settings.html'
Check ($s -and $s.copilot_filter -eq 'off') "desligar tambem fica salvo"

Invoke-Isper 'open_copilot_window' | Out-Null
Check (Wait-IsperWindow 'copilot.html') "Copilot abriu"
$cop = Invoke-Isper 'copilot_get_state' 'undefined' 'copilot.html'
Check ($cop -and $null -eq $cop.filter) "sem reuniao, o Copilot nao mostra filtro"
$errs = Get-JsErrors 'copilot.html'
Check (@($errs).Count -eq 0) "Copilot sem erro de JS $(Format-JsErrors $errs)"

Restart-IsperClean $exe
Finish-E2E 'copilot-filter'
