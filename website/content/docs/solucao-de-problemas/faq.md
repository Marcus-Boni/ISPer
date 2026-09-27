---
title: "Solução de problemas e FAQ"
description: "Correções rápidas para CUDA, microfone, atalhos globais, modelos e atualizações."
section: "Solução de Problemas"
order: 80
---

# Solução de problemas e FAQ

Esta página reúne problemas comuns ao instalar, ditar, gravar reuniões ou compilar o ISPer a partir do código.

## O atalho não funciona

Outro aplicativo pode ter registrado a mesma combinação. Abra Configurações > Ditado e escolha outra opção, ou use "Gravar atalho" e pressione uma combinação livre.

## O app abriu sem modelo

Instale um modelo em Configurações > Modelos Whisper. Pela CLI:

```powershell
cargo run --release -p isper-cli -- models download ggml-small.bin
```

## CUDA não foi encontrado no build

Confirme que o CUDA Toolkit está instalado e que o shell atual recebeu as variáveis `CUDA_PATH` e `CUDA_PATH_V13_3`. Shells abertos antes da instalação podem precisar ser reabertos.

## O executável não substitui no rebuild

Feche o ISPer pelo tray antes de compilar. O Windows bloqueia a substituição de `isper-app.exe` quando o app está aberto.

## A reunião capturou áudio errado

Verifique a fonte em Configurações > Reuniões. Se usar Teams, teste a opção "Só o Microsoft Teams". Evite rodar testes de loopback enquanto há música ou uma reunião real tocando.

## A diarização separou mal os falantes

Informe quantas pessoas participam, em Configurações > Reuniões: é o ajuste de maior efeito. Uma pessoa partida em duas também se conserta na Biblioteca, renomeando uma delas com o nome da outra. Para calibrar o agrupamento, veja [Como a diarização separa as vozes](/docs/identificacao-de-falantes/diarizacao/).

## A atualização não instala durante reunião

Isso é esperado. O atualizador recusa instalar enquanto há reunião ativa para evitar perda de gravação.
