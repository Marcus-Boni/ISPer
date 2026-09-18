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

Se o ISPer juntar ou separar demais os falantes, calibre o threshold sem recompilar:

```powershell
$env:ISPER_DIARIZE_THRESHOLD = "0.2"
isper-cli diarize fixtures/duas-vozes-16k.wav
```

Valores menores tendem a criar mais falantes distintos. O padrão documentado no projeto é `0.3`.

## Renomear participantes

Na Biblioteca, clique em `Participante 1` e digite o nome real. A mudança vale para toda a reunião e mantém a cor daquele falante.
