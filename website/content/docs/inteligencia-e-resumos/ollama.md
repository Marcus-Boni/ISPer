---
title: "Ollama local"
description: "Use Ollama para embeddings locais e entenda o limite atual para resumos."
section: "Inteligência e Resumos"
order: 62
---

# Ollama local

O checkout atual usa Ollama como endpoint compatível com OpenAI para **embeddings da busca semântica**. O provedor de resumos ainda é Groq, Gemini ou Claude.

## Preparar os embeddings

Instale o Ollama e baixe um modelo compatível:

```powershell
ollama pull nomic-embed-text
```

No ISPer, escolha o provedor compatível com OpenAI, use a base `http://localhost:11434/v1` e deixe a chave vazia.

## Reindexar o histórico

Depois de trocar o modelo de embeddings, use **Indexar tudo**. Vetores gerados por modelos diferentes não são comparados entre si.

> [!WARNING/Atenção]
> Esta configuração mantém os embeddings locais. Ela não transforma o Ollama em provedor de resumo na versão documentada.
