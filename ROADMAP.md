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

## Fase 3 — Polimento premium ✅ (completa em 02/09/2026 — só microinterações ficam para depois)

- [x] Ativar CUDA + `large-v3-turbo` q5_0 — a RTX 4050 transcreve 10,4 s de áudio em **0,6 s** (16,1× tempo real; meta de <1,5 s superada). O app prefere o turbo quando compilado com `--features cuda`, e a RAM caiu p/ ~300 MB (o modelo mora na VRAM)
- [x] Settings (27/08): janela de Configurações na bandeja — **atalho** (reaplicado na hora, sem reiniciar), **idioma**, **dicionário pessoal**, **provider de IA + chave + teste de conexão** e **autostart**. Dispositivo de entrada e tema ficam para depois
- [x] Gerenciador de modelos (01/09): crate `isper-models` — catálogo, download com progresso, **SHA-256 conferido contra o `lfs.oid` publicado pelo Hugging Face** (nada de checksum hardcoded), pasta `%LOCALAPPDATA%\ISPer\models`, escrita atômica `.part` → final; UI em Configurações → Modelos (baixar/remover/escolher, troca a quente); sem modelo instalado o app abre as Configurações sozinho
- [x] Histórico de ditados no SQLite (27/08): tabela `dictations` em `%APPDATA%\ISPer\isper.db` — cada texto colado fica registrado com data e métricas; um viewer na UI fica para depois
- [x] Dicionário pessoal (27/08): os termos configurados viram o `initial_prompt` do Whisper em ditados E reuniões — nomes próprios e siglas saem certos
- [x] Instalador (02/09): `ISPer_0.5.0_x64-setup.exe` (NSIS, instalação por usuário sem UAC, pt-BR) gerado pelo bundler do Tauri com as DLLs do CUDA empacotadas — **398 MB** (o `cublasLt64_13.dll` sozinho tem 442 MB antes da compressão); sem modelos dentro — o app abre as Configurações no primeiro uso para baixar. MSI via WiX também configurado (download do WiX depende de rede)
- [x] Instância única (07/09): `tauri-plugin-single-instance` — clicar de novo no atalho não abre outro ISPer; encaminha para o já aberto, que mostra a tela Início (até 08/09, a Biblioteca)
- [x] Biblioteca de reuniões (07/09): janela própria com busca (título/resumo/transcript), detalhe com resumo e transcript por falante (cores por participante), renomear, abrir `.md`, excluir do histórico (arquivo preservado) e aba de Ditados
- [x] Indicador flutuante (07/09): arrastável (posição lembrada), modo mini (ponto + cronômetro da reunião), ocultar com retorno pela bandeja
- [x] Tela Início (08/09): janela central que abre com o app (nunca no autostart — o registro passa `--autostart`), no clique esquerdo do ícone da bandeja e no 2º clique do atalho. Mostra o estado do motor (`EngineStatus`: carregando / pronto / sem modelo / falha — fonte única no backend), o atalho em teclas, modelo + GPU, idioma, dicionário, fonte de áudio, diarização e IA; botão de gravar/encerrar reunião com cronômetro; checklist do que falta ou é opcional (baixar modelo, chave de IA, diarização, primeiro ditado…); totais e reuniões recentes (clique abre a Biblioteca já selecionada). O evento `isper-status` mantém Início e Biblioteca ao vivo; toggle "mostrar ao abrir" no rodapé e em Configurações → Sistema
- [x] Modularização do app (09/09): `main.rs` (2.222 linhas) virou bootstrap de ~370 linhas + 10 módulos por responsabilidade (`state`, `shortcuts`, `tray`, `dictation`, `meetings`, `views`, `overlay`, `settings`, `library`, `home`; `prelude` reexporta). Split mecânico item a item (script conferindo que nenhum item ficou sem destino), sem mudança de comportamento; build limpo, sem warnings novos
- [x] Robustez e engenharia (09/09, v0.10.0): **notificação do Windows** ao salvar a reunião (toast via `tauri-winrt-notification` com AUMID próprio registrado em HKCU — funciona no exe solto; clique abre a Biblioteca na reunião; segunda notificação silenciosa quando os falantes são identificados; opção "abrir .md" / "nada" em Configurações; fallback para o AUMID do PowerShell e, se tudo falhar, abre o arquivo), **sair com reunião ativa salva antes**, **logs em arquivo** com rotação diária e retenção de 14 dias (`tracing-appender`) + **Diagnóstico** nas Configurações (motor, modelo, DLLs do CUDA, microfones, caminhos; copiar / abrir pasta de logs), **SQLite em WAL com busy_timeout** (diarização em segundo plano gravando enquanto a Biblioteca lê), **config.toml gravado atomicamente**, **áudio dos participantes em i16** (metade da RAM em reuniões longas), **CI no GitHub Actions** (fmt, testes dos crates, `cargo check` do app sem CUDA)
- [x] Diarização em segundo plano (09/09): medido com 30 min de áudio sintético, a diarização na CPU levou 12,8 min (703 MB de pico) — o `sherpa-rs` fixa `num_threads: 1`. Antes ela bloqueava o fim da reunião e, na prática, ninguém esperava (todas as reuniões reais ficaram só com "Participantes"). Agora a reunião é salva/aberta na hora e `diarize_in_background` relabela os segmentos no banco (`relabel_segments`, casando pelo início) e regrava o `.md` ao terminar; `home_status.diarizing_meeting` mostra o chip. Próximo passo natural: falar direto com o `sherpa-rs-sys` para usar todas as threads (estimativa: 5–8× mais rápido)
- [x] Rodada "mais completo" (09/09, v0.9.0): **transcrição ao vivo** (`MeetingOptions.on_segment` → evento `isper-live`; painel no card de reunião do Início e última fala no indicador; histórico da reunião atual em memória para quem abre no meio), **título automático** pela IA (linha `TÍTULO:` na mesma chamada do resumo, `summarize_meeting_titled`; `.md` regravado por `render_markdown`, fonte única), **parágrafos legíveis** (`group_speech`: mesmo falante só até 4 s de pausa e 60 s de bloco — Markdown, DOCX e Biblioteca), **renomear falante** por reunião (`rename_speaker` no banco + regravação do `.md`; cores por ordem de aparição), **copiar resumo/transcript** e **exportar SRT/DOCX** (`isper-core::export`, DOCX = OOXML mínimo em ZIP "store" escrito à mão, com testes), **atalho global de reunião** (Ctrl+Alt+M padrão, com debounce; handler distingue os dois `Shortcut`s) e **ícone da bandeja vermelho** durante a gravação (ponto desenhado sobre o ícone em runtime), **polimento do ditado por IA** (opcional; `polish_dictation` com guarda-corpo que devolve o original se a resposta não parecer o mesmo texto; histórico guarda `raw_text`), **escolha de microfone** (`list_input_devices`, `open_input_stream_on`, `AudioHandle::set_device`) e **captura livre de atalho** ("Gravar atalho" nas Configurações). Modelos de diarização instalados nesta máquina
- [x] Indicador com prioridade máxima (09/09): `alwaysOnTop` só liga a flag e o tao não a reaplica quando já está ligada — outras janelas topmost ativadas depois passavam na frente. Agora `show_overlay` e um keepalive (1,5 s enquanto visível) chamam `SetWindowPos(HWND_TOPMOST, SWP_NOACTIVATE)` direto (`windows-sys`), com o HWND capturado no setup
- [x] Busca dentro da reunião (09/09): barra fixa no topo do detalhe da Biblioteca — destaque (`<mark>`) no resumo e no transcript, casamento sem acentos/maiúsculas com mapa de índices, contador, Enter/Shift+Enter, "Só trechos" (filtra as falas), Ctrl+F contextual; a busca geral pré-preenche o termo quando ele está no transcript; ditados também destacam o termo
- [x] Microinterações e animações na UI (08/09): pequeno design system compartilhado em `ui/assets/base.css` (tokens de cor/movimento/raio, componentes: botões com estados ocupado/sucesso/armado, chips, cards, switch, radio, progresso com brilho, abas com indicador deslizante, toast, skeleton) + `ui/assets/ui.js` (toast, count-up, entrada escalonada, confirmação inline em dois passos, abas). Tipografia própria offline: Fraunces (títulos/números) + Hanken Grotesk (corpo), OFL, em `ui/assets/fonts/`. Atmosfera (brilho radial quente + grão) nas janelas; indicador com pop ao acordar, shake no erro, burst no sucesso, waveform interpolado; Início reage ao ditado ao vivo (teclas "afundam", cabeçalho gravar → transcrever → colado ✓, totais em count-up) via `isper-state` agora em broadcast; Biblioteca com skeleton, seleção animada, crossfade do detalhe, "Excluir mesmo?" inline (sem `confirm()`), "copiado ✓"; Configurações com progresso %/brilho, "Salvo ✓", Ctrl+S. Só `transform`/`opacity`, 120–560 ms, e tudo desliga com `prefers-reduced-motion`

## Fase 4 — Notetaker de reuniões Teams ✅ (completa em 02/09/2026 — validar numa reunião real)

A jogada: **não precisa de bot nem API paga** — captura-se o áudio que sai da sua caixa de som (loopback WASAPI) + seu mic.

- [x] Capturar áudio do sistema com o crate `wasapi` em paralelo ao mic — com três defesas descobertas na prática (o loopback do cpal estagna neste endpoint USB): **keepalive** de silêncio integrado (o endpoint nunca suspende), **drenagem completa** (GetBuffer devolve 1 pacote de ~10 ms por chamada) e **watchdog por bytes** (reabre o cliente se ficar 1 s sem dados). Validado: 20/20 s capturados com stream contínuo, transcrição do WAV capturado perfeita
- [x] (Avançado) Loopback **por processo** (01/09): `LoopbackSource::Process` usa `new_application_loopback_client(pid, include_tree=true)` do `wasapi` — acha o processo-raiz do Teams (`ms-teams.exe`/`Teams.exe`, filhos WebView2 inclusos); se não estiver rodando, cai para o sistema e avisa no pill. Configurável em Configurações → Reuniões e na CLI (`--source teams|process:<exe>`)
- [x] Transcrição contínua em blocos (~20 s, cortados no ponto mais silencioso p/ não partir palavra) com timestamps pelo **relógio da reunião** (o loopback não entrega amostras nas pausas — contar amostras derraparia)
- [x] Separação básica de falantes: canal do mic = "Eu", loopback = "Participantes", intercalados por timestamp
- [x] Diarização real (01/09): crate `isper-diarize` (sherpa-onnx via `sherpa-rs` com binários pré-compilados; pyannote segmentation 3.0 + 3D-Speaker ERes2Net, ~45 MB baixados pelo gerenciador). Roda ao encerrar sobre o áudio concatenado dos participantes; o core guarda o mapa bloco→relógio para casar os turnos com os segmentos do Whisper → "Participante 1, 2, 3…". Número de falantes descoberto por agrupamento (threshold padrão 0.3, ajustável via `ISPER_DIARIZE_THRESHOLD`; `isper-cli diarize <wav>` calibra offline). Validado na fixture Maria→Zira→Maria: a 0.2 separou as duas vozes corretamente; rótulos renumerados por ordem de aparição. Calibração final com vozes reais pendente
- [x] Biblioteca de reuniões: SQLite (`%APPDATA%\ISPer\isper.db`) + exportar Markdown (`Documentos\ISPer\Reunioes\`)
- [ ] Detectar reunião ativa (janela do Teams aberta + áudio fluindo) → notificação "Gravar transcrição?"
- [ ] Validar numa reunião real do Teams (ou vídeo do YouTube) — *seu teste!*

⚠️ **LGPD/etiqueta:** avise os participantes de que a reunião está sendo transcrita (o Markdown gerado já traz o lembrete).

## Fase 5 — Inteligência (camada de IA construída em 26/08/2026)

> **Decisão (26/08/2026):** LLM de **nuvem via API**, não local. Rodar um LLM local pesaria na máquina — os 6 GB de VRAM já servem o Whisper durante as reuniões.

- [x] Camada de provider abstraída (trait `LlmProvider` no crate `isper-llm`) — **Claude API** (padrão `claude-opus-5`, com fallback de recusa server-side), **Groq** (free tier, `llama-3.3-70b-versatile`) e **Gemini** (free tier, `gemini-2.5-flash`); HTTP cru via `ureq` (Rust não tem SDK oficial da Anthropic); modelo configurável por provider
- [x] Resumo pós-reunião, pontos principais, action items e decisões — anexado ao Markdown da reunião e gravado na coluna `summary` do SQLite, tanto no app quanto na CLI; se a API falhar, o transcript já está salvo
- [ ] Insights em tempo real: janela deslizante do transcript → prompt periódico ("o que ficou pendente?", "prometi algo?")
- [ ] Busca semântica no histórico de reuniões (embeddings leves — decidir provider na hora)
- [x] Privacidade: só o **texto** do transcript vai à API — áudio nunca sai da máquina; chave no **Credential Manager do Windows** (crate `keyring`; env `ISPER_<PROVIDER>_API_KEY` como fallback); a chave da Gemini vai em header, nunca na URL
- [ ] Validar com sua chave: `isper-cli llm use groq` → `llm set-key groq` → `llm test` — *seu teste!*

---

## Fase 6 — Acabamento premium (08 a 10/09/2026)

Rodadas pedidas depois do notetaker funcionar: primeiro UX (tela Início,
design system com microinterações, indicador sempre no topo, busca dentro da
reunião), depois funcionalidades (v0.9.0), robustez (v0.10.0) e, por fim,
qualidade de transcrição, legendas, momentos marcados e distribuição (v0.11.0).

- [x] Tela Início central; design system (`base.css` + `ui.js`), animações com `prefers-reduced-motion`
- [x] Indicador com prioridade acima de qualquer janela (SetWindowPos TOPMOST + keepalive); busca com destaque dentro da reunião
- [x] v0.9.0: transcrição ao vivo, título por IA, parágrafos, diarização em segundo plano, renomear falantes, copiar/exportar (SRT/DOCX), atalho global de reunião + ícone vermelho na bandeja, polimento do ditado por IA, microfone e captura livre de atalho
- [x] v0.10.0: notificação do Windows ao salvar (em vez de abrir o arquivo), logs em arquivo com rotação, diagnóstico, sair salvando, WAL, config atômica, áudio em i16, CI, app modularizado
- [x] v0.11.0: filtro de alucinações do Whisper (`no_speech_prob`, frases de legenda, loops), correção por dicionário, comandos de voz no ditado ("nova linha", "ponto final", "apagar isso"…), legendas ao vivo no indicador, momentos marcados (★, Ctrl+Alt+K) no Markdown/DOCX/Biblioteca e priorizados no resumo, instalador NSIS com atualização automática assinada
- [ ] Canal CPU (sem CUDA) publicado pelo CI para máquinas sem GPU NVIDIA
- [ ] Assinatura de código (certificado) para o instalador não disparar o SmartScreen

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
