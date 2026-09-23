---
title: "Instalação"
description: "Prepare o ISPer no Windows, escolha a variante certa e confirme que o app abriu corretamente."
section: "Primeiros Passos"
order: 10
---

# Instalação

O ISPer é um aplicativo desktop para Windows. Ele roda a transcrição no próprio computador com `whisper.cpp`, guarda histórico local em SQLite e usa provedores externos apenas quando você ativa recursos de inteligência por API.

> [!NOTE/Observação]
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

## Versão portátil (zip)

Desde a 0.20.0, cada release traz também um zip por variante: a pasta do ISPer inteira, sem instalador. Descompacte onde quiser e abra o `isper-app.exe`. Não precisa de administrador.

- Os dados **não** ficam na pasta do zip: configurações e banco vão para `%APPDATA%\ISPer`, modelos e logs para `%LOCALAPPDATA%\com.isper.desktop` e reuniões para `Documentos\ISPer`, as mesmas pastas da versão instalada.
- Quando sai versão nova, o ISPer avisa e abre a página de Download em vez de instalar por cima. Baixe o zip novo, feche o app e substitua os arquivos da pasta.
- O arquivo `portable.txt` dentro da pasta é o que diz ao ISPer que a cópia é portátil. Sem ele, a atualização tentaria instalar uma segunda cópia.
- Em Configurações → Sistema → Diagnóstico, a linha *Instalação* mostra se o ISPer em uso é a versão instalada ou a portátil.

## Primeira abertura

Numa instalação nova, o ISPer abre a [primeira configuração](/docs/primeiros-passos/primeira-configuracao/): idioma e aparência, microfone, modelo Whisper, atalho e IA opcional. Depois ele fica na bandeja do sistema, e a tela Início mostra o que ainda falta configurar.

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
