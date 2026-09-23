# 0011 — O que é opcional vira configuração, não feature flag de compilação

- **Status:** aceita
- **Data:** 23/09/2026

## Contexto

A fase 7.6 do ROADMAP, escrita em 10/09, pedia "feature flags para o
experimental (legendas ao vivo, comandos de voz)". Naquela semana os dois
recursos acabavam de nascer. Em 23/09 o quadro era outro:

- os **comandos de voz** estão ligados por padrão desde a 0.11 e já têm
  interruptor (Configurações → Ditado, campo `voice_commands`);
- as **legendas ao vivo** foram refeitas na 0.17 (blocos de 6 s e texto
  provisório em ~2–3 s) e fazem parte de como a reunião é acompanhada; o modo
  de legenda do indicador tem botão próprio (`overlay_captions`).

O projeto também já tem o custo de **duas variantes** de instalador (GPU e
CPU, ver [0008](0008-release-em-runners-do-github.md)), e o e2e roda sobre o
binário de verdade. Cada feature do cargo que mudasse o produto dobraria o
que é preciso compilar, testar e publicar.

## Decisão

- **Features do cargo só para diferenças de build**: hoje, só `cuda`.
- **Recurso opcional, arriscado ou em teste é uma configuração** em
  `config.toml`, com padrão seguro, validada por `AppConfig::normalize` e com
  controle na interface: `voice_commands`, `overlay_captions`, `polish`,
  `live_insights`, `final_pass`, `auto_update_check`. Um recurso
  experimental novo entra desligado, em Configurações → Avançado quando for
  técnico.
- **Variáveis de ambiente `ISPER_*` são ferramentas de desenvolvimento e de
  teste** (ex.: `ISPER_UPDATE_ENDPOINT`, `ISPER_UPDATE_DRY_RUN`,
  `ISPER_DIARIZE_THRESHOLD`, `ISPER_MODEL`), não interruptores para quem usa.

## Consequências

- Um binário por variante: o que o e2e testa é o que o usuário instala.
- Ligar ou desligar um recurso não pede reinstalar; e voltar atrás também não.
- O código de um recurso desligado continua no binário — alguns KB; aceitável.
- Os dois caminhos (ligado e desligado) precisam de teste por configuração,
  não por build: é o que o e2e já faz com `set_ui_lang`, `set_ui_theme` e o
  resto.

## Alternativas consideradas

- **Feature do cargo por recurso.** Cada uma dobra a matriz de build; um
  recurso desligado em tempo de compilação não pode ser ligado pelo usuário
  sem outro instalador.
- **Flags remotas (ligar à distância).** Exigiria o app consultar um servidor
  — contra a promessa de não falar com a rede sem o usuário pedir.

## Onde vive

`apps/isper-app/src-tauri/src/config.rs` (campos e `normalize`),
`apps/isper-app/src-tauri/Cargo.toml` e `crates/isper-core/Cargo.toml`
(`[features]`).
