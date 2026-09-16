---
title: "Reuniões, áudio do sistema e LGPD"
description: "Como o ISPer captura microfone e participantes, salva reuniões e preserva privacidade."
section: "Reuniões e Sistema"
order: 40
---

# Reuniões, áudio do sistema e LGPD

O modo reunião grava duas fontes: o microfone como `Eu` e o áudio do sistema como `Participantes`. A transcrição ao vivo acontece em blocos e a reunião salva fica disponível na Biblioteca.

## Iniciar e encerrar

Você pode iniciar pela bandeja, pela tela Início ou pelo atalho global de reunião, que por padrão é `Ctrl + Alt + M`.

Ao encerrar, o ISPer salva:

- Markdown em `Documentos\ISPer\Reunioes\*.md`.
- Histórico e segmentos no SQLite local.
- Resumo, título e itens de ação quando um provider de IA estiver configurado.

## Captura do sistema

O ISPer usa loopback para capturar participantes. Em Configurações > Reuniões, a opção "Só o Microsoft Teams" usa loopback de processo para priorizar o Teams quando possível.

> [!NOTE]
> Sem bot, sem API do Teams e sem olhar janelas: a detecção de chamada usa sessões de áudio do Windows.

## Privacidade e consentimento

O áudio transcrito localmente não sai do computador. Mesmo assim, avise os participantes antes de gravar e transcrever uma reunião, especialmente em contextos cobertos pela LGPD ou por políticas internas.

## Arquivos temporários

Durante reuniões longas, o áudio dos participantes pode ir para arquivos temporários em disco para manter a memória estável. Eles são limpos após o processamento e a diarização.
