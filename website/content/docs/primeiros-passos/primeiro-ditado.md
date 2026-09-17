---
title: "Primeiro ditado"
description: "Use o atalho global do ISPer para ditar em qualquer aplicativo do Windows."
section: "Primeiros Passos"
order: 20
---

# Primeiro ditado

O ditado do ISPer funciona como uma camada local de fala para texto. Você fala, o Whisper transcreve no computador e o texto é colado no aplicativo que estava em foco.

## Atalho global

O app registra o primeiro atalho livre entre as combinações suportadas. A dica no menu da bandeja mostra qual ficou ativo. As combinações comuns são:

- `Ctrl + Alt + Espaço`
- `Ctrl + Shift + Espaço`
- `Ctrl + Alt + D`
- `Ctrl + Alt + I`

Você pode trocar o atalho em Configurações > Ditado.

## Dois modos de uso

| Modo | Como usar | Resultado |
|---|---|---|
| Push-to-talk | Segure o atalho, fale e solte | O texto é colado ao soltar. |
| Mãos-livres | Toque uma vez, fale e toque de novo ou pare | O ISPer encerra após silêncio curto e cola sozinho. |

## O que acontece com o texto

O ISPer cola o resultado com `Ctrl + V` e restaura o clipboard anterior em seguida. O histórico fica no banco local em `%APPDATA%\ISPer\isper.db`.

## Comandos de voz

Comandos como `nova linha`, `novo parágrafo`, `ponto final`, `vírgula`, `abre parênteses`, `fecha aspas` e `apagar isso` são tratados localmente. Eles ajustam pontuação e formatação sem depender de API.

> [!TIP/Dica]
> Use o dicionário pessoal para termos como nomes de clientes, siglas e produtos. Ele vira contexto para o Whisper e também ajuda a corrigir grafias próximas.
