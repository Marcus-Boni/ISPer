---
title: "Microsoft Teams"
description: "Capture uma chamada do Teams pelo áudio do Windows, sem bot ou plugin na reunião."
section: "Reuniões e Sistema"
order: 42
---

# Microsoft Teams

O ISPer captura a reunião no próprio Windows. Ele não adiciona um participante à chamada, não usa a API do Teams e não precisa instalar um plugin no cliente.

## Captura recomendada

Em **Configurações > Reuniões**, escolha **Só o Microsoft Teams**. O ISPer usa o loopback do processo para priorizar o áudio do Teams e ignorar músicas e notificações de outros aplicativos.

Se o Teams não estiver aberto, o aplicativo avisa e usa o áudio geral do sistema como alternativa.

## Detecção de chamada

O ISPer observa as sessões de áudio do Windows. Você pode escolher entre receber um aviso, gravar automaticamente ou não detectar chamadas.

> [!NOTE]
> A detecção usa o estado de áudio local. Ela não lê a lista de participantes, o chat ou o conteúdo da reunião.

## Fones e microfone

- Selecione no ISPer o mesmo dispositivo de saída usado pelo Teams.
- O canal **Eu** vem do microfone configurado no ISPer.
- Use fones para reduzir vazamento do alto-falante no microfone.
- Faça uma gravação curta antes de uma reunião importante.

Avise os participantes sobre a gravação conforme as regras aplicáveis à sua organização e à reunião.
