# ISPer — Roadmap de Desenvolvimento

> Ditado por voz 100% local e gratuito (estilo Wispr Flow), evoluindo para notetaker de reuniões do Teams com insights de IA.
> Objetivo paralelo: **aprender Rust** e entender como transcrição de fala (ASR) funciona por dentro.

**Decidido em:** 25/08/2026 · **Plataforma:** Windows 11 · **Custo alvo:** R$ 0 nas Fases 0–4 · Fase 5 usa API de nuvem (pago por uso)

---

## 0. Hardware disponível (detectado)

| Recurso | Valor | Implicação |
|---|---|---|
| CPU | AMD Ryzen 7 7735HS (8c/16t) | Sobra para inferência CPU se precisar |
| RAM | 32 GB | Sem restrição de modelo |
| GPU | **NVIDIA RTX 4050 Laptop, 6 GB VRAM** | Build com **CUDA** → Whisper `large-v3-turbo` em tempo real |
| GPU 2 | Radeon iGPU | Fallback Vulkan se necessário |

Já instalados: Git 2.52, Node 24, ffmpeg 8.1. Faltando na chegada: **Rust (rustup)** e VS Build Tools — instalados na Fase 0 (26/08/2026).

---

## 1. Decisão de linguagem/stack

**Escolhido: Rust (núcleo) + Tauri 2 (interface)**

| Opção | Prós | Contras | Veredito |
|---|---|---|---|
| **Rust + Tauri 2** | Aprende Rust (objetivo!); áudio em tempo real sem GC; binário ~10 MB; UI premium com web tech; bindings maduros p/ whisper.cpp | Curva de aprendizado íngreme (que é justamente o que você quer) | ✅ **Escolhido** |
| Electron + JS/TS | UI fácil, ecossistema enorme | 150+ MB, alto consumo de RAM; nada novo a aprender em áudio/sistemas | ❌ |
| Python | `faster-whisper` é excelente p/ protótipos ML | Distribuir .exe é doloroso; UI desktop menos polida; GIL atrapalha áudio realtime | Útil só p/ experimentos |
| Elixir | Concorrência elegante | Ecossistema desktop/áudio nativo fraco no Windows | ❌ |
| C# / WinUI | Nativo Windows, whisper.net existe | Não era seu objetivo de aprendizado | Alternativa honrosa |

**Racional técnico:** captura de áudio e inferência são trabalho de sistemas — latência, threads, buffers — onde Rust brilha e ensina muito. O Tauri 2 usa o WebView2 que já vem no Windows 11: você ganha uma UI moderna (HTML/CSS/JS, animações, tema) sem carregar um Chromium inteiro como o Electron. E o `whisper-rs` liga direto no `whisper.cpp` (C++), 100% local, com feature flag `cuda` para a sua RTX 4050 — validado na documentação atual.

---

## 2. Arquitetura

```
isper/                        (workspace cargo)
├─ crates/
│  ├─ isper-core/             # motor: captura → VAD → whisper → texto (lib, sem UI)
│  └─ isper-cli/              # laboratório: testar o motor no terminal
├─ apps/
│  └─ isper-app/              # Tauri 2: tray, hotkey global, overlay, settings
├─ models/                    # modelos ggml baixados (no .gitignore!)
└─ ROADMAP.md
```

**Pipeline de áudio (ditado):**

```
mic (cpal, 48 kHz) → ring buffer → resample 16 kHz mono f32 (rubato)
  → VAD Silero (detecta silêncio) → whisper.cpp via whisper-rs (CUDA, language="pt")
  → texto → clipboard (salvar → colar Ctrl+V via enigo → restaurar)
```

**Princípios (boas práticas):**
- `isper-core` **não conhece UI** — testável sozinho, reutilizado por CLI e app.
- Máquina de estados explícita: `Idle → Recording → Transcribing → Pasting → Idle`.
- Comunicação core ↔ UI por canais/eventos (IPC do Tauri), nunca estado global solto.
- Erros: `thiserror` na lib, `anyhow` no app. Logs: `tracing`. Config: TOML via `serde` em `%APPDATA%\ISPer`.
- Overlay **não-focável** (always-on-top, sem roubar foco): o app alvo continua focado e recebe o Ctrl+V.

**Crates principais:** `cpal` (mic) · `rubato` (resample) · `whisper-rs` (ASR, feature `cuda`) · `voice_activity_detector` ou VAD do whisper.cpp (Silero) · `enigo` (simular Ctrl+V) · `arboard` (clipboard) · `rtrb`/`crossbeam` (ring buffer) · `hound` (WAV) · `rusqlite` (histórico) · plugins Tauri: `global-shortcut` (com `ShortcutState::Pressed/Released` p/ push-to-talk), `tray-icon`, `autostart`, `clipboard-manager`.

---

## 3. Modelos Whisper (formato ggml, do whisper.cpp)

| Modelo | Tamanho | Uso no ISPer |
|---|---|---|
| `small` | ~488 MB | Desenvolvimento (rápido até em CPU) |
| `large-v3-turbo` q5_0 | ~574 MB | **Produção pt-BR** — qualidade de large com 8× a velocidade; folga na sua VRAM de 6 GB |
| `large-v3` | ~3 GB | Máxima qualidade (reuniões importantes, pós-processamento) |

Ordem de aceleração: **CUDA (RTX 4050) → Vulkan → CPU quantizado**. Com CUDA, o turbo transcreve uma frase de 10 s em ~1 s.

---

## Fase 0 — Fundamentos ✅ (ambiente concluído em 26/08/2026)

Ambiente pronto + conceitos essenciais.

- [x] Instalar `rustup` (Rust 1.98) + VS Build Tools 2022 + CMake 4.4.3 portátil + libclang via pip (ver README p/ detalhes)
- [ ] CUDA Toolkit (p/ feature `cuda` do whisper-rs) — adiado para a Fase 3, começamos em CPU
- [ ] Rust Book caps. 1–10 + `rustlings` (ownership, borrowing, `Result`, traits, channels) — *estudo seu, no seu ritmo*
- [ ] Conceitos de ASR: PCM, sample rate, mel spectrogram, arquitetura encoder-decoder do Whisper — *estudo seu*
- [x] `git init`, workspace cargo, `.gitignore` (target/, models/)

**Pronto quando:** `cargo run` funciona num hello world do workspace. ✔
**Você aprende:** toolchain Rust, layout de workspace.

## Fase 1 — Núcleo no terminal ✅ (concluída em 26/08/2026)

Gravar do mic → WAV 16 kHz → transcrever → imprimir. Sem UI: só o motor.

- [x] Capturar microfone com `cpal`, enviando amostras por canal p/ outra thread
- [x] Resample p/ 16 kHz mono f32 com `rubato` (sinc, chunks de 1024 + process_partial)
- [x] Baixar `ggml-small.bin`; transcrever com `whisper-rs` (`set_language(Some("pt"))`)
- [x] `isper-cli rec 5` → grava 5 s e imprime o texto; `isper-cli file audio.wav` → transcreve arquivo
- [x] Medir tempo de inferência ÷ duração do áudio (fator de tempo real)
- [x] Testes: unit tests no isper-core (to_mono, resample) + fixtures TTS pt-BR em `fixtures/` (voz Microsoft Maria) validados via CLI

**Pronto quando:** você fala em pt-BR e o texto sai correto no terminal. ✔ ("Olá! O ISPR está transcrevendo perfeitamente. 1, 2, 3, 4, 5.")
**Você aprende:** threads e ownership em callbacks de áudio, FFI com C++, o pipeline ASR inteiro.

## Fase 2 — MVP de ditado, o "Wispr Flow local" ✅ (concluída em 26/08/2026)

- [x] App Tauri 2 com ícone na bandeja (tray); frontend vanilla HTML/CSS/JS sem build step (Svelte/React ficam p/ a Fase 3 se a UI crescer)
- [x] Hotkey global **push-to-talk** (segurar = gravar, soltar = transcrever) com `ShortcutState::Pressed/Released` — Ctrl+Alt+Espaço estava ocupado nesta máquina, então o app registra o primeiro livre de uma lista de candidatos (ficou **Ctrl+Shift+Espaço**; a dica na bandeja mostra qual)
- [x] Modo *mãos-livres*: toque rápido (<350 ms) no atalho, fala à vontade, VAD por energia (RMS + piso de ruído adaptativo) detecta ~1,2 s de silêncio e cola sozinho; segundo toque encerra na hora (VAD neural Silero fica p/ Fase 4)
- [x] Overlay: janela sem borda, transparente, always-on-top, **não-focável** (`set_focusable(false)`), com waveform animada do nível do mic (eventos IPC → canvas)
- [x] Inserção do texto: salvar clipboard atual → escrever texto → `Ctrl+V` simulado (`enigo`) → restaurar clipboard
- [x] Sons discretos de início/fim (WebAudio na própria UI, sem arquivos)
- [x] Tratamento de erros visível no overlay (áudio curto, modelo carregando, nada reconhecido)

**Pronto quando:** numa sessão do Claude Code no terminal, você aperta o atalho, fala, e o texto aparece no input — o caso de uso que motivou tudo. ✔ *("Testei e funcionou lindamente!" — 26/08)*
**Você aprende:** IPC Tauri, gestão de janelas Win32, integração de sistema.

## Fase 3 — Polimento premium (1–2 semanas)

- [ ] Ativar CUDA + `large-v3-turbo` q5_0; meta de latência: **< 1,5 s** para frase de 10 s
- [ ] Settings: modelo, idioma, atalho, dispositivo de entrada, tema claro/escuro, autostart
- [ ] Gerenciador de modelos: download com barra de progresso + verificação de checksum
- [ ] Histórico de ditados pesquisável (SQLite)
- [ ] Dicionário pessoal: termos que o Whisper erra (nomes próprios, "OptSolv", jargões) corrigidos via `initial_prompt` ou pós-processamento
- [ ] Instalador `.msi`/`.exe` (bundler do Tauri), ícone e identidade visual
- [ ] Microinterações e animações na UI (aqui entra o "premium")

## Fase 4 — Notetaker de reuniões Teams (3–4 semanas)

A jogada: **não precisa de bot nem API paga** — captura-se o áudio que sai da sua caixa de som (loopback WASAPI) + seu mic.

- [ ] Capturar áudio do sistema com o crate `wasapi` (modo loopback) em paralelo ao mic
- [ ] (Avançado) Loopback **por processo**: capturar só o áudio do Teams (API do Windows 10 2004+)
- [ ] Transcrição contínua em blocos com timestamps (janela deslizante com sobreposição p/ não cortar palavras)
- [ ] Separação básica de falantes: canal do mic = "Eu", loopback = "Participantes"
- [ ] Diarização real (quem falou o quê) com `sherpa-onnx` — item stretch
- [ ] Biblioteca de reuniões: SQLite + exportar Markdown (título, data, transcript, participantes)
- [ ] Detectar reunião ativa (janela do Teams aberta + áudio fluindo) → notificação "Gravar transcrição?"

⚠️ **LGPD/etiqueta:** avise os participantes de que a reunião está sendo transcrita.

## Fase 5 — Inteligência (contínuo)

> **Decisão (26/08/2026):** LLM de **nuvem via API**, não local. Rodar um LLM local pesaria na máquina — os 6 GB de VRAM já servem o Whisper durante as reuniões.

- [ ] Camada de provider abstraída (trait `LlmProvider`) — trocar de API sem reescrever o app
- [ ] Resumo pós-reunião, action items e decisões via API de nuvem (ex.: Claude API; alternativas com free tier: Groq, Gemini)
- [ ] Insights em tempo real: janela deslizante do transcript → prompt periódico ("o que ficou pendente?", "prometi algo?")
- [ ] Busca semântica no histórico de reuniões (embeddings leves — decidir provider na hora)
- [ ] Privacidade: só o **texto** do transcript vai à API — áudio nunca sai da máquina; chave de API no Credential Manager do Windows

---

## Boas práticas transversais

- Commits pequenos e frequentes desde o dia 1; mensagens descritivas
- `cargo clippy -- -D warnings` e `cargo fmt` antes de todo commit
- Núcleo testável sem microfone real (trait `AudioSource` → mock nos testes)
- Modelos nunca no git (`models/` no `.gitignore`)
- Privacidade por padrão: nenhum áudio sai da máquina, nunca
- README com GIF de demonstração; CHANGELOG a partir da Fase 3

## Custos: R$ 0 até a Fase 4

Whisper (MIT) · whisper.cpp (MIT) · whisper-rs (Unlicense) · Tauri (MIT/Apache-2.0) · Silero VAD (MIT) · sherpa-onnx (Apache-2.0). Todos os pesos de modelo são abertos e gratuitos. Único custo variável: a API de LLM na Fase 5 (pago por uso; free tiers de Groq/Gemini mitigam).

---

**Próximo passo:** Fase 2 — MVP de ditado (app Tauri 2: hotkey global push-to-talk → overlay → colar no app ativo).
