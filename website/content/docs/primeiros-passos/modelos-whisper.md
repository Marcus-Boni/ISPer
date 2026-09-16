---
title: "Modelos Whisper"
description: "Entenda onde os modelos ficam, como baixá-los e quando escolher Tiny, Small ou Large v3 Turbo."
section: "Primeiros Passos"
order: 30
---

# Modelos Whisper

O ISPer usa modelos `ggml` compatíveis com `whisper.cpp`. O gerenciador de modelos baixa arquivos com progresso e verifica o SHA-256 contra o catálogo publicado.

## Onde os modelos ficam

No app instalado, os modelos ficam em:

```text
%LOCALAPPDATA%\com.isper.desktop\models
```

Durante desenvolvimento, arquivos em `models/` na raiz do repositório também são reconhecidos.

## Baixar pelo app

Abra Configurações > Modelos Whisper e escolha o modelo. Sem nenhum modelo instalado, o ISPer abre essa tela automaticamente.

## Baixar pela CLI

```powershell
cargo run --release -p isper-cli -- models list
cargo run --release -p isper-cli -- models download ggml-large-v3-turbo-q5_0.bin
```

## Como escolher

| Modelo | Melhor para | Custo local |
|---|---|---|
| Tiny / Base | Testes rápidos e máquinas simples | Menor uso de CPU e memória. |
| Small | Ditado geral em português | Bom equilíbrio para CPU. |
| Large v3 Turbo Q5 | Reuniões e qualidade maior | Recomendado com GPU NVIDIA. |

> [!WARNING]
> Um modelo maior não corrige microfone ruim, ruído constante ou fala distante. Em reuniões, configure a captura do sistema e o microfone com cuidado.
