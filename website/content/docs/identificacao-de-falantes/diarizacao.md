---
title: "Como a diarização separa as vozes"
description: "Configure diarização local para transformar Participantes em Participante 1, Participante 2 e outros rótulos."
section: "Identificação de Falantes"
order: 50
---

# Como a diarização separa as vozes

A diarização separa vozes diferentes dentro da faixa de participantes. O ISPer usa modelos locais via `sherpa-onnx`, com pyannote e 3D-Speaker.

## Instalar os modelos

Em Configurações > Reuniões, use "Baixar modelos" para instalar os modelos de diarização. O pacote é pequeno perto dos modelos Whisper e roda localmente.

Pela CLI:

```powershell
cargo run --release -p isper-cli -- models download-diarize
```

## Como o fluxo aparece

1. A reunião é salva e aberta imediatamente com o rótulo `Participantes`.
2. A identificação roda em segundo plano.
3. Ao terminar, os segmentos viram `Participante 1`, `Participante 2` e assim por diante.
4. O banco, a Biblioteca e o Markdown são atualizados.

## Ajustar separação de vozes

Dois erros comuns o ISPer já corrige sozinho. Um trecho solto, que o agrupamento não soube encaixar, vai para quem estava falando ao redor. E dois "participantes" com a mesma voz, uma pessoa partida em duas (comum em gravação curta de uma pessoa só), viram um. Na dúvida, ele não junta: duas pessoas de voz parecida continuam separadas.

Se ainda juntar ou separar demais, o ajuste de maior efeito é informar quantas pessoas participam, em Configurações > Reuniões. Para calibrar sem recompilar:

```powershell
$env:ISPER_DIARIZE_THRESHOLD = "0.6"
isper-cli diarize fixtures/duas-vozes-16k.wav
```

O limiar do agrupamento é uma distância: valores menores criam mais falantes distintos. O padrão é `0.5`, o do próprio sherpa-onnx. A saída também mostra a semelhança entre as vozes de cada par de falantes; a partir de `0.6`, os dois viram um. `--same-voice` (ou `ISPER_DIARIZE_SAME_VOICE`) muda esse limite, e `0` desliga a junção.

## Renomear participantes

Na Biblioteca, clique em `Participante 1` e digite o nome real. A mudança vale para toda a reunião e mantém a cor daquele falante.
