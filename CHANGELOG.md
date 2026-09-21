# Changelog

Todas as mudanças relevantes do ISPer ficam aqui. O formato segue o
[Keep a Changelog](https://keepachangelog.com/pt-BR/1.1.0/) e as versões, o
[Versionamento Semântico](https://semver.org/lang/pt-BR/): MAJOR para quebras,
MINOR para funcionalidades, PATCH para correções. A versão vive só em
`apps/isper-app/src-tauri/Cargo.toml` (o Tauri lê de lá; o `tauri.conf.json`
não a repete); a seção `## [versão]` daqui vira as notas da release
(`scripts/release.ps1`), e o workflow `release.yml` recusa uma tag que não
bata com ela.

## [Unreleased]

### Alterado
- **Legendas ao vivo mais rápidas.** O bloco ao vivo passa de 20 s (teto
  32 s) para 6 s (teto 12 s), cortado no silêncio como antes, e enquanto o
  bloco não fecha uma **legenda provisória** vai ao indicador a cada 2,5 s
  (texto mais claro; o bloco final a substitui). Se o worker acumular fila,
  a captura volta aos blocos longos até esvaziar. O passe final, que gera a
  ata, não muda — só o que aparece na tela durante a reunião.

## [0.16.1] - 2026-09-20

### Corrigido
- **A Biblioteca ordenava por ordem de inserção, não pela data da reunião**
  (`ORDER BY m.id DESC`). Enquanto toda reunião entrava na ordem em que
  acontecia, os dois coincidiam; qualquer reunião trazida de volta pela
  reimportação abaixo aparecia fora de lugar. Agora ordena por `started_ts`.

### Adicionado
- **Reimportar reuniões da pasta** (Configurações → Sistema, e
  `isper-cli import`). O `.md` em `Documentos\ISPer\Reunioes` é gravado
  ANTES do banco e sobrevive a qualquer acidente com o índice; este caminho
  lê a pasta de volta e reinsere o que faltar — falas, horários, falantes,
  momentos marcados e o resumo da IA. É seguro repetir: o que já está na
  Biblioteca é pulado (chave: o caminho do `.md`, com a data como reserva).
  A CLI simula por padrão e só grava com `--apply`.

  O que NÃO volta é a granularidade original: o Markdown guarda parágrafos
  (falas seguidas do mesmo falante já agrupadas), então uma reunião
  reimportada tem segmentos mais longos que a original. O texto é o mesmo.

## [0.16.0] - 2026-09-18

### Adicionado
- **Passe final da reunião** — ao encerrar, o ISPer refaz a transcrição sobre
  o áudio inteiro em segundo plano e substitui a que apareceu ao vivo. Corte
  guiado por detecção de voz (Silero, 0,9 MB, baixado na primeira reunião),
  busca em feixe, contexto entre trechos e falante palavra a palavra. Medido
  no corpus de regressão: **WER 8,45% → 5,28%**, **CER 6,85% → 4,16%**,
  **DER 28,1% → 20,6%**, ao custo de 0,43× a duração da reunião (era 0,36×).
  Desligável em Configurações → Reuniões → Avançado.
- **Número de participantes** em Configurações → Reuniões: informado, o
  agrupamento corta em exatamente N grupos em vez de decidir por distância —
  a única coisa que manteve a contagem estável em reunião longa.
- **`isper-cli bench`** — roda o pipeline inteiro sobre um WAV e grava
  relatório (JSON com todos os parâmetros e métricas), transcrição bruta,
  normalizada, com falantes e palavras com horário. Com `--reference` e
  `--reference-turns` calcula WER, CER e DER. `isper-cli compare` põe duas
  rodadas lado a lado; `isper-cli score` compara dois textos.
- **Corpus de regressão** (`fixtures/reuniao-sintetica.txt` +
  `cargo run -p isper-cli --bin mkfixture`): reunião sintética de 190 s com
  três falantes, transcrição e turnos de referência gerados junto. O áudio não
  é versionado — o roteiro é.
- `docs/transcription-pipeline.md`: arquitetura, parâmetros com origem
  declarada, evidência de cada decisão e como medir.

### Corrigido
- **"Participante 255"**. A causa não era o `u8`: o agrupamento do sherpa-onnx
  usa ligação completa sobre distância de cosseno, e a contagem de grupos
  cresce com a DURAÇÃO da reunião, não com o número de pessoas. Medido na
  mesma reunião de 3 falantes: 7 grupos em 3 min e **34 em 19 min** com o
  limiar antigo (0,3) — extrapolando para 2 h, ~200. Três mudanças: limiar
  passa a 0,5 (o default do próprio sherpa-onnx; o 0,3 fora calibrado numa
  fixture de duas vozes sintéticas), grupos com menos de `max(6 s, 2% da
  fala)` são absorvidos pelo vizinho temporal, e uma contagem implausível
  (>12) não é mais publicada em silêncio: vira aviso e os rótulos genéricos
  ficam. `speaker` virou `u32` — saturar em 255 transformava o sintoma em
  rótulo.
- **Palavra partida entre blocos** (`manual` → `anual`). O corte ao vivo
  procurava a janela de menor energia do último 1,5 s, sem exigir que fosse
  silêncio — e menor energia existe no meio de uma palavra. Agora o ponto
  precisa estar abaixo de 15% do volume do bloco e de um piso absoluto; sem
  silêncio, o buffer segue até 32 s e o corte forçado é contado.
- **Áudio emendado na diarização**. Só o canal dos participantes era gravado,
  sem os blocos silenciosos, colados uns nos outros — o segmentador via troca
  de voz onde havia emenda. Agora os dois canais são gravados inteiros, e a
  posição no arquivo é o instante da reunião (o que não chegou vira silêncio).
- **Falante errado quando duas pessoas falam no mesmo segmento**: a atribuição
  era por segmento do Whisper, com o falante de maior sobreposição levando a
  fala inteira. Agora é por palavra, com alisamento das trocas curtas demais
  para serem reais (o que causava troca de falante no meio de uma frase).
- `best_of` era 1 no motor; o default do whisper.cpp é 5, e com 1 o
  *temperature fallback* não tem candidato para escolher.
- Idioma "pt-br" chegava cru ao whisper.cpp, que só conhece códigos de duas
  letras — a inferência inteira falhava. Agora é reduzido a "pt".
- Os logs do whisper.cpp e do ggml passam pelo `tracing` em vez do stderr, e
  ficam em WARN por padrão: em INFO eles eram 42% do arquivo de log (carga do
  modelo, buffers, uma linha por região de fala do VAD) e enterravam o que
  interessa. `RUST_LOG=whisper_rs=info` traz tudo de volta.

### Segurança
- `rustls` 0.23.43 → 0.23.45 (RUSTSEC-2026-0285): a versão anterior aceitava
  mensagens de handshake TLS 1.3 no nível de criptografia errado. Chega até
  aqui como dependência transitiva do atualizador e do cliente HTTP; o
  handshake continua autenticado, então não dá para alterar a conexão — mas a
  correção é de graça.

### Alterado
- O motor carrega o modelo com alinhamento DTW (timestamps por token bem mais
  precisos, +37 MB de VRAM medidos com `large-v3-turbo-q5_0`).
- Os parâmetros do pipeline saíram do código para estruturas serializáveis
  (`DecodeConfig`, `VadOptions`, `WindowOptions`, `AlignOptions`,
  `ChunkOptions`, `DiarizeOptions`) — é o que o `bench` grava e edita.
- `tools/e2e/meeting.ps1` espera o passe final terminar e confere que ele
  substituiu a transcrição do ao vivo, preservando os momentos marcados.

## [0.15.0] - 2026-09-13

### Adicionado
- Fase 7.3 (segurança e confiança do binário), primeira parte:
  - **Release no GitHub Actions** (`release.yml`): as duas variantes do
    instalador compiladas em runners do GitHub (a GPU com o CUDA Toolkit
    instalado no runner), SBOM CycloneDX, `SHA256SUMS.txt`, assinatura do
    atualizador e publicação da release no push da tag — exigência da SignPath
    Foundation para a futura assinatura Authenticode, cujo job já está no
    pipeline, pulado até a aprovação. `scripts/release.ps1` vira caminho de
    reserva; `docs/RELEASE.md` descreve o processo.
  - **SBOM** (`ISPer_<v>_sbom.cdx.json`) e **somas SHA-256** publicados com
    cada release; a v0.14.0 recebeu os dois retroativamente.
  - `SECURITY.md` (relato privado de vulnerabilidade pelo GitHub, agora
    habilitado), `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md` e modelos de issue e
    PR; alertas e atualizações de segurança do Dependabot habilitados.
  - Política de assinatura de código no README, nos termos da SignPath
    Foundation (candidatura enviada em 13/09).
- Fase 7.4 (dados e observabilidade responsável):
  - **Retenção** (Configurações → Sistema → *Guardar reuniões e ditados por*):
    30, 90, 180 dias ou 1 ano — o ISPer apaga do banco e da pasta de Reuniões
    o que passou do prazo, ao abrir, uma vez por dia e ao encurtar o prazo.
    **O padrão é para sempre**, como sempre foi: nada é apagado sem que o
    usuário escolha um prazo e confirme na tela; antes de apagar, um backup
    automático do banco é gravado em `Documentos\ISPer\Backups` (ficam os três
    últimos).
  - **Backup do banco** com um clique (`Documentos\ISPer\Backups\isper-<data>.db`,
    cópia íntegra mesmo com o app aberto) e instrução de restauração.
  - **Exportar diagnóstico**: um `.zip` em `Documentos\ISPer` com o
    diagnóstico, versões (ISPer, Tauri, WebView2, Windows), `config.toml` e
    `llm.toml` (sem chaves — elas ficam no Credential Manager), métricas e
    os três últimos logs, com as linhas de texto ditado removidas.
  - **Métricas locais** no Diagnóstico: ditados e blocos de reunião dos
    últimos 30 dias — quantidade, falhas, p50/p95 da inferência e fator de
    tempo real. Ficam no banco; nada é enviado a lugar nenhum.
  - Banco com **schema versionado** (`PRAGMA user_version`, migrações em
    transação; um banco de versão mais nova é recusado em vez de alterado) e
    `config.toml` versionado (`config_version`, migração explícita).

### Alterado
- O arquivo de log (`%LOCALAPPDATA%\com.isper.desktop\logs\isper.log.<data>`)
  passa a **JSON Lines** — um objeto por linha, com `timestamp`, `level`,
  `message` e os campos do evento; filtra-se com PowerShell ou jq. O log no
  terminal continua em texto.

## [0.14.0] - 2026-09-11

### Adicionado
- Fase 7.2 (testes que provam robustez):
  - **110 testes automatizados** (eram 58): provider de IA falso para testar
    resumo, título, polimento e insights sem rede; testes *golden* das
    exportações (Markdown, SRT e DOCX comparados byte a byte —
    `ISPER_UPDATE_GOLDEN=1` regenera); testes de propriedade (`proptest`) da
    busca literal no SQLite, do corte em silêncio dos blocos e do VAD por
    energia; 19 testes no app (configuração, migração de pastas, posição do
    indicador, atalhos, atualizador) e 4 na CLI — todos no CI.
  - **Cobertura como tendência** (`coverage.yml`: cargo-llvm-cov → sumário do
    job, lcov e Coveralls, sem bloquear PR) e **smoke noturno** do app real num
    runner Windows (`e2e-nightly.yml`).
  - `tools/e2e/soak.ps1`: reunião longa medindo a memória do processo (o soak
    de 2 h do ROADMAP), e `docs/TESTES.md` com a estratégia de testes e o
    roteiro de validação manual dos casos de áudio e tela.

### Corrigido
- **Fone desconectado no meio da reunião** (ou dispositivo invalidado depois
  de uma suspensão): a captura do microfone é reaberta sozinha — cai para o
  microfone padrão — e a reunião continua; antes o canal "Eu" ficava mudo até
  o fim. No ditado, um microfone que para de entregar encerra a gravação com o
  que foi capturado, em vez de ficar presa até soltar a tecla.
- Erros de áudio do Windows vêm com a explicação em português: dispositivo em
  uso exclusivo por outro app, desconectado, formato não aceito, nenhum
  dispositivo.
- Indicador flutuante: a posição lembrada é conferida contra os monitores
  atuais ao abrir — monitor desligado ou escala/resolução trocada não deixa
  mais o indicador fora da tela.

### Alterado
- **Áudio dos participantes em disco** durante a reunião (`%TEMP%\ISPer`,
  apagado ao fim): a memória de uma reunião longa fica estável em vez de
  crescer ~115 MB por hora; a identificação de falantes lê o arquivo uma vez,
  depois.
- `clippy::unwrap_used` em todo o workspace: nenhum `unwrap()` em código de
  produção; um mutex envenenado por um pânico em outra thread não derruba mais
  o app (`lock_or_recover`).
- Configurações: uma única regra de validação (`AppConfig::normalize`) para a
  tela e para o `config.toml` editado à mão — um valor fora da lista volta ao
  padrão em vez de ser gravado; termos repetidos no dicionário somem.
- Fase 7.1 (qualidade automatizada) concluída — só engenharia, nada muda para
  quem usa o app:
  - **Versão numa só fonte**: o `tauri.conf.json` deixou de repetir a versão
    (o Tauri lê do `Cargo.toml` do app); `release.ps1` e `release.yml`
    conferem só o `Cargo.toml` e recusam um `version` que volte ao
    `tauri.conf.json`.
  - **Toolchain fixo**: `rust-toolchain.toml` (1.98.0 com rustfmt e clippy) —
    a mesma versão na máquina de quem desenvolve e no CI, que passa a ler o
    canal do arquivo; `rustfmt.toml` (estilo da edição 2024) e `.editorconfig`.
  - **App na edição 2024** (os crates já estavam): migração mecânica, sem
    mudança de comportamento; `let`-chains onde o clippy novo pediu.
  - **Dependências**: sysinfo 0.39, toml 1, dirs 6, crossbeam-channel 0.5.17
    e **ureq 3** (isper-llm e isper-models migrados: agente com timeout
    global, erros 4xx/5xx lidos com o corpo da resposta, download em
    streaming sem limite de tamanho). GitHub Actions: checkout v7 e
    gitleaks-action v3. Os PRs do Dependabot correspondentes ficam
    supersedidos.
  - **Higiene**: `loopdump.wav` (7,7 MB, captura de teste da Fase 4) saiu do
    versionamento — o arquivo local fica e o histórico não é reescrito.
  - **Branch `main` protegida** por ruleset do GitHub: sem push direto, sem
    force-push nem exclusão, histórico linear e PR obrigatório com os três
    checks do CI verdes. Toda mudança entra por branch + PR — as do
    mantenedor e as do Dependabot.

## [0.13.0] - 2026-09-10

### Adicionado
- **Detecção de chamada do Teams → "Gravar transcrição?"** (fecha o último
  item da Fase 4). O ISPer sonda as sessões de áudio do Windows a cada 4 s:
  em chamada, o Teams (ou um filho WebView2 dele) mantém o microfone aberto.
  Com histerese (~8 s para começar, ~24 s para terminar), ao detectar a
  chamada avisa por três canais — toast do Windows (clicar grava), banner na
  tela Início e aviso no indicador — e, quando a chamada termina com a
  gravação ligada, pergunta se encerra. Modos em Configurações → Reuniões:
  avisar (padrão), gravar automaticamente (e encerrar sozinho) ou não detectar.
  `isper_core::calls` (sondagem + `CallTracker` testado) e `calls.rs` no app.
- **Insights ao vivo** (Fase 5): durante a reunião, a cada 3/5/10 min os
  últimos ~15 min de transcrição vão ao provider de IA com as perguntas que
  importam enquanto ainda dá tempo — pendências, compromissos de "Eu",
  decisões, perguntas em aberto — e a resposta anterior é consolidada em vez de
  recomeçar. Painel no card de reunião do Início, com "Atualizar agora"
  (funciona como rodada avulsa mesmo com o recurso desligado). Rodadas sem
  fala nova são puladas. Opt-in em Configurações → Inteligência.
  `isper_llm::insights` (prompt testado com um `LlmProvider` falso).
- **Busca semântica** (Fase 5): reuniões e ditados viram vetores (embeddings)
  guardados no SQLite ao lado do texto (tabela `embeddings`, com o modelo que
  os gerou); a Biblioteca ganha o botão "Semântica", que acha pelo sentido
  ("quando falamos do orçamento?") e abre a reunião já rolada no trecho.
  Decisão de provider: **Gemini** (`gemini-embedding-001`, free tier, reutiliza
  a chave do Gemini) ou **qualquer endpoint compatível com OpenAI** — o que
  inclui o **Ollama local** (`nomic-embed-text`, `bge-m3`), 100% na máquina e
  sem chave. Cada reunião salva e cada ditado colado é indexado em segundo
  plano; "Indexar tudo" cobre o histórico e descarta vetores de modelos antigos.
  `isper_core::embed` (recorte em trechos, cosseno), `isper_llm::embeddings`,
  `search.rs` no app.
- **Indicador flutuante fixo**: o botão "Indicador" do Início e o item da
  bandeja agora alternam mostrar/ocultar, e em repouso o indicador fica na
  tela até você ocultá-lo (× nele, botão ou bandeja), mostrando "pronto ·
  atalho". Antes ele aparecia por 2,5 s e sumia.
- Janela de Configurações redimensionável (mínimo 480×520), como a Biblioteca.
- Fase 7 (maturidade de engenharia) no ROADMAP. Primeiro item entregue:
  `cargo clippy --workspace --all-targets -- -D warnings` no CI com
  `[workspace.lints]` compartilhado por todos os crates, `cargo deny`
  (vulnerabilidades, licenças permitidas, duplicatas, origens) com
  `deny.toml`, Dependabot (Cargo + GitHub Actions, PRs semanais agrupados) e
  gitleaks no CI.

### Alterado
- Início: o botão de gravar reunião ganhou respiro antes da lista de atalhos e
  os chips ("GPU · CUDA", "chave guardada"…) deixaram de colar no texto.
- Biblioteca: a aba Ditados ocupa a janela inteira (o painel de detalhe não
  tinha o que mostrar e a coluna de 340 px espremia o texto com barra
  horizontal); as linhas se reorganizam em janelas estreitas.
- CSP fechada para atributos de estilo: `style-src-attr` saiu da política e
  nenhum HTML usa mais `style="…"` (36 atributos e dois `style.cssText`
  virarem classes; a entrada escalonada usa `data-i`). Item 7.1 do ROADMAP.
- `MeetingStore::save_dictation` devolve o id da linha; `LlmSettings` ganha a
  seção `[embeddings]` (a CLI `llm use` a preserva).
- CSP real nas janelas do app (`default-src 'self'` + origens do IPC/asset do
  Tauri) no lugar de `csp: null`; scripts e estilos inline continuam
  funcionando porque o Tauri injeta os hashes no empacotamento. Violações
  de CSP entram em `window.__isperErrors`, que o smoke test e2e verifica em
  cada janela — um recurso bloqueado não falha mais em silêncio. Atributos
  `style="…"` seguem permitidos (`style-src-attr 'unsafe-inline'`): o Tauri
  injeta hashes em `style-src`, o que desliga o `'unsafe-inline'` dessa
  diretiva — o smoke test pegou 29 bloqueios nas Configurações antes disto.

## [0.12.2] - 2026-09-10

### Alterado
- Dados locais (modelos e logs) mudaram de `%LOCALAPPDATA%\ISPer` para
  `%LOCALAPPDATA%\com.isper.desktop`, o identificador do app. A pasta antiga
  é a pasta padrão de instalação por usuário do Tauri: quando o registro não
  aponta para uma instalação anterior, o instalador coloca o programa dentro
  da pasta de dados, ao lado dos modelos, e ficam duas cópias do ISPer na
  máquina — foi o que aconteceu. O app move a pasta antiga sozinho na primeira
  abertura e registra a mudança no log.

### Corrigido
- Causa real do "nenhum modelo instalado" visto após a atualização para a
  0.12.0: o modelo copiado durante o desenvolvimento ficou numa pasta
  virtualizada do assistente de desenvolvimento (um app empacotado, que
  redireciona as escritas em AppData), e o ISPer do usuário nunca o teve. Não
  era defeito do app; a robustez da 0.12.1 fica.

## [0.12.1] - 2026-09-10

### Alterado
- Modelos, logs, configurações e banco deixaram de depender das variáveis de
  ambiente `LOCALAPPDATA`/`APPDATA`/`USERPROFILE`: as pastas vêm da API de
  pastas conhecidas do Windows, com as variáveis como reserva; o Diagnóstico e
  o log avisam quando uma delas falta. A suspeita que motivou a mudança (um
  processo aberto sem `LOCALAPPDATA`) não se confirmou — ver 0.12.2.

## [0.12.0] - 2026-09-10

### Adicionado
- Variante **CPU** do instalador (`ISPer_<v>_x64-cpu-setup.exe`), para máquinas
  sem GPU NVIDIA, com canal próprio de atualização (`latest-cpu.json`).
- Runtime do Visual C++ (`msvcp140`, `vcruntime140`, `vcruntime140_1`) dentro
  dos instaladores: uma máquina limpa não precisa mais do redistribuível.
- Pânicos registrados no arquivo de log (mensagem, local, thread e backtrace);
  antes o processo sumia sem rastro.
- Testes ponta a ponta no repositório (`tools/e2e`): smoke, reunião com a
  fixture de duas vozes e atualizador com servidor local, todos via CDP.
- `CHANGELOG.md` e workflow `release.yml`: a tag da release precisa bater com
  os manifests e ter seção aqui; o CI roda de novo sobre a tag.

### Alterado
- `scripts/release.ps1` gera as duas variantes em `dist\v<versão>\`, usa a
  seção do CHANGELOG como notas, exige árvore do git limpa e CI verde no commit
  antes de publicar.
- As DLLs do CUDA saíram do `tauri.conf.json` base e foram para
  `tauri.gpu.conf.json`: `cargo build` de desenvolvimento não copia mais
  500 MB de DLLs a cada build.

## [0.11.1] - 2026-09-10

### Corrigido
- O app instalado pela 0.11.0 não abria: faltavam no instalador as DLLs do
  sherpa-onnx (`sherpa-onnx-c-api`, `onnxruntime`, `cargs`) que o executável
  importa. Descoberto ao instalar pelo `setup.exe` e validar a primeira
  atualização automática, que funcionou de ponta a ponta.
- Processo de publicação: checagens do GitHub sem depender do stderr, notas
  com aspas por arquivo, republicação de artefatos já gerados (`-SkipBuild`).

## [0.11.0] - 2026-09-10

### Adicionado
- Instalador NSIS com **atualização automática assinada** (minisign) a partir
  das releases do GitHub: checagem 45 s depois de abrir e uma vez por dia,
  banner no Início, "Verificar agora" em Configurações → Sistema.
- **Comandos de voz** no ditado: "nova linha", "novo parágrafo", pontuação,
  "apagar isso", "tudo em maiúsculas/minúsculas".
- **Legendas ao vivo** como modo do indicador flutuante (botão CC).
- **Momentos marcados** (Ctrl+Alt+K ou ★): seção no Markdown e no DOCX, chips
  na Biblioteca e prioridade no resumo por IA.

### Alterado
- Transcrição mais limpa: filtro de alucinações do Whisper (frases de legenda,
  loops, só símbolos, `no_speech_prob` > 0,75) e correção por semelhança com o
  dicionário pessoal, no ditado e nas reuniões.

## [0.10.0] - 2026-09-09

### Adicionado
- Notificação do Windows ao salvar a reunião (clicar abre a Biblioteca), em vez
  de abrir o arquivo; toast silencioso quando os falantes são identificados.
- Logs em arquivo com rotação diária (`%LOCALAPPDATA%\ISPer\logs`, 14 dias) e
  painel de Diagnóstico em Configurações → Sistema.
- CI no GitHub Actions (formatação, testes, checagem do app sem CUDA).

### Alterado
- Sair com reunião em andamento encerra e salva antes de fechar.
- SQLite em modo WAL com `busy_timeout`; `config.toml` gravado de forma atômica;
  áudio dos participantes guardado em 16 bits (metade da memória).
- App reorganizado em módulos por responsabilidade.

## [0.9.0] - 2026-09-09

### Adicionado
- Transcrição ao vivo no Início, título automático por IA, parágrafos por
  falante, diarização em segundo plano, renomear participantes, copiar e
  exportar (Markdown, SRT, DOCX), atalho global de reunião com ícone vermelho
  na bandeja, polimento do ditado por IA (opcional), escolha do microfone e
  captura livre de atalho.

## [0.8.0] - 2026-09-09

### Adicionado
- Indicador flutuante com prioridade acima de qualquer janela (reafirmado via
  `SetWindowPos` enquanto visível).
- Busca com destaque dentro da transcrição de uma reunião.

## [0.7.0] - 2026-09-08

### Adicionado
- Design system (`base.css`, `ui.js`): microinterações, animações que respeitam
  `prefers-reduced-motion`, fontes OFL locais.

## [0.6.0] - 2026-09-08

### Adicionado
- Tela Início: estado do motor, atalhos, reunião com cronômetro, checklist e
  reuniões recentes.

### Corrigido
- Congelamento ao abrir janelas a partir de comandos síncronos na thread
  principal (Biblioteca em branco, indicador travado).

## [0.5.0] - 2026-09-02

### Adicionado
- Primeiro instalador NSIS (por usuário) com as DLLs de runtime do CUDA.
- Loopback por processo (só o Teams), diarização com sherpa-onnx, Biblioteca
  de reuniões e ditados, indicador arrastável com modo mini.

[Unreleased]: https://github.com/Marcus-Boni/ISPer/compare/v0.15.0...HEAD
[0.15.0]: https://github.com/Marcus-Boni/ISPer/compare/v0.14.0...v0.15.0
[0.14.0]: https://github.com/Marcus-Boni/ISPer/compare/v0.13.0...v0.14.0
[0.13.0]: https://github.com/Marcus-Boni/ISPer/compare/v0.12.2...v0.13.0
[0.12.2]: https://github.com/Marcus-Boni/ISPer/compare/v0.12.1...v0.12.2
[0.12.1]: https://github.com/Marcus-Boni/ISPer/compare/v0.12.0...v0.12.1
[0.12.0]: https://github.com/Marcus-Boni/ISPer/compare/v0.11.1...v0.12.0
[0.11.1]: https://github.com/Marcus-Boni/ISPer/releases/tag/v0.11.1
