# 0002 — Rust + Tauri 2 + whisper.cpp, tudo local

- **Status:** aceita
- **Data:** 25/08/2026

## Contexto

O ISPer começou como um ditado por voz para Windows — segurar um atalho,
falar e ter o texto colado no app focado — que depois viraria notetaker de
reuniões. Três exigências vieram do usuário desde o início: **custo zero**,
**nada de áudio saindo da máquina** e o objetivo pessoal de **aprender Rust**
e entender ASR por dentro. A máquina de referência tem uma RTX 4050 (6 GB de
VRAM), então a transcrição podia ser local e rápida.

O trabalho pesado é de sistemas: captura de áudio sem cortes, buffers entre
threads, inferência pesada, latência. A interface é secundária, mas precisa
ser bonita e responsiva.

## Decisão

- **Núcleo em Rust**, num workspace cargo: `isper-core` (captura, VAD,
  Whisper, reunião, banco — sem UI), `isper-cli` (laboratório), `isper-models`,
  `isper-llm`, `isper-diarize`.
- **Interface em Tauri 2**, sobre o WebView2 que já vem no Windows 11.
- **ASR com whisper.cpp** via `whisper-rs`, com a feature `cuda` na variante
  GPU e o modelo `large-v3-turbo` q5_0 como padrão de produção.

O núcleo não conhece a interface: comunica-se com ela por canais e eventos
do Tauri, nunca por estado global solto. Erros com `thiserror` nas libs e
`anyhow` no app; logs com `tracing`.

## Consequências

- Medido na RTX 4050: 10,4 s de áudio transcritos em 0,6 s (16× tempo real).
  O binário do app, sem os modelos e as DLLs do CUDA, fica perto de 10 MB.
- A curva de aprendizado é íngreme — o que era justamente o objetivo — e
  algumas armadilhas custaram caro: o crate `cmake` engole o `/O2` no MSVC
  (whisper.cpp ~13× mais lento sem forçar as flags em `.cargo/config.toml`),
  e construir uma janela do WebView2 na thread principal a partir de um
  comando congela o loop de eventos (`views::open_or_focus`).
- O CUDA pesa: a variante GPU leva ~400 MB de DLLs redistribuíveis e o build
  no CI instala o toolkit a cada release (ver [0008](0008-release-em-runners-do-github.md)).
- Só Windows por enquanto: captura por WASAPI, colagem por Ctrl+V simulado,
  Credential Manager. Outro sistema operacional pede outra camada de captura
  e de integração, não um recompilar.

## Alternativas consideradas

| Opção | Por que ficou de fora |
|---|---|
| Electron + JS/TS | 150 MB ou mais e alto consumo de RAM; nada a aprender em áudio e sistemas |
| Python (`faster-whisper`) | ótimo para experimentos, mas distribuir o `.exe` é doloroso, a UI desktop fica menos polida e o GIL atrapalha áudio em tempo real |
| C# / WinUI (`whisper.net`) | nativo e viável — a alternativa honrosa —, mas não era o objetivo de aprendizado |
| Elixir | ecossistema de desktop e áudio nativo fraco no Windows |
| API de transcrição na nuvem | quebra o "nada de áudio sai da máquina" e o custo zero |

## Onde vive

`Cargo.toml` (workspace), `crates/`, `apps/isper-app/src-tauri/`,
`.cargo/config.toml` (flags do whisper.cpp). Seção 1 do `ROADMAP.md`.
