# 0006 — Dois modos de transcrição: ao vivo e passe final

- **Status:** aceita
- **Data:** 18/09/2026

## Contexto

Uma única transcrição servia a dois propósitos que puxam para lados opostos:

- **durante a reunião**, legenda, insights e o sinal de "está gravando"
  precisam chegar em segundos, com custo que caiba no tempo real;
- **depois**, a ata, as decisões, a busca e a IA precisam do texto mais
  correto possível, com o falante certo e horários por palavra.

Otimizar o mesmo pipeline para as duas coisas deixava ambas medíocres. A
auditoria de 18/09 também achou defeitos do modo único: o corte "na janela de
menor energia" caía no meio de palavras (`manual` → `anual`), e a diarização
sobre blocos emendados degenerava ([0005](0005-diarizacao-pos-hoc.md)).

## Decisão

Um motor, **dois modos**:

| | ao vivo | passe final |
|---|---|---|
| quando | durante a reunião | ao encerrar, em segundo plano |
| fatiamento | buffer cortado em silêncio de verdade | VAD Silero sobre o áudio inteiro |
| decoder | greedy, `best_of` 5 | beam search 5 |
| contexto | nenhum | fim da janela anterior |
| falante | "Eu" / "Participantes" | por palavra, via diarização |
| custo | cabe no tempo real | ~0,4× a duração da reunião |

Os dois canais vão a disco contínuos, no relógio da reunião, silêncio
incluído. A transcrição ao vivo **não é jogada fora**: é o que aparece na
hora e o que fica se o passe final falhar; o passe final só a substitui
quando termina inteiro, numa transação (`store::replace_segments`). Pode ser
desligado em Configurações → Reuniões → Avançado.

## Consequências

- Medido no corpus de regressão (190 s, 3 falantes): WER 8,45% → 5,28%,
  CER 6,85% → 4,16%, DER 28,1% → 20,6%, a 0,43× tempo real.
- Com as legendas provisórias da 0.17.0, o ao vivo passou a mostrar a fala em
  ~2–3 s, porque não precisa mais ser bom o bastante para virar ata.
- Custo de disco e memória cresce com a duração: ~0,46 GB em `%TEMP%` e
  ~0,9 GB de pico de RAM para 2 h (extrapolado; o maior corpus medido tem
  19 min).
- Há uma janela, depois de encerrar, em que a reunião mostra a transcrição do
  ao vivo; o Início e a Biblioteca sinalizam "refazendo a transcrição…".
- Tudo foi medido em voz sintética; o corpus certo é uma reunião real com um
  trecho corrigido à mão (`isper-cli bench --reference` já espera por ele).

## Alternativas consideradas

- **Só o ao vivo, mais caprichado.** Beam search e contexto não cabem no
  tempo real junto com a captura e a UI.
- **Só o final.** Sem legenda nem insights durante a reunião.
- **Substituir o ao vivo aos poucos.** Uma transcrição meio a meio seria pior
  do que qualquer uma das duas inteiras; a troca atômica é mais simples de
  provar.

## Onde vive

`crates/isper-core/src/{chunk,vad,profile,engine,context,pipeline,align}.rs`,
`apps/isper-app/src-tauri/src/final_pass.rs`,
[`docs/transcription-pipeline.md`](../transcription-pipeline.md),
`isper-cli bench` e `isper-cli compare`.
