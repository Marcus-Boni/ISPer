# ISPer — Roadmap de Desenvolvimento

> Ditado por voz 100% local e gratuito (estilo Wispr Flow), evoluindo para notetaker de reuniões do Teams com insights de IA.
> Objetivo paralelo: **aprender Rust** e entender como transcrição de fala (ASR) funciona por dentro.

**Decidido em:** 25/08/2026 · **Plataforma:** Windows 11 · **Custo alvo:** R$ 0 nas Fases 0–4 · Fase 5 usa API de nuvem (pago por uso)

---

## Estado atual — 11/09/2026 · v0.14.0 (fases 7.1 e 7.2)

| Fase | Estado | Resumo |
|---|---|---|
| F0 Fundamentos | ✅ | ambiente pronto; só ficam 2 itens de estudo pessoal |
| F1 Núcleo no terminal | ✅ | |
| F2 MVP de ditado | ✅ | validado no caso de uso real |
| F3 Polimento premium | ✅ | |
| F4 Notetaker Teams | ✅ | validado em reunião real (07/09); detecção de chamada entregue em 10/09 (validar numa chamada real) |
| F5 Inteligência | ✅ | resumo, título, polimento, insights ao vivo e busca semântica (Gemini ou Ollama local) — 10/09 |
| F6 Acabamento premium | ✅ | falta só a assinatura de código (→ 7.3) |
| F7 Maturidade de engenharia | 🟡 | 7.1 e 7.2 concluídas (11/09); 7.3 com 3 de 4 itens (falta a candidatura à SignPath); 7.5 com 2 itens entregues; 7.4 e 7.6 não começadas |

**79 itens entregues · 17 em aberto** (2 deles de estudo pessoal). Ordem sugerida: candidatura SignPath → 7.4 → 7.5 → 7.6.

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
- [x] Transcrição contínua em blocos (~20 s, cortados no ponto mais silencioso p/ não partir palavra) com timestamps pelo **relógio da reunião** (o loopback não entrega amostras nas pausas — contar amostras derraparia)
- [x] Separação básica de falantes: canal do mic = "Eu", loopback = "Participantes", intercalados por timestamp
- [x] Diarização real (01/09): crate `isper-diarize` (sherpa-onnx via `sherpa-rs` com binários pré-compilados; pyannote segmentation 3.0 + 3D-Speaker ERes2Net, ~45 MB baixados pelo gerenciador). Roda ao encerrar sobre o áudio concatenado dos participantes; o core guarda o mapa bloco→relógio para casar os turnos com os segmentos do Whisper → "Participante 1, 2, 3…". Número de falantes descoberto por agrupamento (threshold padrão 0.3, ajustável via `ISPER_DIARIZE_THRESHOLD`; `isper-cli diarize <wav>` calibra offline). Validado na fixture Maria→Zira→Maria: a 0.2 separou as duas vozes corretamente; rótulos renumerados por ordem de aparição. Calibração final com vozes reais pendente
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

- [ ] Migrações de schema por `PRAGMA user_version` (em vez de `ALTER` ignorando erro) e config versionada com migração explícita
- [ ] Política de retenção (apagar transcrições após N dias — LGPD) e backup/exportação do banco
- [ ] Criptografia em repouso opcional (SQLCipher) para transcrições sensíveis
- [ ] Logs em JSON com rotação; "Exportar diagnóstico" (zip com logs + config sem segredos + versões); métricas locais (latência p50/p95, taxa de erro) — telemetria remota só opt-in

### 7.5 Experiência premium

- [ ] Onboarding de primeira execução (testar mic → escolher modelo → atalho → IA opcional)
- [ ] `desfazer` em toast no lugar de `confirm()` para exclusões; tema claro/escuro seguindo o sistema
- [ ] i18n desde já (dicionário JSON, pt-BR primeiro) e README em inglês
- [ ] Acessibilidade: navegação completa por teclado e teste com NVDA
- [x] Detecção de reunião ativa → "Gravar transcrição?" (10/09; detalhes na Fase 4)
- [x] Indicador flutuante fixo em repouso (10/09): "Indicador" no Início e na bandeja alternam mostrar/ocultar; antes o preview sumia em 2,5 s. Configurações redimensionável; Ditados em largura inteira

### 7.6 Distribuição e documentação

- [ ] `winget install ISPer` (manifesto no winget-pkgs) e zip portátil
- [ ] Vitrine do repositório no GitHub: descrição em inglês revisada, *topics* (rust, tauri, whisper, speech-to-text, windows, meeting-notes), README em inglês com GIF de demonstração, imagem de *social preview*
- [ ] Site de docs (mdBook no GitHub Pages): guia, FAQ, troubleshooting, arquitetura
- [ ] ADRs em `docs/adr/` (Rust+Tauri, LLM em nuvem, keepalive do loopback, diarização pós-hoc)
- [ ] `cargo doc` com `#![deny(missing_docs)]` no core; feature flags para o experimental (legendas ao vivo, comandos de voz)

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

**Próximo passo:** a candidatura à SignPath Foundation (ação do mantenedor, ver `docs/RELEASE.md`) e, enquanto ela tramita, a 7.4 — dados e observabilidade responsável. Continuam com quem tem a máquina: cadastrar o segredo `TAURI_SIGNING_PRIVATE_KEY` no repositório (o job `publish` precisa dele), rodar o soak de 2 h (`tools/e2e/soak.ps1 -Minutes 120`), ativar o repositório no Coveralls e validar a detecção de chamada e os insights ao vivo numa reunião real do Teams.
