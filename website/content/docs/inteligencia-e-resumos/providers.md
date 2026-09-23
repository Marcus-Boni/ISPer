---
title: "Escolher um provedor de resumos"
description: "Conecte Groq, Gemini, Claude ou outro provedor compatível para gerar resumos e insights."
section: "Inteligência e Resumos"
order: 60
---

# Escolher um provedor de resumos

O ISPer transcreve localmente. A camada de inteligência é opcional e trabalha sobre texto: resumo, decisões, pontos principais, action items, título automático e insights ao vivo.

## Provedores disponíveis

O README do projeto documenta os provedores `groq`, `gemini` e `claude` para resumo. A chave fica no Credential Manager do Windows, não em arquivo de configuração do projeto.

```powershell
cargo run --release -p isper-cli -- llm use groq
cargo run --release -p isper-cli -- llm set-key groq
cargo run --release -p isper-cli -- llm test
```

## O que é enviado

Somente o texto do transcript ou do ditado polido é enviado ao provedor configurado. O áudio permanece local.

> [!WARNING/Atenção]
> Antes de usar provedores de nuvem, confirme se a política da sua organização permite enviar transcrições para esse serviço.

## Sem provedor

Sem provedor configurado, ditado, reuniões, Biblioteca, exportações e transcrição continuam funcionando. Apenas os recursos de resumo, polimento e insights ficam indisponíveis.

## Insights ao vivo

Quando ativados, os insights analisam janelas recentes da transcrição durante a reunião e consolidam pendências, promessas, decisões e perguntas abertas.

## Copilot

O mesmo provedor alimenta o Copilot, que durante a reunião transforma a conversa em cards de decisão, ação, risco e pergunta. Diferente dos insights, ele analisa toda reunião gravada quando há um provedor configurado — veja [Memória, privacidade e custo do Copilot](/docs/copilot/memoria-privacidade-e-custo/).
