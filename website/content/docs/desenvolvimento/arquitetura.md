---
title: "Arquitetura"
description: "Como o ISPer é dividido: o motor em Rust, o app Tauri, os fluxos de ditado e de reunião, onde ficam os dados e as decisões registradas."
section: "Desenvolvimento"
order: 91
---

# Arquitetura

O ISPer é um workspace Rust com um app Tauri 2. Tudo o que envolve áudio roda no seu computador. A rede só entra em três casos: baixar modelos, checar se há versão nova e, se você configurar, mandar **texto** a um provedor de IA.

## As peças

| Peça | O que faz |
|---|---|
| `crates/isper-core` | O motor, sem interface: captura do microfone e do áudio do sistema, reamostragem, Whisper, VAD, reunião, SQLite e exportação |
| `crates/isper-models` | Catálogo e download dos modelos, com SHA-256 conferido |
| `crates/isper-diarize` | Quem falou o quê, com sherpa-onnx (segmentação pyannote e embeddings 3D-Speaker) |
| `crates/isper-llm` | Provedores de IA (Groq, Gemini, Claude), resumo pós-reunião e Copilot |
| `crates/isper-cli` | Laboratório de terminal: gravar, transcrever, medir (`bench`) e reconstruir a Biblioteca (`import`) |
| `apps/isper-app` | O app: `src-tauri` em Rust, com um módulo por assunto, e `ui` em HTML, CSS e JavaScript |

O motor fica separado da interface para ser testado sozinho, com fontes de áudio falsas, relógio injetado e provedor de IA falso. O mesmo código atende o app e o `isper-cli`.

## Ditado

1. O atalho global começa a captura do microfone (cpal).
2. Quando você solta o atalho, ou depois da pausa no modo mãos-livres, o áudio é convertido para 16 kHz mono e vai ao Whisper (whisper.cpp, via `whisper-rs`).
3. O texto passa pelo filtro de alucinações, pelos comandos de voz e pelo dicionário pessoal.
4. O ISPer cola com Ctrl+V e devolve o que estava no clipboard. O ditado entra no histórico.

## Reunião

1. Uma thread sonda as sessões de áudio do Windows. Quando o Teams entra em chamada, o ISPer pergunta se deve gravar, ou começa sozinho no modo automático.
2. São dois canais: o microfone ("Eu") e o que os participantes falam. O segundo vem por loopback do WASAPI, só do Teams quando o Windows permite, com keepalive e watchdog para não travar.
3. **Ao vivo**, as legendas chegam em poucos segundos. Os dois canais vão a disco contínuos, no relógio da reunião.
4. Ao encerrar, o **passe final** roda em segundo plano (VAD Silero e beam search), e a diarização separa "Participante 1, 2…" por palavra. Ele só substitui a transcrição ao vivo quando termina inteiro.
5. A reunião vira um `.md` e linhas no SQLite. Com IA configurada, sai o resumo com decisões e tarefas.

O `.md` é gravado antes do banco. Se o índice se perder, `isper-cli import` reconstrói a Biblioteca a partir dos arquivos.

## Onde ficam os dados

| O quê | Onde |
|---|---|
| Configurações e banco (`isper.db`) | `%APPDATA%\ISPer` |
| Modelos e logs | `%LOCALAPPDATA%\com.isper.desktop` |
| Reuniões em Markdown | `Documentos\ISPer` |
| Chaves de API | Gerenciador de Credenciais do Windows |

## Interface

A interface é HTML, CSS e JavaScript puros, sem bundler, embutidos no executável. As fontes são locais, e a política de segurança de conteúdo (CSP) só permite o próprio app e a ponte com o Rust (IPC). Os dicionários de idioma e o tema chegam a cada janela por um script de inicialização, antes da primeira pintura.

## Releases e atualização

Uma tag de versão dispara o workflow de release no GitHub Actions. Ele confere a versão contra o `CHANGELOG`, roda o CI inteiro e compila duas variantes: **GPU** (CUDA) e **CPU**. A release sai com `SHA256SUMS.txt`, SBOM e a assinatura minisign de cada instalador. O atualizador do app confere essa assinatura com a chave pública embutida antes de instalar.

## Decisões registradas

Cada decisão grande tem um ADR em [`docs/adr/`](https://github.com/Marcus-Boni/ISPer/tree/main/docs/adr), com o contexto, o que foi escolhido e o que ficou de fora. Por exemplo:

- Rust, Tauri 2 e whisper.cpp, tudo local;
- IA na nuvem, só com o texto;
- os dois modos de transcrição, ao vivo e passe final;
- nada some sem o usuário pedir.

O pipeline de transcrição tem um documento próprio, [`docs/transcription-pipeline.md`](https://github.com/Marcus-Boni/ISPer/blob/main/docs/transcription-pipeline.md).

## Referência da API

Todo item público do `isper-core` é documentado, e o CI recusa um item novo sem documentação. Para abrir a referência:

```powershell
cargo doc -p isper-core --no-deps --open
```
