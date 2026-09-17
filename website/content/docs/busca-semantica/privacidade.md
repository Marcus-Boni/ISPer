---
title: "Privacidade da busca semântica"
description: "Escolha entre embeddings locais e remotos sabendo exatamente qual texto é processado."
section: "Busca Semântica"
order: 72
---

# Privacidade da busca semântica

A busca semântica transforma trechos em vetores numéricos e guarda esses vetores no SQLite ao lado do texto.

## Modo local

Com Ollama ou outro endpoint local compatível com OpenAI, os trechos são processados na própria máquina. Confirme que a URL aponta para `localhost` e que nenhum proxy corporativo redireciona essa conexão.

## Modo remoto

Com Gemini, OpenAI, Mistral ou outro endpoint remoto, os trechos necessários à indexação são enviados ao serviço configurado. Consulte as políticas e limites do provedor antes de indexar reuniões confidenciais.

## Troca de modelo

Ao trocar provedor, modelo ou dimensão, reconstrua o índice. O ISPer evita comparar vetores incompatíveis, mas a decisão sobre qual provedor pode receber o texto continua sendo sua.
