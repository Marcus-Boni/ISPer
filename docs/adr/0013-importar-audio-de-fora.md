# 0013 — Importar áudio de fora: decodificação local, o mesmo passe final e a origem no banco

- **Status:** aceita
- **Data:** 23/09/2026

## Contexto

A Fase 9 ("ISPer no Bolso") nasceu de uma conversa com o líder do usuário,
em 22/09: o time usa o Plaud para gravar reuniões presenciais, e 90% do uso
é baixar a transcrição. Nada disso precisa de tempo real — grava-se no
aparelho e processa-se depois. O primeiro passo, a 9.0, vale sozinho e não
depende de app de celular: o ISPer do PC passa a transcrever **arquivos** de
áudio (o que o app do Plaud exporta, o gravador do celular, a gravação de
uma reunião do Teams).

Até a 0.20.0, o app só transcrevia o que ele mesmo gravava, e o `isper-cli`
só lia WAV. O que existia e servia:

- o **passe final** (`pipeline::run`, [ADR 0006](0006-dois-modos-ao-vivo-e-passe-final.md))
  já recebe um único buffer de 16 kHz mono e já sabe diarizar
  ([ADR 0005](0005-diarizacao-pos-hoc.md));
- o `.md` é o artefato durável, reimportável por `isper-cli import`
  ([ADR 0009](0009-nada-some-sem-o-usuario-pedir.md)).

## Decisão

1. **Decodificação local, em Rust puro, com o symphonia 0.6**
   (`isper_core::decode`): MP3, M4A e MP4 (AAC), AAC cru (ADTS), WAV, FLAC e
   OGG Vorbis. A licença (MPL-2.0) já é aceita pelo `cargo deny`.
2. **Em fluxo**: cada pacote decodificado vira mono e vai direto para um
   reamostrador incremental (`Resampler16k`, o mesmo filtro do
   `resample_to_16k`, que agora é feito com ele). A memória fica só no
   resultado, ~230 MB por hora de áudio; decodificar tudo e converter
   depois custaria 1,4 GB por hora num arquivo de celular 48 kHz estéreo.
   A leitura informa progresso (pela posição no arquivo) e pode ser
   cancelada.
3. **O mesmo passe final** sobre a trilha única, com diarização: as falas
   saem como "Participante 1, 2…" (ou "Participantes", se o agrupamento for
   implausível). Não há canal "Eu" — numa gravação presencial todos estão no
   mesmo microfone. O passe final ganhou `run_cancellable`, e o `run`
   continua igual.
4. **A origem fica no banco** (schema v5: `meetings.source_name` e
   `source_sha256`). O nome aparece na Biblioteca e no cabeçalho do `.md`
   (`> Importada do arquivo …`), e a linha é lida de volta pelo `import`. O
   SHA-256 impede importar o mesmo áudio duas vezes.
5. **Data e título pelo nome do arquivo** (`isper_core::recording`). A data
   de modificação é a da exportação, não a da conversa. Então vale a data e
   hora escritas no nome (`2026-09-22 15-30-46`, `REC_20260922_153046`,
   `22-09-2026 15h30`); sem ela, a do arquivo. O nome vira o título quando é
   descritivo ("Reunião com fornecedor"); um carimbo de gravador
   ("REC_0012", "Nova gravação 3") dá lugar ao título da IA ou ao padrão.
6. **A reimportação passa a casar por início e título**, e não só pelo
   início, que tem precisão de minuto: duas gravações exportadas juntas
   podem começar no mesmo minuto, e uma sumiria.

## Consequências

- **Fica melhor:** qualquer gravação vira uma reunião da Biblioteca, com o
  mesmo texto de uma reunião gravada pelo ISPer. No teste com o modelo, um
  MP3 44,1 kHz estéreo deu exatamente o texto do WAV original. O
  `isper-cli` (`file`, `bench`, `diarize`) lê os mesmos formatos.
- **Fica pior:** o binário cresce com os decodificadores (Rust puro, sem
  DLL nova). O AAC não descarta o atraso do codificador (sem *gapless*): uns
  20–40 ms de silêncio no começo, irrelevantes para a transcrição.
- **Passa a ser obrigatório:** toda regravação da ata usa o
  `meeting_markdown` do app, que monta a linha da origem a partir do
  `MeetingDetail` — a mesma regra que as decisões do Copilot (#67). Um teste
  segura isso (`ata_regravada_de_um_audio_importado_mantem_a_origem`).

## Alternativas consideradas

- **ffmpeg como programa auxiliar:** lê tudo, inclusive Opus, mas são ~80 MB
  a mais no instalador, um processo externo para vigiar e uma licença (LGPL
  ou GPL, conforme o build) para acompanhar.
- **Media Foundation do Windows:** sem dependência nova, mas os codecs mudam
  por edição (as edições "N" vêm sem eles) e o motor ficaria preso a COM.
- **Opus via libopus agora:** o `.opus` das mensagens de voz ficou de fora.
  O symphonia ainda não tem o decodificador, e o libopus traria uma
  biblioteca C para o build. O erro diz isso ao usuário, com a saída
  (converter para MP3 ou WAV).
- **Decodificar tudo e converter depois:** mais simples, mas a memória de
  uma reunião longa (item 2) inviabiliza.

## Onde vive

- `crates/isper-core/src/decode.rs`, `recording.rs`, `audio.rs`
  (`Resampler16k`), `pipeline.rs` (`run_cancellable`), `store.rs`
  (`migrate_to_v5`, `save_imported`, `meeting_with_source`), `meeting.rs`
  (`insert_source_line`) e `import.rs`.
- `apps/isper-app/src-tauri/src/library.rs` (`meeting_markdown`).
- Fixtures em `fixtures/formatos/`, gerados do `fala-16k.wav` com o ffmpeg.
- A importação pela Biblioteca e a pasta vigiada ficam no PR seguinte da 9.0.
