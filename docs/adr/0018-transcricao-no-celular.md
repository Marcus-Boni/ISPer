# 0018 — Transcrição no celular: o passe final do PC, retomável, com o modelo pela memória

- **Status:** aceita
- **Data:** 26/09/2026

## Contexto

Com a 9.3, o celular grava e o PC transcreve. Mas o PC nem sempre está ao
alcance: numa visita, numa viagem ou num celular sem PC pareado, a gravação
ficava esperando. A Fase 9.4 faz o celular transcrever sozinho. O critério do
plano é que 1 h de reunião vire ata num intermediário, sem PC, dentro de uma
meta de tempo que o laboratório da 9.1 fixa.

As restrições:

- **não há pressa:** o líder aceita a ata depois ("não precisaria fazer
  transcrição em tempo real", 22/09). O celular só precisa do modo final;
- **o processador de celular é mais lento:** num Snapdragon 855, o modelo small
  levou 1,43× a duração do áudio (laboratório, 24/09). Uma reunião de 1 h dá
  mais de uma hora de CPU, com bateria e calor junto;
- **o Android interrompe trabalhos longos:** a tomada sai, o sistema precisa
  da memória, o app é fechado. Um trabalho comum do WorkManager tem 10 min;
- **a memória vai de 3 a 12 GB:** o modelo que cabe num topo de linha não
  cabe num aparelho de entrada;
- **a ata do PC é melhor:** o modelo turbo na GPU, a separação de falantes
  e o resumo por IA.

## Decisão

1. **O mesmo passe final do PC** (`isper_core::pipeline`): VAD, busca em
   feixe, contexto entre janelas e falante por palavra. Outro motor ficaria
   com outros números, outro dicionário e outro jeito de errar. A ata sai pelo
   mesmo `render_markdown`, e a regra de "Participante N" ou "Participantes"
   (`FinalTranscript::speaker_rows`) passou para o núcleo, e o PC usa a mesma.

2. **A transcrição continua de onde parou** (`pipeline::run_resumable`).
   Cada janela do VAD concluída vira uma linha num arquivo ao lado da gravação
   (`<id>.transcricao.jsonl`), com os segmentos e o estado que a próxima janela
   precisa: o texto que vira contexto e onde terminou a última palavra. Um
   cabeçalho descreve a rodada inteira (modelo, idioma, decodificação, as
   janelas do VAD, o tamanho do áudio). Se qualquer coisa mudar, o arquivo não
   serve e a transcrição recomeça. Uma última linha pela metade é descartada.
   Retomada assim, a transcrição chega ao **mesmo texto** de uma sem
   interrupção, e o teste com a reunião sintética de 190 s confere segmento
   por segmento. O cabeçalho é comparado **como texto**: um número com vírgula
   lido de volta de um JSON pode sair com o último bit diferente, e aí nenhuma
   rodada bateria com a guardada.

3. **O modelo pela memória do aparelho** (`device_plan`), como ponto de
   partida:

   | Memória (o que o Android informa) | Modelo | Falantes |
   |---|---|---|
   | ≥ 5 GB (um "6 GB" informa ~5,3) | small (181 MB) | no celular |
   | 3 a 5 GB | base (57 MB) | no PC |
   | < 3 GB | só o PC transcreve | no PC |

   A separação de falantes no celular fica ligada a partir de 5 GB porque, no
   Snapdragon 855, ela ficou perto da do PC (DER 22,1% contra 20,6%). Quem usa
   troca o modelo e liga ou desliga os falantes nos Ajustes. O laboratório
   continua medindo cada aparelho, e os limites mudam com os números.

4. **Quando transcrever, pelo WorkManager.** O padrão é **ao carregar**
   (`requiresCharging`, e com espaço livre). Há também "assim que a gravação
   termina", na bateria, e "só no PC". "Transcrever agora" na Biblioteca vale
   em qualquer modo. Antes vem o download dos modelos (`ModelsWorker`): no
   Wi-Fi por padrão, ~230 MB na primeira vez, e com qualquer rede quando quem
   usa pede. Os dois rodam em primeiro plano, com a notificação do andamento.
   A transcrição usa o tipo "processamento de mídia" a partir do Android 15;
   antes dele, dataSync, que cobre "processar arquivos locais". Parado pelo
   sistema, o trabalho interrompe a transcrição na janela em que está, e o
   WorkManager roda de novo quando as condições voltam.

5. **O PC primeiro, quando ele já tem o áudio.** Uma gravação que o PC
   recebeu inteira (na fila, processando ou pronta) não é transcrita no
   celular: a ata vem pela sincronia. A que o celular fez fica em
   `<id>.ata.md`, o mesmo lugar da do PC, com um `<id>.ata.json` que diz que
   ela é do celular. Quando a do PC chega, ela substitui a do celular, e o
   `.ata.json` sai. A sincronia passou a buscar a ata do PC também quando a
   que existe é do celular.

6. **Uma falha fica anotada** (`<id>.transcricao.erro`) e tira a gravação da
   fila até "Tentar de novo". Sem isso, um áudio corrompido seria tentado em
   toda rodada. Uma gravação sem nenhuma fala também vira falha, com o motivo,
   porque uma ata vazia não serve a ninguém.

## Consequências

- **Fica melhor:** a gravação vira ata sem PC e sem rede. No emulador, a
  transcrição tirada da tomada no meio continuou de onde parou e terminou
  com a ata ("Ata do celular"), e a ata do PC, quando chega, fica no lugar.
- **Custa bateria e processador:** por isso o padrão é ao carregar. Na
  bateria, é só com um pedido de quem usa.
- **O áudio inteiro fica na memória** (16 kHz, f32: ~230 MB por hora). Numa
  reunião de 2 h são ~460 MB, além do modelo. Os limites da tabela contam com
  isso. Ler o áudio em partes, janela por janela, é o próximo passo, se um
  aparelho de 3 a 4 GB sofrer com reuniões longas.
- **Android 15 limita o primeiro plano** de dataSync e processamento de mídia a
  6 h por dia. Uma reunião de 1 h a 1,5× cabe folgada; uma fila grande
  continua no dia seguinte, de onde parou.
- **Ficaram para depois:** o "minha voz" (rotular o dono do celular pela voz)
  e preservar edições quando a ata do PC substitui a do celular. O celular
  ainda não edita a ata.

## Alternativas consideradas

- **Parakeet TDT 0.6B pelo sherpa-onnx.** É candidato a um nível rápido
  (4,76% de WER em português no FLEURS), mas foi treinado em português europeu,
  pesa ~620 MB em int8 e precisa ser medido em pt-BR antes. Entra se o
  laboratório mostrar ganho.
- **ML Kit GenAI (Google).** Em pt-BR só no modo básico, e o áudio de um
  arquivo precisa entrar em ritmo de tempo real: uma reunião de 1 h levaria
  1 h, sem o passe final.
- **Recomeçar do zero a cada interrupção.** Mais simples, mas joga fora o que
  foi feito. Num aparelho lento, com a tomada entrando e saindo, uma reunião
  longa talvez nunca terminasse.
- **Guardar por segmento, e não por janela.** O Whisper decodifica uma janela
  inteira (até ~30 s) por vez. Guardar mais fino não economizaria nada, e a
  janela é onde o contexto passa de uma para a outra.
- **Deixar só o PC transcrever.** É o que a 9.3 já faz. A 9.4 existe para o
  celular que está longe do PC, ou que não tem PC.

## Onde vive

- `crates/isper-core/src/resume.rs` (o arquivo de retomada),
  `crates/isper-core/src/pipeline.rs` (`run_resumable`, `resume_progress`,
  `speaker_rows`) e o teste
  `o_passe_final_interrompido_continua_de_onde_parou_com_o_mesmo_texto` em
  `crates/isper-core/tests/pipeline.rs`
- `crates/isper-mobile/src/local.rs` (`device_plan`, `pending_local`,
  `LocalTranscription`, a origem da ata e as falhas)
- `apps/isper-android/app/src/main/java/com/isper/mobile/transcribe/`
  (`LocalTranscribe`, `ModelsWorker`, `TranscribeWorker`, os Ajustes)
- `tools/e2e/android-transcribe.ps1` (no emulador)
