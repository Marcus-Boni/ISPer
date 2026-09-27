# A transcricao no proprio celular (Fase 9.4) de ponta a ponta, num emulador:
#   1. nos Ajustes, o modelo Tiny e os falantes desligados (rapido no emulador);
#   2. "Ao carregar": fora da tomada a gravacao espera; na tomada, comeca;
#   3. tirar da tomada no meio: para, e o que foi feito fica guardado;
#   4. de volta na tomada: continua de onde parou, e a ata sai ("Ata do celular");
#   5. "Transcrever agora", fora da tomada, numa segunda gravacao;
#   6. um audio corrompido vira "Falhou no celular" e sai da fila.
#
#   powershell -File tools\e2e\android-transcribe.ps1 [-Apk <app-debug.apk>] [-KeepEmulator]
#
# As gravacoes entram direto na pasta do app: um .opus feito pelo
# `isper-cli encode` e o manifesto ao lado, como o gravador deixaria. A tomada
# e simulada pelo `dumpsys battery`. Precisa do isper-cli compilado
# (cargo build --release -p isper-cli --no-default-features basta) e de rede
# no emulador na primeira vez (o modelo Tiny, ~31 MB).

param(
  [string]$Apk = '',
  [switch]$KeepEmulator
)

. "$PSScriptRoot\android-common.ps1"

$cli = Join-Path $script:AndroidRoot 'target\release\isper-cli.exe'
if (-not (Test-Path $cli)) { throw "isper-cli nao encontrado: $cli (cargo build --release -p isper-cli --no-default-features)" }
$fixtures = Join-Path $script:AndroidRoot 'fixtures'
$dir = "/sdcard/Android/data/$script:Pkg/files/Gravacoes"
$work = Join-Path ([IO.Path]::GetTempPath()) ('isper-e2e-android-transcribe-' + [guid]::NewGuid().ToString('N').Substring(0, 8))
New-Item -ItemType Directory -Force $work | Out-Null
$utf8 = New-Object System.Text.UTF8Encoding($false)

function Set-Charging([bool]$On) {
  if ($On) {
    Adb shell dumpsys battery set ac 1 | Out-Null
    Adb shell dumpsys battery set status 2 | Out-Null
  } else {
    Adb shell dumpsys battery unplug | Out-Null
    Adb shell dumpsys battery set status 3 | Out-Null
  }
}

function Push-Recording([string]$Id, [string]$Opus, [double]$Secs) {
  $manifest = [ordered]@{
    version = 1; id = $Id; started_at = '2026-09-26T10:00:00-03:00'; state = 'finished'
    audio_file = "$Id.opus"; duration_secs = $Secs; moments = @(); gaps = @()
    sample_rate = 16000; source_name = $null
  } | ConvertTo-Json -Compress
  $json = Join-Path $work "$Id.json"
  [IO.File]::WriteAllText($json, $manifest, $utf8)
  Adb push $Opus "$dir/$Id.opus" 2>$null | Out-Null
  Adb push $json "$dir/$Id.json" 2>$null | Out-Null
}

function Test-DeviceFile([string]$Path) {
  ((Adb shell "ls $Path 2>/dev/null") -join '') -match [regex]::Escape(($Path -split '/')[-1])
}

function Get-Percent([string]$Text) {
  $m = [regex]::Match([string]$Text, '(\d+)%')
  if ($m.Success) { return [int]$m.Groups[1].Value }
  return -1
}

Start-IsperEmulator
Install-IsperApk -Apk $Apk
Adb shell am force-stop $script:Pkg | Out-Null
# Estado limpo: sem gravacoes, sem PC pareado e com os ajustes de fabrica. Os
# modelos ficam (baixar de novo a cada rodada so gastaria rede).
Adb shell rm -rf $dir 2>$null | Out-Null
Adb shell run-as $script:Pkg rm -rf files/sync shared_prefs/transcricao.xml 2>$null | Out-Null
Adb shell mkdir -p $dir | Out-Null
Set-Charging $false

# ------------------------------------------------------------- as gravacoes
$a = '20260926-100000'   # a reuniao sintetica de 190 s: varias janelas
$b = '20260926-110000'   # 29 s de fala
$c = '20260926-120000'   # corrompida
& $cli encode (Join-Path $fixtures 'reuniao-sintetica-16k.wav') (Join-Path $work "$a.opus") *> $null
& $cli encode (Join-Path $fixtures 'fala-16k.wav') (Join-Path $work "$b.opus") *> $null
$junk = New-Object byte[] 40000
(New-Object Random 7).NextBytes($junk)
[IO.File]::WriteAllBytes((Join-Path $work "$c.opus"), $junk)
Check ((Test-Path (Join-Path $work "$a.opus")) -and (Test-Path (Join-Path $work "$b.opus"))) "o isper-cli fez os .opus de teste"

try {
  # ------------------------------------------------------------ 1. ajustes
  "== 1. Ajustes: o modelo Tiny, sem falantes"
  Open-Tab 1
  Check (Invoke-UiTap -Tag 'ajustes') "a engrenagem abre os Ajustes"
  Check (Invoke-UiTap -Tag 'modo-carregando') "modo 'Ao carregar'"
  Check (Invoke-UiTap -Tag 'modelo-ggml-tiny-q5_1.bin') "modelo Tiny"
  # O apply() do SharedPreferences grava em segundo plano: espera o arquivo.
  $prefs = ''
  for ($i = 0; $i -lt 20 -and -not ($prefs -match 'ggml-tiny-q5_1.bin' -and $prefs -match 'carregando'); $i++) {
    Start-Sleep -Milliseconds 500
    $prefs = (Adb shell run-as $script:Pkg cat shared_prefs/transcricao.xml 2>$null) -join ''
  }
  Check ($prefs -match 'ggml-tiny-q5_1.bin' -and $prefs -match 'carregando') "os ajustes ficaram guardados"
  # No emulador (4 GB) o plano nao separa falantes; se estiver ligado, desliga.
  if (((Adb shell run-as $script:Pkg cat shared_prefs/transcricao.xml 2>$null) -join '') -match 'name="falantes" value="true"') {
    Invoke-UiTap -Tag 'falantes-no-celular' | Out-Null
  }
  Check (Invoke-UiTap -Tag 'ajustes-voltar') "de volta a Biblioteca"

  # --------------------------------------------- 2. fora da tomada, espera
  "== 2. fora da tomada: a gravacao espera"
  Push-Recording $a (Join-Path $work "$a.opus") 190
  Open-Tab 1
  $chip = Wait-UiText "local-$a" { param($t) $t -like 'Transcreve ao carregar*' } 30
  Check ($chip -like 'Transcreve ao carregar*') "a Biblioteca diz '$chip'"
  Start-Sleep -Seconds 12
  Check (-not (Test-DeviceFile "$dir/$a.transcricao.jsonl")) "fora da tomada, nada comecou"

  # ------------------------------------------------ 3. na tomada, comeca
  "== 3. na tomada: comeca (baixando o modelo na primeira vez)"
  Set-Charging $true
  $chip = Wait-UiText "local-$a" { param($t) (Get-Percent $t) -ge 20 -and $t -notlike '*continua*' } 300
  Check ((Get-Percent $chip) -ge 20) "transcrevendo no celular: '$chip'"

  # ---------------------------------- 4. fora da tomada no meio: para
  "== 4. tirar da tomada no meio: para e guarda o que foi feito"
  Set-Charging $false
  $chip = Wait-UiText "local-$a" { param($t) $t -like '*continua ao carregar*' } 60
  Check ($chip -like '*continua ao carregar*') "parou: '$chip'"
  $lines = @(Adb shell cat "$dir/$a.transcricao.jsonl" 2>$null).Count
  Check ($lines -ge 2) "o arquivo de retomada guarda $($lines - 1) janela(s)"
  Check (-not (Test-DeviceFile "$dir/$a.ata.md")) "sem ata ainda"

  # ----------------------------- 5. de volta na tomada: continua e termina
  "== 5. de volta na tomada: continua de onde parou"
  Adb logcat -c | Out-Null
  Set-Charging $true
  $chip = Wait-UiText "local-$a" { param($t) $t -eq 'Ata do celular' } 300
  Check ($chip -eq 'Ata do celular') "a Biblioteca diz '$chip'"
  $log = (Adb logcat -d -s ISPerTranscricao 2>$null) -join "`n"
  Check ($log -match "$a.*continuando \d+ janelas") "continuou de onde tinha parado ($([regex]::Match($log, 'continuando \d+ janelas').Value))"
  $md = (Adb shell cat "$dir/$a.ata.md" 2>$null) -join "`n"
  Check ($md -match '^# Reuni' -and $md -match '26/09/2026 10:00') "a ata tem o titulo com a hora da gravacao"
  Check ($md -match 'Feita no celular, com o modelo Tiny \(q5\)') "a ata diz que foi feita no celular, com o Tiny"
  Check ($md -match '\*\*\[00:') "a ata tem as falas"
  $meta = (Adb shell cat "$dir/$a.ata.json" 2>$null) -join '' | ConvertFrom-Json
  Check ($meta.origem -eq 'device') "a ata esta marcada como do celular"
  Check (-not (Test-DeviceFile "$dir/$a.transcricao.jsonl")) "o arquivo de retomada saiu"
  Check (Invoke-UiTap -Tag "ver-ata-$a") "Ver a ata"
  $title = Wait-UiText 'ata-titulo' { param($t) $t } 20
  Check ($title -like '*26/09/2026 10:00') "a tela da ata abriu: '$title'"
  Adb shell input keyevent KEYCODE_BACK | Out-Null

  # --------------------------------- 6. "Transcrever agora", na bateria
  "== 6. 'Transcrever agora', fora da tomada"
  Set-Charging $false
  Push-Recording $b (Join-Path $work "$b.opus") 29
  Open-Tab 1
  Check (Invoke-UiTap -Tag "transcrever-$b") "botao 'Transcrever agora'"
  $chip = Wait-UiText "local-$b" { param($t) $t -eq 'Ata do celular' } 180
  Check ($chip -eq 'Ata do celular') "sem tomada, a ata saiu: '$chip'"

  # ----------------------------------------------- 7. audio corrompido
  "== 7. um audio corrompido vira falha anotada"
  Push-Recording $c (Join-Path $work "$c.opus") 10
  Open-Tab 1
  Check (Invoke-UiTap -Tag "transcrever-$c") "botao 'Transcrever agora'"
  $chip = Wait-UiText "local-$c" { param($t) $t -eq 'Falhou no celular' } 120
  Check ($chip -eq 'Falhou no celular') "a Biblioteca diz '$chip'"
  Check (Test-DeviceFile "$dir/$c.transcricao.erro") "a falha ficou anotada (e a gravacao saiu da fila)"
  Check (Invoke-UiTap -Tag "tentar-no-celular-$c") "Tentar de novo"
  $chip = Wait-UiText "local-$c" { param($t) $t -like 'Transcreve ao carregar*' } 30
  Check ($chip -like 'Transcreve ao carregar*') "de volta a fila: '$chip'"
} finally {
  Adb shell dumpsys battery reset | Out-Null
  Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
}

Stop-IsperEmulatorIfStarted -Keep:$KeepEmulator
Finish-Android -Name 'android-transcribe'
