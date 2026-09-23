# ISPer — Roadmap de Desenvolvimento

> Ditado por voz 100% local e gratuito (estilo Wispr Flow), evoluindo para notetaker de reuniões do Teams com insights de IA.
> Objetivo paralelo: **aprender Rust** e entender como transcrição de fala (ASR) funciona por dentro.

**Decidido em:** 25/08/2026 · **Plataforma:** Windows 11 · **Custo alvo:** R$ 0 nas Fases 0–4 · Fase 5 usa API de nuvem (pago por uso)

---

## Estado atual — 23/09/2026 · v0.20.0 (7.5 e 7.6 entregues; o que falta da F7 depende de terceiros ou é à mão)

| Fase | Estado | Resumo |
|---|---|---|
| F0 Fundamentos | ✅ | ambiente pronto; só ficam 2 itens de estudo pessoal |
| F1 Núcleo no terminal | ✅ | |
| F2 MVP de ditado | ✅ | validado no caso de uso real |
| F3 Polimento premium | ✅ | |
| F4 Notetaker Teams | ✅ | validado em reunião real (07/09); detecção de chamada entregue em 10/09 (validar numa chamada real); **auditoria completa do pipeline em 18/09** — passe final, VAD, falante por palavra e a causa do "Participante 255" (v0.16.0) |
| F5 Inteligência | ✅ | resumo, título, polimento, insights ao vivo e busca semântica (Gemini ou Ollama local) — 10/09 |
| F6 Acabamento premium | ✅ | falta só a assinatura de código (→ 7.3) |
| F7 Maturidade de engenharia | 🟡 | 7.1 e 7.2 concluídas (11/09); 7.3 com 3 de 4 itens (candidatura à SignPath enviada em 13/09, aguardando); 7.4 com 3 de 4 itens (criptografia em repouso adiada com decisão registrada); 7.5 com 5 de 6 (v0.19.0 — falta a rodada com o NVDA); 7.6 com 3 de 5 (v0.20.0 — winget em revisão no winget-pkgs, social preview a subir à mão) |
| F8 Copilot de reunião | 🟡 | entregue na v0.18.0 (22/09): decisões, ações, riscos e perguntas ao vivo, ata, streaming, memória de reuniões passadas, `Ctrl+Alt+C` e Biblioteca; em inglês desde a v0.19.0; 5 itens em aberto — validar a memória, custo com a janela fechada, notas que não são salvas, idioma do que a IA escreve e o sync do portal quando sobra a branch do PR anterior |

**106 itens entregues · 13 em aberto** (2 deles de estudo pessoal; o placar sai das caixas do arquivo). Ordem sugerida: consertar o sync do portal antes da próxima release e decidir o custo do Copilot com a janela fechada e as notas dele (8.3) → a rodada com o NVDA (7.5) e a social preview (7.6), que são à mão → validar a memória do Copilot numa reunião real, com a busca semântica ligada, enquanto o winget e a SignPath tramitam.

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
- [x] CUDA Toolkit (p/ feature `cuda` do whisper-rs) — feito na Fase 3 (26/08): CUDA 13.3 + `large-v3-turbo` na RTX 4050
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
- [x] Transcrição contínua em blocos (~20 s) com timestamps pelo **relógio da reunião** (o loopback não entrega amostras nas pausas — contar amostras derraparia). **Revisto em 18/09**: o corte era "a janela de 100 ms de menor energia do último 1,5 s", e menor energia existe no meio de uma palavra — era daí que saía `manual` → `anual`. Agora o ponto tem de ser silêncio de verdade (abaixo de 15% do volume do bloco e de um piso absoluto); sem silêncio, o buffer segue até 32 s e o corte forçado é contado
- [x] Separação básica de falantes: canal do mic = "Eu", loopback = "Participantes", intercalados por timestamp
- [x] Diarização real (01/09): crate `isper-diarize` (sherpa-onnx via `sherpa-rs` com binários pré-compilados; pyannote segmentation 3.0 + 3D-Speaker ERes2Net, ~45 MB baixados pelo gerenciador). **Revista em 18/09**: rodava sobre o áudio *concatenado* (só os blocos não silenciosos, emendados) com limiar 0,3. Medindo a mesma reunião de 3 falantes em duas durações, o agrupamento do sherpa (ligação completa sobre distância de cosseno) cria grupos proporcionais à DURAÇÃO: 7 grupos aos 3 min e **34 aos 19 min** — extrapolando para 2 h, ~200, que é o "Participante 255" relatado (o `u8` era o sintoma). Agora: áudio contínuo no relógio da reunião, limiar 0,5 (o default do próprio sherpa-onnx), absorção de grupos com menos de `max(6 s, 2% da fala)`, recusa de publicar contagem implausível e o campo **"quantos participantes"** em Configurações — a única coisa que se manteve estável na reunião longa. `isper-cli diarize <wav> --speakers N --threshold T` calibra offline
- [x] **Passe final** (18/09): ao encerrar, o áudio inteiro dos dois canais é retranscrito em segundo plano com VAD Silero, busca em feixe, contexto entre trechos e falante **palavra a palavra**, substituindo a transcrição do ao vivo. No corpus de regressão (190 s, 3 falantes): WER 8,45% → 5,28%, CER 6,85% → 4,16%, DER 28,1% → 20,6%, a 0,43× tempo real. Desligável em Configurações → Reuniões → Avançado. Arquitetura e medições em [`docs/transcription-pipeline.md`](docs/transcription-pipeline.md)
- [x] **Benchmark reproduzível** (18/09): `isper-cli bench` roda o pipeline sobre um WAV e grava relatório JSON com todos os parâmetros, transcrição bruta, normalizada, com falantes e palavras com horário; com `--reference`/`--reference-turns` calcula WER, CER e DER. `isper-cli compare` põe duas rodadas lado a lado. Corpus gerado por `cargo run -p isper-cli --bin mkfixture` a partir de um roteiro versionado (o áudio não entra no Git)
- [x] Biblioteca de reuniões: SQLite (`%APPDATA%\ISPer\isper.db`) + exportar Markdown (`Documentos\ISPer\Reunioes\`)
- [x] Detectar reunião ativa → "Gravar transcrição?" (10/09): em vez de olhar janelas, o ISPer sonda as **sessões de áudio do WASAPI** — em chamada, o Teams (ou um filho WebView2) mantém o microfone aberto. `isper_core::calls` (`probe` + `CallTracker` com histerese: ~8 s para começar, ~24 s para terminar; só reprodução exige o dobro). Aviso por toast (clicar grava), banner no Início e indicador; fim da chamada com gravação ligada pergunta se encerra. Modos: avisar (padrão) · gravar automaticamente (e encerrar sozinho) · desligado
- [x] Validado em reunião real do Teams (07/09) — a rodada de 07/09 (instância única, Biblioteca, indicador) e a diarização em segundo plano saíram dessa primeira reunião

⚠️ **LGPD/etiqueta:** avise os participantes de que a reunião está sendo transcrita (o Markdown gerado já traz o lembrete).

## Fase 5 — Inteligência (camada de IA construída em 26/08/2026)

> **Decisão (26/08/2026):** LLM de **nuvem via API**, não local. Rodar um LLM local pesaria na máquina — os 6 GB de VRAM já servem o Whisper durante as reuniões.

- [x] Camada de provider abstraída (trait `LlmProvider` no crate `isper-llm`) — **Claude API** (padrão `claude-opus-5`, com fallback de recusa server-side), **Groq** (free tier, `llama-3.3-70b-versatile`) e **Gemini** (free tier, `gemini-2.5-flash`); HTTP cru via `ureq` (Rust não tem SDK oficial da Anthropic); modelo configurável por provider
- [x] Resumo pós-reunião, pontos principais, action items e decisões — anexado ao Markdown da reunião e gravado na coluna `summary` do SQLite, tanto no app quanto na CLI; se a API falhar, o transcript já está salvo
- [x] Insights em tempo real (10/09): a cada 3/5/10 min, os últimos ~15 min do transcript vão ao provider com quatro perguntas — pendências, compromissos de "Eu", decisões, perguntas em aberto — e a resposta anterior é consolidada (`isper_llm::insights`, prompt testado com `LlmProvider` falso). Painel no card de reunião do Início com "Atualizar agora"; rodadas sem fala nova são puladas; opt-in (custa API)
- [x] Busca semântica no histórico (10/09) — **provider decidido**: os providers de chat não servem (Anthropic e Groq não têm embeddings), então ficam **Gemini** (`gemini-embedding-001`, free tier, reutiliza a chave) ou **endpoint compatível com OpenAI**, que cobre o **Ollama local** (`nomic-embed-text`/`bge-m3`, 100% na máquina, sem chave). Vetores normalizados no SQLite (`embeddings`, com o modelo que os gerou), trechos de ~700 caracteres por parágrafo (`isper_core::embed`), cosseno por força bruta; botão "Semântica" na Biblioteca abre a reunião rolada no trecho; indexação automática ao salvar + "Indexar tudo"
- [x] Privacidade: só o **texto** do transcript vai à API — áudio nunca sai da máquina; chave no **Credential Manager do Windows** (crate `keyring`; env `ISPER_<PROVIDER>_API_KEY` como fallback); a chave da Gemini vai em header, nunca na URL
- [x] Validado com chave real (01/09): lista de modelos ao vivo por provider e erro claro de modelo indisponível; resumos, títulos e polimento por IA em uso desde então

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
- [x] v0.11.1: instalador corrigido — faltavam as DLLs do sherpa-onnx (`sherpa-onnx-c-api`, `onnxruntime`, `cargs`) e o app instalado pela 0.11.0 não abria; descoberto ao instalar pelo setup.exe e validar a primeira atualização automática
- [x] v0.12.0: variante CPU do instalador (gerada localmente pelo `release.ps1`; o runner do GitHub não tem CUDA nem consegue ligar o sherpa-onnx pré-compilado), runtime do Visual C++ dentro dos instaladores, pânicos no log, testes ponta a ponta em `tools/e2e`, `CHANGELOG.md` e workflow `release.yml` validando a tag
- [x] v0.12.1/v0.12.2: pastas do usuário pela API do Windows (não pelas variáveis de ambiente) e dados locais em `%LOCALAPPDATA%\com.isper.desktop`, fora da pasta padrão de instalação do Tauri (`%LOCALAPPDATA%\ISPer`), com migração automática
- [ ] Assinatura de código do instalador — caminho decidido em 10/09: gratuito via SignPath Foundation, depois de mover o build da release para o CI (*detalhes e alternativas descartadas em 7.3*)

## Fase 7 — Maturidade de engenharia (a partir de 10/09/2026)

Com F0–F6 entregues e o app em uso real, esta fase é sobre o que separa um
projeto bom de um produto confiável — o que times grandes fazem por padrão.
A ordem é impacto ÷ esforço.

### 7.1 Qualidade automatizada ✅ (concluída em 11/09/2026)

- [x] `cargo clippy --workspace --all-targets -- -D warnings` no CI + `[workspace.lints]` compartilhado por todos os crates
- [x] `cargo deny` (vulnerabilidades RustSec, licenças permitidas, duplicatas, origens) com `deny.toml` versionado
- [x] Dependabot para Cargo e GitHub Actions (PRs semanais agrupados)
- [x] gitleaks no CI — nenhum segredo no histórico
- [x] CSP real no Tauri (`default-src 'self'`) no lugar de `null`; violações entram em `window.__isperErrors` e o smoke test falha se houver alguma
- [x] Remover os `style="…"` inline dos HTML e fechar `style-src-attr` (10/09): 36 atributos e dois `style.cssText` viraram classes utilitárias em `base.css` (classe dobrada em vez de `!important`); a entrada escalonada usa `data-i` → `--i` via CSSOM. Qualquer atributo que voltar aparece como violação de CSP no smoke test
- [x] Versão numa só fonte (11/09): o `tauri.conf.json` não declara mais `version` (o Tauri lê do `Cargo.toml` do app); `release.ps1` e `release.yml` conferem só o `Cargo.toml` e recusam a duplicata se ela voltar
- [x] Edição Rust unificada (11/09): app em 2024 via `cargo fix --edition` — só avisos benignos de ordem de drop em expressões finais; `let`-chains onde o clippy novo pediu
- [x] Higiene do repositório (11/09): `loopdump.wav` fora do índice e no `.gitignore` (o arquivo local fica; o histórico não é reescrito — branch pública com tags e releases apontando para esses commits); `isper.db` da raiz segue como lixo local ignorado
- [x] `rust-toolchain.toml` (1.98.0 + rustfmt + clippy; o CI lê o canal do arquivo), `rustfmt.toml` (estilo 2024) e `.editorconfig` (11/09)
- [x] Proteção da branch `main` (11/09): ruleset do GitHub — sem push direto, sem force-push nem exclusão, histórico linear, PR obrigatório com os três checks do CI verdes. PRs do Dependabot triados num PR só: sysinfo 0.39, toml 1, dirs 6, crossbeam-channel 0.5.17, **ureq 3 migrado à mão** (isper-llm e isper-models, com testes de rede `#[ignore]`), checkout v7 e gitleaks-action v3

### 7.2 Testes que provam robustez ✅ (concluída em 11/09/2026 — 110 testes, eram 58)

- [x] `LlmProvider` falso (`isper-llm/src/testing.rs`, só em testes): resumo, título, polimento e insights testados de ponta a ponta sem rede — conteúdo do pedido, separação do título, guarda-corpo do polimento, propagação de erro do provider
- [x] Testes no crate do app (19) e na CLI (4), rodando no CI: `AppConfig::normalize` virou a regra única de validação (tela de Configurações e `config.toml` editado à mão), `load_from`/`save_to` com caminho, `migrate_legacy_local_in`, `missing_vars`, `clamp_to_monitors`, `llm_summary`, rótulos e candidatos de atalho, erros do atualizador; CLI com o `debug_assert` do clap e o parse dos subcomandos. Auditoria dos `unwrap()`: `clippy::unwrap_used` ligado no workspace (livre só em testes, `clippy.toml`), os 123 `lock().unwrap()` do app viraram `lock_or_recover()` (mutex envenenado não derruba o app) e não sobrou `unwrap()` em produção
- [x] Testes de propriedade (`proptest`): `like_pattern` conferido contra o `LIKE … ESCAPE` do próprio SQLite, `quiet_cut`, VAD por energia com relógio injetado (`Vad::new(started)` / `should_stop(rms, now)`)
- [x] Testes *golden* das exportações: `crates/isper-core/tests/golden/` com Markdown, SRT, `document.xml` e o `.docx` inteiro (ZIP determinístico, lido e conferido — CRC e tamanhos); `ISPER_UPDATE_GOLDEN=1` regenera
- [x] Cobertura como tendência: `coverage.yml` (cargo-llvm-cov no push em `main`, resumo no sumário do job, lcov como artefato, envio ao Coveralls sem bloquear PR) — falta só ativar o repositório em coveralls.io uma vez
- [x] Áudio dos participantes em disco: `OthersAudio`/`PcmSpool` gravam o PCM 16 kHz em `%TEMP%\ISPer` durante a reunião (era `Vec<i16>` em RAM, 115 MB/h) e a diarização lê de volta uma vez, depois; `tools/e2e/soak.ps1` mede a memória numa reunião longa (`-Minutes 120` é o soak de 2 h — rodar antes da próxima release)
- [x] Casos de áudio: microfone que para de entregar (fone desconectado, suspensão) é reaberto pelo fatiador (`AudioFeed`/`MicFeed`) sem derrubar a reunião, e o ditado encerra com o que capturou; erros do WASAPI/cpal ganham dica em português (modo exclusivo, dispositivo invalidado, formato, sem dispositivo); a posição do indicador é conferida contra os monitores atuais (monitor desligado, DPI). Roteiro de validação manual em `docs/TESTES.md`
- [x] e2e no CI como job noturno (`e2e-nightly.yml`): compila o app CPU num runner Windows e roda o `smoke.ps1` contra o exe real via CDP. VB-Cable descartado: o runner não tem dispositivo de áudio e o driver exigiria reboot; `meeting.ps1` e `soak.ps1` seguem locais

### 7.3 Segurança e confiança do binário

> **Decisão (10/09/2026) — assinatura de código pelo caminho gratuito: SignPath Foundation.** Certificado OV para projetos open source, chave no HSM deles, emitido para a Foundation; o ISPer cumpre os critérios (licença MIT, repositório público, download gratuito, projeto mantido e já publicado). Descartados: **Azure Trusted Signing** (US$ 9,99/mês, mas hoje não aceita pessoa física nem empresa no Brasil), **certificado OV pago** (US$ 100–400/ano + chave em token/HSM) e **autoassinado** (para o SmartScreen vale o mesmo que não assinar). O que a assinatura dá: editor verificado e reputação que acumula no certificado entre versões — ela **não** elimina o aviso do SmartScreen por si só (desde 2024 nem EV dá reputação instantânea; são semanas de instalações limpas). Enquanto o público for o autor e colegas de confiança, o instalador segue sem assinatura: a integridade das atualizações já é garantida pelo minisign do Tauri, e o aviso na primeira instalação é um clique ("Mais informações → Executar assim mesmo").

> **Correção (11/09/2026):** a SignPath Foundation só assina artefatos cujos jobs do GitHub Actions rodaram **todos em runners hospedados** — o runner self-hosted planejado abaixo não serviria (e num repositório público executaria código de qualquer PR). A premissa "CUDA e sherpa-onnx não compilam nos runners" caiu: o sherpa-onnx pré-compilado é baixado pelo `sherpa-rs` em qualquer máquina e o toolkit CUDA é instalado no runner pelo `Jimver/cuda-toolkit`. Detalhes em [`docs/RELEASE.md`](docs/RELEASE.md).

- [x] Builds de release no CI (11/09): `release.yml` compila as duas variantes em runners hospedados do GitHub — a CPU em `windows-latest`; a GPU em `windows-2022`, com o CUDA Toolkit 13.3 instalado no runner (sub-pacotes nvcc/nvvm/crt/cudart/cublas/thrust) e o whisper.cpp gerado com Ninja no ambiente do MSVC —, gera o SBOM, assina para o atualizador, escreve `latest*.json` e `SHA256SUMS.txt` e publica a release no push da tag; `workflow_dispatch` é ensaio. `scripts/release.ps1` vira caminho de reserva e compartilha a lógica de fechamento (`scripts/release-assets.ps1`)
- [ ] Assinatura gratuita via **SignPath Foundation**: o job `sign` já está no pipeline (pulado até as variáveis `SIGNPATH_*` existirem) e a assinatura minisign do atualizador acontece depois dele, porque o Authenticode muda os bytes. Falta a **candidatura** em signpath.org (ação do mantenedor: repositório, página de releases, descrição do app, `SECURITY.md` como contato) — começando pela variante CPU, porque a GPU embute as DLLs redistribuíveis do CUDA, que a SignPath pode ou não aceitar como bibliotecas de sistema; depois, cadastrar token e variáveis e publicar a próxima versão. A reputação no SmartScreen acumula com o tempo
- [x] SBOM CycloneDX 1.5 (`cargo cyclonedx`, ~420 componentes) publicado com cada release, junto com `SHA256SUMS.txt` (11/09; a v0.14.0 recebeu os dois retroativamente)
- [x] `SECURITY.md` (relato privado pelo GitHub — habilitado —, escopo, versões com suporte, como o app se protege), `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md` (Contributor Covenant 2.1), modelos de issue e PR (11/09, PR #10); alertas e atualizações de segurança do Dependabot habilitados

### 7.4 Dados e observabilidade responsável

- [x] Migrações de schema por `PRAGMA user_version` (13/09): `SCHEMA_VERSION = 2`, um passo por transação, banco legado entra como 0 e passa por um passo 1 idempotente, banco de versão maior é recusado com mensagem clara; instantes numéricos (`started_ts`, `at_ts`) preenchidos a partir das datas em texto. `config.toml` com `config_version` e `AppConfig::migrate` (mudanças de formato), separado de `normalize` (valores)
- [x] Retenção (13/09): Configurações → Sistema → "Guardar reuniões e ditados por" (30/90/180/365 dias; padrão para sempre) — varredura ao abrir, uma vez por dia e ao encurtar o prazo; apaga do banco (reunião, segmentos, momentos, vetores, ditados) e os `.md`/`.srt`/`.docx` da reunião. Backup com um clique (`VACUUM INTO` em `Documentos\ISPer\Backups`, com o app aberto); restauração manual documentada
- [ ] Criptografia em repouso opcional (SQLCipher) — **decisão (13/09): adiada.** O `rusqlite` só oferece SQLCipher compilando o OpenSSL junto (`bundled-sqlcipher-vendored-openssl`: Perl no build, minutos a mais no CI, chave para guardar e recuperar) e o ganho é pequeno enquanto o banco vive no perfil do usuário, protegido pelas ACLs do Windows — o BitLocker cobre o disco inteiro. Volta ao plano se surgir demanda (máquina compartilhada, exigência de compliance)
- [x] Observabilidade (13/09): log em arquivo em **JSON Lines** com rotação diária e 14 dias; "Exportar diagnóstico" (`.zip` com `diagnostico.json`, versões — ISPer, Tauri, WebView2, Windows —, `config.toml` e `llm.toml` sem segredos, métricas e os três últimos logs com as linhas de texto ditado removidas); métricas locais no Diagnóstico — ditados e blocos de reunião dos últimos 30 dias, falhas, p50/p95 da inferência e fator de tempo real — gravadas pelo próprio app (`events`, 90 dias). **Sem telemetria remota**, nem opt-in: nada sai da máquina

### 7.5 Experiência premium

- [x] Onboarding de primeira execução (testar mic → escolher modelo → atalho → IA opcional) (23/09): janela guiada com medidor de nível, download do modelo recomendado, ditado de teste e chave de IA testada; `config.toml` versão 2 (quem já usava não a vê); Configurações → Sistema reabre
- [x] `desfazer` em toast no lugar de `confirm()` para exclusões; tema claro/escuro seguindo o sistema (23/09)
- [x] i18n desde já (dicionário JSON, pt-BR primeiro) e README em inglês (23/09): todas as janelas (Copilot inclusive), bandeja, notificações, erros e README.en; o que a IA escreve (resumo e Copilot) continua em pt-BR — item em aberto na 8.3
- [ ] Acessibilidade: navegação completa por teclado e teste com NVDA — teclado, contraste AA, nomes acessíveis e reflow entregues e cobertos pelo `a11y.ps1` (23/09); roteiro do NVDA em `docs/TESTES.md`, falta a rodada com o leitor de tela
- [x] Detecção de reunião ativa → "Gravar transcrição?" (10/09; detalhes na Fase 4)
- [x] Indicador flutuante fixo em repouso (10/09): "Indicador" no Início e na bandeja alternam mostrar/ocultar; antes o preview sumia em 2,5 s. Configurações redimensionável; Ditados em largura inteira

### 7.6 Distribuição e documentação

- [ ] `winget install ISPer` (manifesto no winget-pkgs) e zip portátil — zip portátil no `release.yml` (23/09; o app avisa da versão nova sem instalar por cima) e manifestos do pacote `MarcusBoni.ISPer` (instalador CPU) gerados por `scripts/winget-manifests.ps1` e validados com `winget validate`; submetido em 23/09 como [microsoft/winget-pkgs#439838](https://github.com/microsoft/winget-pkgs/pull/439838) (0.19.0: CLA aceito e as 10 etapas de validação aprovadas, inclusive a instalação em máquina limpa; aguarda moderador) e [#439952](https://github.com/microsoft/winget-pkgs/pull/439952) (0.20.0); da versão seguinte em diante, o `winget.yml` abre o PR sozinho com o Komac ([`packaging/winget/README.md`](packaging/winget/README.md)); o zip portátil saiu na 0.20.0; decisões no [ADR 0012](docs/adr/0012-distribuicao-portatil-e-winget.md)
- [ ] Vitrine do repositório no GitHub: descrição em inglês revisada, *topics* (rust, tauri, whisper, speech-to-text, windows, meeting-notes), README em inglês com GIF de demonstração, imagem de *social preview* — descrição, 15 *topics*, GIF no `README.en.md` e a imagem em `docs/media/` prontos (23/09), gerados do app real por [`tools/showcase/`](tools/showcase/) só com telas sem dado pessoal; falta subir a imagem em Settings → Social preview (sem API, é à mão)
- [x] Site de docs (mdBook no GitHub Pages): guia, FAQ, troubleshooting, arquitetura (23/09): o portal [isper.pages.dev/docs](https://isper.pages.dev/docs/) já fazia o papel do mdBook, com busca (Next estático no Cloudflare Pages, checagem de links e orçamento no CI), e já tinha guia, FAQ e solução de problemas; entraram **Arquitetura** (as peças, os fluxos de ditado e de reunião, onde ficam os dados, os ADRs), **Primeira configuração** e **Tema, idioma e acessibilidade** (com o Desfazer)
- [x] ADRs em `docs/adr/` (23/09): Rust+Tauri, LLM em nuvem, keepalive do loopback, diarização pós-hoc e mais seis — os dois modos de transcrição, a UI sem build step, releases no CI, retenção "para sempre" com Desfazer, SQLCipher adiado e o próprio registro; índice e modelo em [`docs/adr/README.md`](docs/adr/README.md)
- [x] `cargo doc` com `#![deny(missing_docs)]` no core; feature flags para o experimental (legendas ao vivo, comandos de voz) (23/09): os 203 itens públicos do `isper-core` documentados, `#![deny(missing_docs)]` e `cargo doc` com avisos como erro no CI; feature flags **não** — os dois recursos já eram maduros e têm interruptor nas Configurações, e cada flag dobraria as variantes de build (decisão em [ADR 0011](docs/adr/0011-opcional-e-configuracao-nao-feature-flag.md))

## Fase 8 — Copilot de reunião (21 e 22/09/2026 · v0.18.0)

Durante a reunião, uma janela própria mostra a fala ao vivo ao lado de um feed
de decisões, ações, riscos e perguntas extraídos pela IA; o que o usuário
confirma vai para a ata e para a Biblioteca. Começou de um plano escrito por
outro agente (`implementation_plan.md`, fora do repositório) com ~1.600 linhas
implementadas; a revisão encontrou colisão de ids entre rodadas, notas que se
perdiam, legenda provisória duplicada, rede bloqueante no comando, cinco campos
do estado calculados e nunca mostrados na tela e, no app real, a janela fora da
ACL. Guia de uso em [isper.pages.dev/docs/copilot](https://isper.pages.dev/docs/copilot/usar-o-copilot/).

### 8.1 O Copilot ✅ (v0.18.0, 22/09/2026)

- [x] HUD (`copilot.html`) com a fala ao vivo e o feed de cards — Decisão, Ação (responsável e prazo, com aviso de prazo em aberto), Risco e Pergunta —, confirmar, descartar e desfazer. **O id do card é derivado de `kind` + título normalizado**, nunca o da IA: o modelo devolve `"c1"`, `"c2"` a cada rodada, e "Confirmar" mexia no card errado; títulos reformulados mesclam por semelhança de tokens
- [x] Gatilhos locais: frases de acordo, tarefa e objeção antecipam a análise, sem rede. Primeira leitura aos 20 s, pulso de 45 s, piso de 15 s entre rodadas
- [x] O que o usuário confirma entra na ata (seção do `.md`, com a cobrança dos prazos em aberto) e na tabela `decisions` (schema v3) — lido no instante em que a reunião encerra, e não no salvamento em segundo plano, que ainda espera o worker do Whisper
- [x] Streaming SSE: `LlmProvider::complete_stream` para Claude, Groq e Gemini, com a implementação padrão caindo no `complete`. O protocolo ficou separado do HTTP (`read_sse` sobre `BufRead`) para ser testado com bytes de cada API; "Pergunte à Reunião" e o bloco de notas escrevem na tela conforme o modelo gera
- [x] Memória de reuniões passadas sobre a busca semântica: disparada quando o tópico muda, até três cards, uma por reunião, com o resumo de uma frase do tópico como consulta
- [x] Dinâmica de fala acumulada — a lista `live` é podada em 400 segmentos, então somar a partir dela mostraria só o fim de uma reunião longa — e aviso de monólogo pela sequência contígua, não pelo total
- [x] Modo acoplado de 380 px com fixar por cima, e atalho global `Ctrl+Alt+C` no mesmo mecanismo dos outros três
- [x] Seção "Decisões e alertas" no detalhe da reunião, na Biblioteca, com salto para a fala que originou cada card
- [x] Ícones SVG no lugar de emojis e glifos em todas as janelas (os marcadores da lista de pendências por máscara CSS, porque `::marker` só aceita texto)
- [x] Geração da reunião em cada thread de análise: encerrar não interrompe uma chamada HTTP em voo, e sem ela o resultado atrasado caía na reunião seguinte e a limpeza da thread velha apagava o canal da nova
- [x] Janela na ACL (`capabilities/default.json`). Fora dela o Tauri nega `plugin:event|listen` e a janela não recebe evento nenhum — mas os comandos do app continuam respondendo, então ela carrega o estado ao abrir e congela, parecendo viva. **Janela nova precisa entrar nessa lista**
- [x] Banco de testes da janela (`tools/e2e/copilot-harness.py`): o HUD e a Biblioteca num Tauri simulado, sem compilar com CUDA. Não pega a ACL — lá `listen()` é mock
- [x] Cópia do banco antes de migrar o schema (`isper.db.v2.bak`, `VACUUM INTO`): até a v0.18.0 a migração era de mão única sem cópia nenhuma — o backup automático só existia antes da retenção

### 8.2 Lançamento ✅ (22 e 23/09/2026)

- [x] v0.18.0 publicada pelo `release.yml`: instaladores GPU e CPU, `latest*.json` assinados, SBOM e `SHA256SUMS.txt` conferidos no que foi publicado. O CHANGELOG da versão foi reescrito para quem vem da 0.17.1 — saíram as correções de bugs que só existiram durante o desenvolvimento do próprio Copilot
- [x] Sincronização do portal, que **nunca tinha funcionado** (zero execuções): o `release: published` nascia do `GITHUB_TOKEN` e não disparava o sync (o `release.yml` agora o dispara por `workflow_dispatch`, #41); o secret `PORTAL_SYNC_TOKEN` não existia (criado em 23/09); e `--tag v1.2.3` com espaço matava o script (#40). Caminho completo verificado com um PR de teste (#42) aberto pelo token, com os checks disparando, fechado sem merge

### 8.3 Em aberto

- [ ] Validar a memória com reuniões reais e calibrar `RECALL_MIN_SCORE` (0,55 foi escolhido para errar para o lado de calado, sem medição). Depende de ligar a busca semântica e indexar o histórico
- [ ] Decidir se o Copilot analisa com a janela fechada. Hoje ele roda em toda reunião gravada com provedor configurado — perto de 80 chamadas por hora —, ao contrário dos Insights ao vivo, que só fazem rodadas periódicas se ativados
- [ ] Salvar as notas do Copilot com a reunião. Hoje elas ficam só na memória do app e somem quando a reunião seguinte começa
- [ ] O que a IA escreve no idioma da interface: o resumo, os cards, as respostas e as notas do Copilot seguem o prompt em pt-BR mesmo com a interface em inglês (registrado na i18n da 7.5, 23/09)
- [ ] O sync do portal falha quando a branch do PR anterior ficou no remoto: o repositório não apaga a branch no merge, o checkout raso do workflow não a conhece e o `git push --force-with-lease` recusa com *stale info*. Aconteceu na v0.20.0 (a run das 18:43; a das 18:45 abriu o #61); a próxima release repete
- [x] Ver o disparo automático do portal (#41) funcionar numa release: "Portal avisado" no resumo da v0.19.0 (23/09), que abriu o #53 sozinha; na v0.20.0 o disparo também saiu

## Boas práticas transversais

- Commits pequenos e frequentes desde o dia 1; mensagens descritivas
- `cargo clippy -- -D warnings` e `cargo fmt` antes de todo commit
- Núcleo testável sem microfone real (trait `AudioFeed` no fatiador de blocos → fonte falsa nos testes; VAD com relógio injetado; provider de IA falso)
- Modelos nunca no git (`models/` no `.gitignore`)
- Privacidade por padrão: nenhum áudio sai da máquina, nunca
- README com GIF de demonstração; CHANGELOG a partir da Fase 3

## Custos: R$ 0 até a Fase 4

Whisper (MIT) · whisper.cpp (MIT) · whisper-rs (Unlicense) · Tauri (MIT/Apache-2.0) · Silero VAD (MIT) · sherpa-onnx (Apache-2.0). Todos os pesos de modelo são abertos e gratuitos. Único custo variável: a API de LLM na Fase 5 (pago por uso; free tiers de Groq/Gemini mitigam).

---

**Próximo passo:** validar a v0.16.0 numa reunião real e produzir um trecho de referência corrigido à mão — todo o ganho medido até aqui está em voz sintética, que pega regressão mas não mede qualidade absoluta. Depois, 7.5 — experiência premium (onboarding, tema, desfazer, acessibilidade, i18n), enquanto a candidatura à SignPath tramita (enviada em 13/09; o segredo `TAURI_SIGNING_PRIVATE_KEY` já está no repositório). Vale uma release 0.15.0 antes: as fases 7.3 e 7.4 mudam o que o usuário vê (retenção, backup, diagnóstico exportado, release compilada no CI). Continuam com quem tem a máquina: rodar o soak de 2 h (`tools/e2e/soak.ps1 -Minutes 120`), ativar o repositório no Coveralls e validar a detecção de chamada e os insights ao vivo numa reunião real do Teams.
