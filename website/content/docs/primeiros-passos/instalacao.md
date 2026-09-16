---
title: "Instalação"
description: "Prepare o ISPer no Windows, escolha a variante certa e confirme que o app abriu corretamente."
section: "Primeiros Passos"
order: 10
---

# Instalação

O ISPer é um aplicativo desktop para Windows. Ele roda a transcrição no próprio computador com `whisper.cpp`, guarda histórico local em SQLite e usa provedores externos apenas quando você ativa recursos de inteligência por API.

> [!NOTE]
> O áudio das reuniões e ditados não precisa sair do computador para ser transcrito. Recursos como resumo por IA ou embeddings em nuvem enviam texto ao provedor escolhido apenas quando configurados por você.

## Antes de instalar

- Windows x64 com WebView2.
- CPU com AVX2 para a variante CPU.
- GPU NVIDIA com CUDA para a variante GPU.
- Espaço livre para modelos Whisper em `%LOCALAPPDATA%\com.isper.desktop\models`.

## Baixar o instalador

Na página de Download, escolha a variante conforme o seu hardware:

| Variante | Quando usar | Observação |
|---|---|---|
| GPU CUDA | PCs com GPU NVIDIA compatível | Inclui DLLs do CUDA e tem pacote maior. |
| CPU | Qualquer PC x64 com AVX2 | Alternativa segura quando não há NVIDIA. |

Depois do download, confira o `SHA256SUMS.txt` publicado na release antes de instalar em ambientes controlados.

## Primeira abertura

Ao abrir, o ISPer fica na bandeja do sistema e mostra a tela Início. Sem modelo Whisper instalado, ele direciona você para Configurações > Modelos Whisper.

## Rodar a partir do código

Para desenvolvimento, use Rust e as ferramentas de build descritas no README do repositório:

```powershell
cargo build --release
cargo run --release -p isper-app
```

Com GPU NVIDIA, o build padrão usa CUDA. Sem CUDA, compile sem features padrão:

```powershell
cargo build --release -p isper-app --no-default-features
```

## Próximo passo

Instale um modelo Whisper e faça o primeiro ditado.
