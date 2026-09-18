---
title: "Privacidade e dados"
description: "Entenda onde áudio, texto, modelos, resumos e embeddings são processados e armazenados."
section: "Reuniões e Sistema"
order: 44
---

# Privacidade e dados

A transcrição, a diarização e o histórico do ISPer funcionam localmente. Recursos opcionais de inteligência podem enviar texto ao provedor escolhido por você.

## Fluxo dos dados

| Recurso | Processamento padrão | Pode usar rede |
|---|---|---|
| Transcrição Whisper | Local | Download inicial do modelo |
| Diarização | Local | Download inicial dos modelos |
| Resumo | Desligado até configurar | Envia texto ao Groq, Gemini ou Claude |
| Busca semântica | Conforme configuração | Pode usar Gemini ou endpoint compatível com OpenAI |
| Atualizações | GitHub Releases | Consulta desligável nas Configurações |

## Armazenamento

Configurações e banco ficam em `%APPDATA%\ISPer`. Modelos e logs ficam em `%LOCALAPPDATA%\com.isper.desktop`. Reuniões são exportadas para `Documentos\ISPer\Reunioes`.

Na versão 0.15.0, a retenção pode ser definida para 30, 90, 180 dias ou um ano. O padrão permanece **para sempre** até você escolher e confirmar um prazo.

## Antes de compartilhar um diagnóstico

Use **Exportar diagnóstico**. O pacote remove linhas com texto ditado e não inclui chaves, que ficam no Credential Manager do Windows. Mesmo assim, revise o ZIP antes de enviá-lo.
