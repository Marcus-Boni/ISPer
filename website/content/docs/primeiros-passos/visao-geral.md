---
title: "Como o ISPer funciona"
description: "Conheça o fluxo local de áudio, transcrição, histórico, busca e inteligência opcional."
section: "Primeiros Passos"
order: 5
---

# Como o ISPer funciona

O ISPer fica na bandeja do Windows e transforma fala em texto sem depender de um serviço de transcrição remoto.

## Ditado

O atalho global inicia a captura do microfone. Ao terminar, o Whisper transcreve localmente e o texto é colado no aplicativo que estava em foco. O clipboard anterior é restaurado.

## Reuniões

O canal **Eu** vem do microfone. O canal **Participantes** vem do áudio do sistema ou do loopback específico do Teams. A diarização local separa as falas depois que a reunião é salva.

## Histórico e busca

Ditados e reuniões ficam no SQLite local e na Biblioteca. A busca textual não precisa de configuração. A busca semântica exige um modelo de embeddings local ou remoto.

## Inteligência opcional

Resumos e insights usam Groq, Gemini ou Claude somente quando você configura um provedor. Nesse fluxo, o texto necessário é enviado; a transcrição do áudio continua local.

Comece pelo guia de instalação e faça um ditado curto antes de configurar recursos adicionais.
