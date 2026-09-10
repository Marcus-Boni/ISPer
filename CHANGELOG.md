# Changelog

Todas as mudanças relevantes do ISPer ficam aqui. O formato segue o
[Keep a Changelog](https://keepachangelog.com/pt-BR/1.1.0/) e as versões, o
[Versionamento Semântico](https://semver.org/lang/pt-BR/): MAJOR para quebras,
MINOR para funcionalidades, PATCH para correções. A versão vive em
`apps/isper-app/src-tauri/Cargo.toml` e em `tauri.conf.json`; a seção
`## [versão]` daqui vira as notas da release (`scripts/release.ps1`), e o
workflow `release.yml` recusa uma tag que não bata com os dois.

## [Unreleased]

### Adicionado
- Fase 7 (maturidade de engenharia) no ROADMAP. Primeiro item entregue:
  `cargo clippy --workspace --all-targets -- -D warnings` no CI com
  `[workspace.lints]` compartilhado por todos os crates, `cargo deny`
  (vulnerabilidades, licenças permitidas, duplicatas, origens) com
  `deny.toml`, Dependabot (Cargo + GitHub Actions, PRs semanais agrupados) e
  gitleaks no CI.

### Alterado
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

[Unreleased]: https://github.com/Marcus-Boni/ISPer/compare/v0.12.2...HEAD
[0.12.2]: https://github.com/Marcus-Boni/ISPer/compare/v0.12.1...v0.12.2
[0.12.1]: https://github.com/Marcus-Boni/ISPer/compare/v0.12.0...v0.12.1
[0.12.0]: https://github.com/Marcus-Boni/ISPer/compare/v0.11.1...v0.12.0
[0.11.1]: https://github.com/Marcus-Boni/ISPer/releases/tag/v0.11.1
