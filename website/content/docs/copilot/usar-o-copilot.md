---
title: "Usar o Copilot durante a reunião"
description: "Decisões, ações, riscos e perguntas extraídos da conversa enquanto ela acontece, com o que você confirmar indo para a ata."
section: "Copilot"
order: 64
---

# Usar o Copilot durante a reunião

O Copilot acompanha a reunião enquanto ela acontece. De um lado fica a fala transcrita; do outro, um feed de cards que a IA extrai da conversa — o que foi decidido, quem ficou com o quê, as objeções que ficaram sem resposta e as perguntas que valeria fazer antes de encerrar.

A transcrição continua 100% local. O Copilot trabalha sobre o **texto** da conversa, com o mesmo provedor de IA dos resumos.

> [!NOTE/Observação]
> O Copilot precisa de um provedor de IA configurado (veja [Escolher um provedor de resumos](/docs/inteligencia-e-resumos/providers/)). Sem ele, a janela abre e mostra a fala ao vivo, mas não gera cards.

## Abrir

Com uma reunião gravando, abra o Copilot por qualquer um destes caminhos:

- o atalho **`Ctrl+Alt+C`**;
- o botão do Copilot no indicador flutuante;
- o botão **Copilot HUD** no Início;
- a bandeja do sistema.

O atalho traz o Copilot para a frente; com ele já na frente, esconde a janela. O que estiver digitado no chat e nas notas continua lá. Dá para trocar a combinação em **Configurações → Reuniões**.

Abra o Copilot quando quiser acompanhar a reunião: ele só lê a conversa com a janela aberta (veja [Quando os cards aparecem](#quando-os-cards-aparecem)).

## Os cards

Cada card tem um tipo:

| Tipo | O que é |
|---|---|
| **Decisão** | Um acordo firmado na conversa. |
| **Ação** | Uma tarefa atribuída, com responsável e prazo. Quando o prazo não foi combinado, o card avisa: **prazo em aberto**. |
| **Risco** | Uma objeção ou dúvida levantada que ficou sem resposta. |
| **Pergunta** | Algo que valeria você perguntar antes de a reunião acabar. |

Entre os cards, os marcados como **urgente** vêm primeiro. O horário no canto de cada card é o instante da fala que o originou — clique nele para ir até o trecho na transcrição.

Os filtros no topo do feed separam os tipos.

## Quando os cards aparecem

O Copilot lê a conversa **enquanto a janela dele está aberta** — mesmo atrás de outra janela, como fica ao lado do Teams. Com ela aberta desde o começo, a primeira leitura acontece cerca de 20 segundos depois de a reunião começar; depois disso, o Copilot relê a conversa periodicamente enquanto houver fala nova.

Fechada, escondida pelo atalho ou minimizada, a janela não gera leituras. A fala ao vivo e a dinâmica da conversa continuam sendo registradas, e ao abrir o Copilot no meio da reunião ele lê em poucos segundos o que já foi dito (até os últimos 20 minutos).

Com a janela aberta, algumas frases antecipam a leitura, porque costumam marcar o momento em que algo importante acontece:

- acordos — *"então fica combinado"*, *"fechado"*, *"vamos seguir com"*;
- tarefas — *"eu envio"*, *"fica de"*, *"vai avaliar"*;
- objeções — *"discordo"*, *"me preocupa"*, *"tem um risco"*.

Essa detecção roda na sua máquina, sem rede. A faixa no topo da janela diz o que está acontecendo — por exemplo, *"Analisando — acordo detectado…"*.

## Confirmar, descartar e desfazer

- **Confirmar** marca o card como validado. É o que leva o card para a **ata**.
- **Descartar** tira o card do feed, e a IA para de sugeri-lo.
- **Desfazer** devolve o card para a fila.

Os descartados podem ser vistos de novo pelo link no fim do feed.

Ao encerrar a reunião, o que você confirmou entra no fim do Markdown da reunião numa seção própria, **Decisões e ações validadas no Copilot**, com responsáveis, prazos e a lista do que ficou sem prazo. Os cards que você não confirmou não entram.

## Pergunte à Reunião

A aba **Perguntar** responde sobre o que já foi dito — *"qual valor o cliente citou?"*, *"o que ficou pendente?"*, *"dê dois argumentos contra essa proposta"*. A resposta aparece enquanto é escrita. Se a conexão cair no meio, o que já chegou continua na tela.

`Enter` envia; `Shift+Enter` quebra a linha.

## Bloco de notas

Na aba **Notas**, anote solto durante a reunião — *"prazo da entrega"*, *"quem fica com o SLA"*. O botão **Enriquecer com a reunião** reescreve cada item com os números e as falas exatas da conversa, mantendo a sua ordem e marcando o que não chegou a ser discutido.

As notas salvam sozinhas enquanto você digita e voltam se você fechar e reabrir a janela durante a reunião. **Desfazer** volta ao texto de antes do enriquecimento.

As notas vão com a reunião. Ao encerrar, elas entram no fim da ata, na seção **Notas da reunião**, e aparecem na Biblioteca. Se você continuar escrevendo depois do fim, com a janela ainda mostrando a reunião que acabou, cada alteração também vai para ela — a aba mostra *salvo na reunião*. Quando outra reunião começa, o bloco fica vazio para ela.

> [!NOTE/Observação]
> As notas ficam na sua máquina. Elas não vão para o resumo da reunião: só saem para o provedor de IA quando você usa **Enriquecer com a reunião**.

## Dinâmica da conversa

O medidor no topo mostra a proporção de fala entre você e os participantes ao longo da reunião. Se você falar vários minutos seguidos sem ninguém interromper, aparece um aviso discreto de monólogo. A conta olha a sequência contínua, não o total.

## Ao lado do Teams

O Copilot cabe em 380 px de largura para ficar acoplado ao lado do Teams ou do Meet. Ao estreitar a janela, as duas colunas viram uma só; a disposição escolhida volta sozinha quando ela cresce. O botão de alfinete mantém o Copilot por cima das outras janelas.

## Atalhos dentro da janela

| Atalho | Ação |
|---|---|
| `/` | Buscar na transcrição |
| `Esc` | Limpar a busca |
| `Ctrl+1`, `Ctrl+2`, `Ctrl+3` | Ir para Decisões, Perguntar e Notas |

## Depois da reunião

Na **Biblioteca**, o detalhe da reunião ganha a seção **Decisões e alertas (Copilot)** com o que você confirmou e, abaixo dela, **Suas notas (Copilot)**. Clicar no horário de um card rola o transcript até a fala. Na lista de reuniões, um selo mostra quantas decisões cada uma tem.
