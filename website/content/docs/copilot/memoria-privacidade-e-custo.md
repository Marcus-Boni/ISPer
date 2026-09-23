---
title: "Memória, privacidade e custo do Copilot"
description: "Como o Copilot lembra de reuniões passadas, o que sai da sua máquina e quanto ele consome do provedor de IA."
section: "Copilot"
order: 66
---

# Memória, privacidade e custo do Copilot

## Memória de reuniões passadas

Quando o assunto da conversa muda, o Copilot procura, no seu histórico, reuniões anteriores que falaram do mesmo tema. As que encontrar aparecem no topo do feed como cards de **Memória**, com o trecho, a data e um botão para abrir aquela reunião na Biblioteca.

- Aparecem no máximo três de cada vez, uma por reunião.
- **Dispensar** tira o card do feed; aquela reunião não volta durante a chamada atual.
- A reunião que está sendo gravada não entra na busca — ela só é indexada depois de salva.

A memória usa a mesma base da busca semântica. Para ela funcionar:

1. Configure a busca semântica em **Configurações → Inteligência** (veja [Configurar a busca semântica](/docs/busca-semantica/configuracao/)).
2. Clique em **Indexar tudo** para incluir as reuniões que você já tem.

Sem isso, o Copilot funciona normalmente, só não lembra de nada — o filtro **Memória** fica vazio.

> [!TIP/Dica]
> A memória procura pelo **resumo do assunto**, não pela transcrição inteira. Reuniões com pouca fala ou sobre temas muito diferentes tendem a não aparecer, e isso é intencional: um card de memória errado no meio de uma reunião atrapalha mais do que ajuda.

## O que sai da sua máquina

O **áudio nunca sai**. A transcrição acontece localmente, e o que o Copilot envia é texto:

| O quê | Para onde | Quanto |
|---|---|---|
| Leitura da conversa (os cards) | Provedor de IA | Os últimos 20 minutos de transcrição |
| Pergunte à Reunião e Notas | Provedor de IA | Os últimos 30 minutos, só quando você pede |
| Memória | Provedor de embeddings | Uma frase com o assunto do momento |

Com o **Ollama** como provedor de embeddings, a memória não envia nada para fora. O provedor de IA dos cards é o mesmo dos resumos.

> [!WARNING/Atenção]
> Antes de usar o Copilot numa reunião de trabalho, confirme se a política da sua organização permite enviar transcrições para o provedor configurado.

## Custo

**Com um provedor de IA configurado, o Copilot analisa toda reunião gravada — com a janela dele aberta ou não.** É o que permite abrir o Copilot no meio da reunião e já encontrar os cards prontos.

Na prática, numa conversa contínua:

- a primeira leitura acontece cerca de 20 segundos depois do início;
- depois, o Copilot relê a conversa a cada 45 segundos, enquanto houver fala nova;
- as frases que marcam acordos, tarefas e objeções antecipam leituras, com um intervalo mínimo entre elas.

Isso dá algo perto de **80 leituras por hora de reunião**, cada uma com até 20 minutos de transcrição. Perguntas, enriquecimento de notas e a busca da memória são chamadas extras, feitas só quando acontecem.

- **Com provedores gratuitos** (Groq, Gemini), reuniões longas podem esbarrar no limite de uso da conta. Quando isso acontece, a faixa no topo da janela mostra o erro e oferece **Tentar de novo**.
- **Com provedores pagos**, o consumo é proporcional ao tempo de reunião gravada.

Os **Insights ao vivo** do Início são outro recurso, com comportamento diferente: eles só fazem leituras periódicas se você ativá-los em **Configurações → Inteligência**. O Copilot hoje não tem uma opção equivalente — para ele não consumir o provedor, é preciso deixar o provedor de IA sem configuração, o que desliga também os resumos.
