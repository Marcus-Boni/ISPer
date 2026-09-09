# ISPer

Ditado por voz 100% local (estilo Wispr Flow) em Rust + whisper.cpp, evoluindo para
notetaker de reuniões do Teams. Roadmap completo em [ROADMAP.md](ROADMAP.md).

## Estrutura

```
crates/isper-core/   # motor: captura (cpal) → resample (rubato) → Whisper (whisper-rs)
crates/isper-cli/    # laboratório da Fase 1: transcrição no terminal
crates/isper-models/ # catálogo e download de modelos (SHA-256 do Hugging Face)
crates/isper-llm/    # providers de IA (Groq, Gemini, Claude) e resumo pós-reunião
crates/isper-diarize/# quem falou o quê (sherpa-onnx: pyannote + 3D-Speaker)
apps/isper-app/      # app Tauri 2: src-tauri (Rust) + ui (HTML/CSS/JS sem build step)
  ui/assets/         # design system: base.css (tokens, componentes, movimento),
                     # ui.js (toast, count-up, confirmação inline…) e fontes OFL locais
models/              # modelos ggml (gitignored — baixar, ver abaixo)
fixtures/            # WAVs de teste gerados com TTS do Windows (voz pt-BR Maria)
```

A interface é vanilla e offline: as fontes (Fraunces e Hanken Grotesk, licença
OFL) vão dentro do app — nada é baixado em tempo de execução. Animações usam só
`transform`/`opacity` e respeitam `prefers-reduced-motion` do Windows
("Efeitos de animação" desligados → interface estática).

## Pré-requisitos de build (Windows)

| Ferramenta | Como foi instalado | Observação |
|---|---|---|
| Rust (rustup) | `rustup-init.exe -y` | toolchain `stable-x86_64-pc-windows-msvc` |
| VS Build Tools 2022 | `winget install Microsoft.VisualStudio.2022.BuildTools` + workload VCTools | compila o whisper.cpp (C++) |
| CMake | zip portátil em `%LOCALAPPDATA%\Programs\cmake-*\bin` (no PATH de usuário) | exigido pelo whisper-rs-sys |
| libclang | `pip install --user libclang` | exigido pelo bindgen; `LIBCLANG_PATH` aponta p/ `%APPDATA%\Python\Python311\site-packages\clang\native` (persistido como env var de usuário) |

## Build

Basta:

```bash
cargo build --release
```

CUDA vem **ligado por padrão** (`default = ["cuda"]` em `isper-cli` e
`isper-app`); sem GPU NVIDIA use `--no-default-features`. As flags de
otimização e SIMD ficam em [`.cargo/config.toml`](.cargo/config.toml) — o
cargo as aplica automaticamente ao build script, de qualquer shell.

**Por que elas existem (pegadinha crítica no Windows/MSVC):** o crate `cmake`
engole os flags de otimização do modo Release — sem intervenção, o whisper.cpp
sai compilado **sem `/O2`** (equivale a `/Od`, ~13× mais lento). O build.rs
do `whisper-rs-sys` repassa qualquer env `GGML_*` ou `CMAKE_*` ao CMake, e é
isso que o `.cargo/config.toml` explora. Se mudar essas flags, rode antes
`cargo clean -p whisper-rs-sys --release`; para conferir os flags usados:
`Select-String -Path target\release\build\whisper-rs-sys-*\output -Pattern 'CL\.exe /c'`.

### Build com CUDA (Fase 3)

Requer o CUDA Toolkit (`winget install Nvidia.CUDA`). Depois:

```bash
cargo build --release
```

(Num shell aberto **antes** da instalação do CUDA, exporte
`CUDA_PATH` e `CUDA_PATH_V13_3` apontando para o toolkit — shells novos já
os recebem do instalador. `CMAKE_CUDA_ARCHITECTURES=89` — só a arquitetura da
RTX 4050 — já está no `.cargo/config.toml` e corta MUITO o tempo de build.)

Pegadinhas encontradas:
- Erro `The CUDA Toolkit directory '' does not exist` → o MSBuild não achou
  `CUDA_PATH_V13_3` no ambiente (shells abertos antes da instalação não têm
  a variável) — exporte-a como acima.
- O instalador do CUDA pode não copiar a `Nvda.Build.CudaTasks.v13.3.dll` —
  copie (como admin) os arquivos de
  `<toolkit>\extras\visual_studio_integration\MSBuildExtensions\` para
  `<BuildTools>\MSBuild\Microsoft\VC\v170\BuildCustomizations\`.

- As DLLs de runtime do CUDA 13 (`cudart64_13.dll`, `cublas64_13.dll`…) ficam
  em `<toolkit>\bin\x64` — o instalador põe no PATH de máquina, mas shells
  abertos antes da instalação precisam adicionar o caminho manualmente
  (sintoma: o exe morre na hora com STATUS_DLL_NOT_FOUND, sem mensagem).
- O cargo não consegue substituir `isper-app.exe` com o app aberto
  (`Acesso negado`) — feche pelo tray antes de rebuildar.

Com CUDA, o app prefere `models/ggml-large-v3-turbo-q5_0.bin` automaticamente.
Medido na RTX 4050: 10,4 s de áudio transcritos em 0,6 s (16× tempo real).

## Modelos

O ISPer tem um **gerenciador de modelos** (crate `isper-models`): catálogo,
download com progresso e **SHA-256 verificado contra o publicado no Hugging
Face**, tudo em `%LOCALAPPDATA%\ISPer\models`. Sem nenhum modelo instalado, o
app abre as Configurações sozinho para você baixar um.

```bash
cargo run --release -p isper-cli -- models list
```

```bash
cargo run --release -p isper-cli -- models download ggml-large-v3-turbo-q5_0.bin
```

No app: Configurações → **Modelos Whisper** (baixar, remover e escolher; a
troca é a quente). Em desenvolvimento, arquivos em `models/` na raiz do
repositório também são reconhecidos.

## Uso

### App de ditado (Fase 2)

```bash
cargo run --release -p isper-app
```

O app fica na **bandeja do sistema** e abre com a **tela Início**: uma janela
central com o estado do motor (modelo carregado, GPU, atalho ativo), fonte de
áudio das reuniões, diarização e IA, o botão de gravar reunião (com
cronômetro), um checklist do que falta ou é opcional configurar, totais e as
reuniões recentes (clique abre a Biblioteca já na reunião). Ela volta com um
clique esquerdo no ícone da bandeja ou clicando de novo no atalho do ISPer;
no início junto com o Windows o app nasce quieto na bandeja. Para não abri-la
no lançamento manual, desmarque "Mostrar esta tela ao abrir" no rodapé (ou em
Configurações → Sistema).

O atalho global é Ctrl+Alt+Espaço, ou o
primeiro livre entre Ctrl+Shift+Espaço / Ctrl+Alt+D / Ctrl+Alt+I — a dica no
menu da bandeja mostra qual foi registrado. Dois modos:

- **Push-to-talk**: segure o atalho, fale, solte → o texto é colado no app
  focado via Ctrl+V (o clipboard anterior é restaurado em seguida).
- **Mãos-livres**: toque rápido no atalho, fale à vontade → ~1,2 s de
  silêncio (ou um segundo toque) encerra e cola sozinho.

Requer `models/ggml-small.bin` (ou a env `ISPER_MODEL` apontando para outro
modelo ggml).

Na bandeja, **"Configurações…"** abre a tela com: atalho global (trocado na
hora, sem reiniciar), idioma, **dicionário pessoal** (termos que o Whisper
deve grafar certo — viram o `initial_prompt`), provider de IA com chave e
teste de conexão, e iniciar com o Windows. Cada ditado também fica no
histórico (`%APPDATA%\ISPer\isper.db`, tabela `dictations`).

### Notetaker de reuniões (Fase 4)

No app: bandeja → **"Iniciar gravação de reunião"**. O pill mostra o estado;
o mesmo menu encerra ("Encerrar e transcrever a reunião") — o transcript
abre sozinho e fica salvo em `Documentos\ISPer\Reunioes\*.md` + SQLite em
`%APPDATA%\ISPer\isper.db`. Mic = "Eu"; áudio do sistema (loopback) =
"Participantes". Avise os participantes (LGPD).

**Biblioteca**: bandeja → "Biblioteca de reuniões…" (ou o botão na tela
Início) lista todas as
reuniões com busca no título, resumo e transcript; cada uma abre com resumo,
transcript por falante, renomear, abrir o `.md` e excluir do histórico (o
arquivo fica). A aba Ditados mostra o histórico do que você ditou.

**Indicador flutuante**: arraste-o para onde quiser (a posição é lembrada);
passe o mouse para ver `–` (modo mini: só o ponto + cronômetro da reunião) e
`×` (ocultar — volta em bandeja → "Mostrar indicador flutuante"). Enquanto
visível ele fica **acima de qualquer janela**, inclusive de outras "sempre no
topo" (Teams em chamada, players): o ISPer reafirma essa prioridade ao
mostrá-lo e a cada 1,5 s, sem roubar o foco do que você está usando.

**Buscar dentro de uma reunião**: com a reunião aberta, a barra "buscar nesta
reunião" (ou Ctrl+F) destaca cada ocorrência no resumo e no transcript — sem
diferenciar maiúsculas nem acentos —, com contador, Enter/Shift+Enter para
navegar e "Só trechos" para ver apenas as falas que contêm o termo. Se a
reunião apareceu por causa da busca geral, o termo já vem destacado.

**Só o Teams**: em Configurações → Reuniões, escolha "Só o Microsoft Teams" —
o ISPer usa o *process loopback* do Windows e ignora notificações, músicas e
outros apps (se o Teams não estiver aberto, cai para o sistema e avisa).

**Quem falou o quê**: Configurações → Reuniões → "Baixar modelos (~45 MB)"
instala pyannote + 3D-Speaker (via sherpa-onnx, 100% local); ao encerrar a
reunião, "Participantes" vira "Participante 1", "Participante 2"… O número de
falantes é descoberto por agrupamento; se juntar ou separar demais, calibre
o threshold sem recompilar: `ISPER_DIARIZE_THRESHOLD=0.2` (menor = mais
falantes distintos; padrão 0.3). Para calibrar offline sem regravar:
`isper-cli diarize fixtures/duas-vozes-16k.wav`.

Na CLI (laboratório):

```bash
cargo run --release -p isper-cli -- meeting 30 --source teams
```

```bash
cargo run --release -p isper-cli -- models download-diarize
```

Depuração do loopback (taxa de entrega por segundo + WAV):

```bash
cargo run --release -p isper-cli --bin loopdump -- 15
```

### Inteligência de nuvem (Fase 5)

Ao fim de cada reunião, o ISPer pode gerar **resumo, pontos principais,
action items e decisões** via API de LLM — anexados ao Markdown e ao banco.
Configure uma vez:

```bash
cargo run --release -p isper-cli -- llm use groq
```

```bash
cargo run --release -p isper-cli -- llm set-key groq
```

```bash
cargo run --release -p isper-cli -- llm test
```

Providers: `groq` (chave gratuita em console.groq.com/keys), `gemini`
(aistudio.google.com/apikey) e `claude` (console.anthropic.com; usa
`claude-opus-5` por padrão). Os catálogos de modelos mudam rápido e variam
por conta — liste os que a SUA chave enxerga com `llm models` (ou o botão
"Listar modelos" nas Configurações) e escolha com
`llm use <provider> --model <id>`. A chave fica no **Credential Manager do
Windows** — nunca em arquivo. Privacidade: só o TEXTO do transcript é
enviado; o áudio nunca sai da máquina. Sem provider configurado, tudo
funciona normalmente — apenas sem resumo.

### CLI (Fase 1)

```bash
cargo run --release -p isper-cli -- rec 5
```

```bash
cargo run --release -p isper-cli -- file fixtures/fala-16k.wav
```

Opções: `--model <caminho>` (padrão `models/ggml-small.bin`), `--lang <pt|en|auto>`.

## Instalador (.exe / .msi)

```bash
cd apps/isper-app && npx --yes @tauri-apps/cli@latest build
```

Gera `target/release/bundle/nsis/ISPer_<versão>_x64-setup.exe` (~398 MB;
instalação por usuário, sem UAC) e, com `--bundles msi`, o
`bundle/msi/ISPer_<versão>_x64_en-US.msi` (o bundler baixa NSIS/WiX do GitHub
na primeira vez). As DLLs de runtime do CUDA (`cudart`, `cublas`, `cublasLt`
— ~500 MB, o `cublasLt` sozinho tem 442 MB) vão empacotadas: copie-as de
`<CUDA>\bin\x64` para `apps/isper-app/src-tauri/resources/cuda/` antes de
gerar (a pasta é gitignored). **Feche o ISPer antes de gerar** (o bundler
reescreve o exe). O instalador não traz modelos: no primeiro uso o app abre as
Configurações para baixar um.

## Testes

```bash
cargo test -p isper-core
```

## Licença

[MIT](LICENSE). Whisper (MIT) · whisper.cpp (MIT) · Tauri (MIT/Apache-2.0) —
todos os modelos usados têm pesos abertos.
