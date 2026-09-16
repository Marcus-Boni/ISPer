---
title: "Inteligência e resumos"
description: "Conecte Groq, Gemini, Claude ou outro provider compatível para gerar resumos e insights."
section: "Inteligência e Resumos"
order: 60
---

# Inteligência e resumos

O ISPer transcreve localmente. A camada de inteligência é opcional e trabalha sobre texto: resumo, decisões, pontos principais, action items, título automático e insights ao vivo.

## Providers disponíveis

O README do projeto documenta providers `groq`, `gemini` e `claude` para resumo. A chave fica no Credential Manager do Windows, não em arquivo de configuração do projeto.

```powershell
cargo run --release -p isper-cli -- llm use groq
cargo run --release -p isper-cli -- llm set-key groq
cargo run --release -p isper-cli -- llm test
```

## O que é enviado

Somente o texto do transcript ou do ditado polido é enviado ao provider configurado. O áudio permanece local.

> [!WARNING]
> Antes de usar providers de nuvem, confirme se a política da sua organização permite enviar transcrições para esse serviço.

## Sem provider

Sem provider configurado, ditado, reuniões, Biblioteca, exportações e transcrição continuam funcionando. Apenas os recursos de resumo, polimento e insights ficam indisponíveis.

## Insights ao vivo

Quando ativados, os insights analisam janelas recentes da transcrição durante a reunião e consolidam pendências, promessas, decisões e perguntas abertas.
