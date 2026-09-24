# 0016 — Gravação no celular: Ogg/Opus a 32 kbit/s, em páginas de 1 s, com um manifesto ao lado

- **Status:** aceita
- **Data:** 24/09/2026

## Contexto

A Fase 9.2 põe o gravador dentro do app Android da 9.1
([ADR 0014](0014-celular-nativo-com-nucleo-rust.md)). É o que substitui o
Plaud: o celular grava a reunião inteira, com a tela apagada, e o texto sai
depois, no PC (9.3) ou no próprio aparelho. Um gravador de reunião tem de
aguentar o que acontece com um celular numa sala:

- **o processo morrer no meio.** Fabricantes matam apps em segundo plano
  (Xiaomi, Samsung e Motorola, cada um de um jeito), a bateria acaba, o
  sistema reinicia. O que já foi gravado não pode se perder;
- **uma ligação entrar.** O Android dá o microfone à chamada e passa a
  entregar silêncio a quem estava gravando;
- **horas de áudio.** Uma reunião de 2 h em WAV de 16 kHz ocupa 230 MB; em
  48 kHz, 690 MB. Isso pesa no aparelho e na ida para o PC.

O formato mais óbvio no Android, M4A (AAC) pelo `MediaRecorder`, escreve o
índice (`moov`) só no fim: um processo morto deixa um arquivo que nenhum
player abre. E o `MediaRecorder` não entrega o PCM, que o app usa para a
onda na tela.

## Decisão

1. **Ogg/Opus, mono, 32 kbit/s VBR** (`Application::Voip`,
   `Signal::Voice`), codificado pelo libopus oficial no núcleo Rust
   (`isper_core::ogg_opus`), e não no Kotlin. O Android entrega PCM do
   `AudioRecord` (fonte `MIC`, 48 kHz, ou 16 kHz se o aparelho não abrir a
   48 kHz) em blocos de 100 ms. Medido no corpus de regressão (190 s,
   3 vozes, `isper-cli encode` e o passe final do PC):

   | áudio | WER | CER | DER | tamanho |
   |---|---|---|---|---|
   | WAV 16 kHz (referência) | 9,86% | 7,17% | 23,46% | 115 MB/h |
   | Opus 32 kbit/s | 9,86% | 7,17% | 22,62% | 10,5 MB/h |
   | Opus 24 kbit/s | 9,86% | 7,17% | 20,72% | — |

   A transcrição sai **idêntica** à do WAV. O DER oscila alguns pontos entre
   rodadas do agrupamento: não é ganho nem perda. O VBR fica, em média, em
   23,4 kbit/s. Ficamos nos 32, não nos 24, pela margem em salas ruidosas e
   com a voz longe do celular: a economia seria de ~2 MB por hora.
2. **À prova de queda.** Uma página Ogg fecha a cada segundo (50 pacotes de
   20 ms), e o arquivo vai para o disco (`sync_data`) a cada 5 páginas. A
   leitura aceita um arquivo truncado: vale até a última página inteira. Se
   só o processo morre, perde-se a página em andamento (até 1 s); se o
   aparelho desliga de repente, até os 5 s desde o último sync. No e2e,
   matar o app aos 13,5 s devolveu 13,0 s de áudio.
3. **Um manifesto JSON ao lado de cada `.opus`** (`isper_core::capture`),
   escrito **antes** do áudio e trocado de forma atômica (arquivo temporário
   + rename). Ele guarda o estado (`recording`, `finished`, `recovered`,
   `imported`), a duração, os momentos marcados e os trechos sem áudio
   (`silenced` numa ligação, `paused`, `reopened` quando o microfone caiu e
   voltou). Ao abrir, o app procura manifestos em `recording` que não são a
   gravação em andamento e os fecha como `recovered`, com a duração lida do
   próprio Ogg. É o mesmo princípio do `.md` no desktop: o arquivo é a
   verdade, e um índice pode ser refeito a partir dele.
4. **Ligação e microfone.** O `AudioRecordingCallback` avisa quando o
   Android silencia a gravação (`isClientSilenced`). A gravação continua, com
   silêncio no lugar, para a linha do tempo não se deslocar, e o trecho fica
   marcado. Se o `read` falhar (o servidor de áudio reiniciou), o microfone é
   reaberto na mesma taxa, e a reabertura fica marcada.
5. **Serviço em primeiro plano do tipo microfone**, com notificação
   (cronômetro, Marcar, Pausar e Parar) e um *wake lock* parcial com limite
   de 12 h. `START_NOT_STICKY`: se o sistema matar o serviço, o app não
   começa uma gravação nova sozinho, e a antiga é recuperada ao abrir.
6. **Só o próprio ISPer começa uma gravação.** O widget e o bloco das
   Configurações rápidas abrem uma activity **não exportada**
   (`RecordShortcutActivity`), que pede a permissão e liga o serviço. O
   Android não deixa um serviço de microfone começar com o app em segundo
   plano, e essa activity é o que põe o app em primeiro plano. A
   `MainActivity`, exportada, aceita apenas o *compartilhar* (importar um
   áudio) e o `autorun` do laboratório, que processa sem gravar. Nenhum outro
   app liga o microfone do ISPer por intent.
7. **As gravações ficam em `Android/data/com.isper.mobile/files/Gravacoes`**:
   sem permissão de armazenamento, fora da galeria e do backup
   (`allowBackup=false`). Apagar tem Desfazer
   ([ADR 0009](0009-nada-some-sem-o-usuario-pedir.md)).
   `hasFragileUserData` faz o Android oferecer manter essa pasta ao
   desinstalar, para uma reinstalação com a mesma chave. Os APKs do CI são assinados com **uma chave fixa**, guardada
   nos segredos do repositório, para a versão nova instalar por cima. O
   Android só atualiza um app assinado com a mesma chave. Com a chave de
   debug de cada run, atualizar exigia desinstalar, e isso apagava as
   gravações.
8. **O desktop lê Opus.** A mesma decodificação serve às mensagens de voz do
   WhatsApp (`.opus`, ou `.ogg` com Opus dentro), que a 9.0 recusava
   ([ADR 0013](0013-importar-audio-de-fora.md)).

## Consequências

- **Fica melhor:** uma hora de reunião ocupa ~10,5 MB, 11× menos que o WAV
  de 16 kHz, com a mesma transcrição. Mandar para o PC (9.3) fica barato, e
  64 GB livres cabem milhares de horas.
- **Fica melhor:** o que foi gravado sobrevive a uma queda do processo. Isso
  está provado no emulador (`am force-stop`) e nos testes do núcleo (arquivo
  truncado a 60%).
- **Fica melhor:** o formato e o manifesto vivem no núcleo Rust. O iOS (9.7)
  reaproveita o mesmo código, e o desktop ganhou Opus sem nada novo.
- **Fica pior:** uma biblioteca C a mais no build (libopus via `opusic-sys`,
  BSD-3, compilada pelo CMake junto do whisper.cpp) e alguns segundos a mais
  na compilação limpa.
- **Fica pior:** uma ligação vira silêncio na gravação. É o Android que
  decide, e o manifesto registra o trecho. Gravar os dois lados da chamada
  não é permitido a apps comuns.
- **Obrigatório:** a chave de assinatura (o `.jks` e a senha) tem de ser
  guardada fora da máquina. Perdida, a próxima versão não atualiza por cima,
  e cada aparelho precisa desinstalar **e apagar os dados**: o Android recusa
  uma chave nova mesmo com os dados mantidos (testado no emulador,
  `INSTALL_FAILED_UPDATE_INCOMPATIBLE`). O que não foi para o PC se perde.
- **Pendente:** o critério de pronto da 9.2 é num aparelho de verdade: 2 h
  com a tela apagada, uma ligação no meio, o app morto aos 90 min, num
  Samsung e num Motorola ou Xiaomi. O emulador cobre o fluxo, não os
  matadores de processo dos fabricantes.

## Alternativas consideradas

- **M4A/AAC pelo `MediaRecorder`:** o formato nativo, mas o arquivo fica
  ilegível se o processo morrer antes do `stop()`, e não há PCM para a onda
  nem para detectar o silêncio de uma ligação.
- **Ogg/Opus pelo `MediaRecorder`** (Android 10+): resolve o índice, mas não
  controla quando as páginas vão para o disco, não entrega o PCM e ficaria
  só no Android.
- **WAV ou FLAC:** sem perda e fáceis de recuperar, mas 115 MB/h (WAV
  16 kHz) e ~50 MB/h (FLAC), para uma transcrição que o Opus já deixa igual.
- **Opus a 24 kbit/s ou menos:** a 24 a transcrição é a mesma, mas sobra
  pouca margem para uma sala ruim, e a economia não compensa. Abaixo disso,
  não foi medido.
- **Codificar no Kotlin (`MediaCodec`) e só mandar o arquivo para o Rust:**
  duplicaria o empacotamento Ogg, que o iOS e o desktop também precisam, e
  tiraria do núcleo o controle da durabilidade.
- **Chave de debug comum versionada no repositório:** sem segredo para
  configurar, mas é uma chave pública. Qualquer APK assinado com ela
  atualizaria o ISPer instalado e leria as gravações.

## Onde vive

- `crates/isper-core/src/ogg_opus.rs` (gravar e ler Ogg/Opus),
  `crates/isper-core/src/capture.rs` (manifesto, recuperação, importar),
  `crates/isper-core/src/decode.rs` (o desktop lendo Opus)
- `crates/isper-mobile/src/recording.rs` (a fachada `Recorder` para o
  Kotlin)
- `apps/isper-android/app/src/main/java/com/isper/mobile/recording/`
  (serviço, widget, bloco, atalho, tela) e `.../library/` (a Biblioteca do
  celular)
- `crates/isper-cli/src/main.rs` (`isper-cli encode`, usado na medição)
- `tools/e2e/android-recorder.ps1` (gravar, queda e tela apagada no
  emulador), `.github/workflows/android.yml` (assinatura)
