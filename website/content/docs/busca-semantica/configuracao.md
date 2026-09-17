---
title: "Busca semântica"
description: "Indexe reuniões e ditados por sentido usando embeddings locais ou provedores compatíveis."
section: "Busca Semântica"
order: 70
---

# Busca semântica

A busca semântica transforma reuniões e ditados em vetores guardados no SQLite local. Ela encontra trechos pelo sentido, mesmo quando a palavra exata não aparece.

## Provedores

O ISPer aceita Gemini para embeddings e endpoints compatíveis com OpenAI. Para uso local, o README recomenda Ollama com modelos como `nomic-embed-text` ou `bge-m3`.

```powershell
ollama pull nomic-embed-text
```

Base local comum:

```text
http://localhost:11434/v1
```

## Indexação

Cada reunião salva e cada ditado colado pode ser indexado em segundo plano. O botão "Indexar tudo" cobre o histórico anterior e refaz o índice quando o modelo muda.

## Limites importantes

- Vetores de modelos diferentes não são comparáveis.
- Trocar o modelo exige reindexação.
- Com Ollama local, texto e vetores permanecem na máquina.
- Com provedor de nuvem, o texto necessário para gerar embeddings é enviado ao serviço.

## Uso na Biblioteca

Na Biblioteca, use o botão Semântica e pergunte naturalmente, por exemplo: `quando falamos do orçamento?`. O resultado abre a reunião no trecho relevante.
