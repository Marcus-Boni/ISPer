# 0005 — Diarização depois da reunião, sobre o áudio contínuo

- **Status:** aceita (revista em 18/09/2026)
- **Data:** 01/09/2026

## Contexto

Separar "Eu" de "Participantes" sai de graça (são dois canais). Saber *qual*
participante falou exige diarização: segmentar a fala, extrair um
*embedding* de cada trecho e agrupar. O ISPer usa o sherpa-onnx
(`isper-diarize`, via `sherpa-rs`), com pyannote segmentation 3.0 e
3D-Speaker ERes2Net, ~45 MB, 100% local.

Duas medições pesaram:

- **É lenta na CPU.** O `sherpa-rs` 0.6 fixa `num_threads: 1` e não expõe o
  parâmetro: 30 min de áudio levaram 12,8 min, com 703 MB de pico. Rodar isso
  ao vivo, em paralelo ao Whisper, não cabe.
- **O agrupamento cresce com a duração, não com as pessoas** (medido em
  18/09). A ligação completa sobre distância de cosseno criava 7 grupos aos
  3 min e 34 aos 19 min na mesma reunião de 3 falantes — extrapolando para
  2 h, ~200. Era a causa do "Participante 255" (o `u8` saturando era só o
  sintoma).

## Decisão

A diarização roda **depois** que a reunião é salva, **em segundo plano**, e
nunca atrasa a transcrição: a reunião abre na hora com "Participantes", e
"Participante 1, 2…" aparece quando a identificação termina.

Desde a revisão de 18/09, ela roda sobre o **áudio contínuo no relógio da
reunião** (não mais sobre os blocos não silenciosos emendados), dentro do
passe final ([0006](0006-dois-modos-ao-vivo-e-passe-final.md)), com:

1. **número de participantes** informável em Configurações → Reuniões — o
   sherpa corta o dendrograma em exatamente N grupos; foi a única coisa
   estável na reunião longa;
2. **limiar 0,5**, o padrão do próprio sherpa-onnx (o 0,3 anterior vinha de
   uma fixture sintética de duas vozes);
3. **absorção de grupos fracos** (menos que `max(6 s, 2% da fala)` ou menos
   de 2 turnos) pelo grupo forte mais próximo no tempo;
4. **guarda:** acima de 12 grupos o resultado não é publicado — fica
   "Participantes", com aviso no log, em vez de inventar dezenas de pessoas.

## Consequências

- A reunião nunca espera a diarização; fechar o app no meio cancela só a
  identificação daquela reunião.
- A diarização é o gargalo do passe final (0,29× tempo real, mais que o dobro
  do ASR). Subir as threads exige PR no `sherpa-rs` ou chamar o sherpa-onnx
  direto.
- Fala sobreposta vira um falante só, e o modelo de embedding foi treinado em
  chinês — trocá-lo sem medir em voz real seria adivinhar.
- Reconhecer a mesma voz entre reuniões diferentes continua fora.

## Alternativas consideradas

- **Diarização em tempo real.** Não cabe no tempo real com uma thread, e
  disputaria a máquina com o Whisper durante a reunião.
- **Serviço de nuvem.** Mandaria áudio para fora — contra
  [0002](0002-rust-tauri-whisper-cpp.md) e [0003](0003-llm-na-nuvem-so-texto.md).
- **Continuar sobre o áudio concatenado.** Emendar blocos distorcia as
  trocas de falante e os horários, e o agrupamento degenerava com a duração.

## Onde vive

`crates/isper-diarize/` (inclusive `postprocess.rs`),
`apps/isper-app/src-tauri/src/final_pass.rs`,
[`docs/transcription-pipeline.md`](../transcription-pipeline.md) (seção 6,
com as tabelas medidas), `isper-cli diarize <wav> --speakers N --threshold T`.
