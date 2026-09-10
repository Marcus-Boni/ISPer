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
  src-tauri/src/     # main.rs (bootstrap) + módulos por responsabilidade: state,
                     # shortcuts, tray, dictation, meetings, views, overlay,
                     # settings, library, home, notify, config (prelude reexporta)
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
hora, sem reiniciar — escolha na lista ou clique em **"Gravar atalho"** e
pressione a combinação que quiser), **microfone** (vale para o ditado e para
o canal "Eu" das reuniões; se desconectar, cai para o padrão), idioma,
**dicionário pessoal** (termos que o Whisper deve grafar certo — viram o
`initial_prompt`), provider de IA com chave e teste de conexão, e iniciar
com o Windows. Cada ditado também fica no histórico (`%APPDATA%\ISPer\isper.db`,
tabela `dictations`).

**Comandos de voz** (ligados por padrão; Configurações → Ditado): diga
*nova linha*, *novo parágrafo*, *ponto final*, *vírgula*, *ponto de
interrogação*, *ponto de exclamação*, *dois pontos*, *ponto e vírgula*,
*reticências*, *abre/fecha parênteses*, *abre/fecha aspas*, *travessão* ou
*arroba* e o ISPer insere o símbolo, arruma o espaçamento e a maiúscula
seguinte. No fim do ditado, *apagar isso* descarta tudo (nada é colado) e
*tudo em maiúsculas* / *tudo em minúsculas* muda a caixa. É processamento
de texto local, casado por palavra inteira e sem diferenciar acentos.

**Qualidade da transcrição**: o motor suprime tokens que não são fala e filtra
as alucinações clássicas do Whisper — "Legendas pela comunidade", loops de uma
palavra repetida, trechos só de símbolos e segmentos que o próprio modelo
marca como "não é fala" (probabilidade > 0,75). O dicionário pessoal, além de
orientar o modelo, corrige por semelhança o que ele ainda errar ("ísper" →
"ISPer", "opt solve" → "OptSolv"), no ditado e nas reuniões.

**Polimento por IA (opcional)**: em Configurações → Inteligência, "Polir os
ditados com IA antes de colar" tira hesitações ("é", "hã", "tipo"),
repetições e arruma pontuação usando o provider configurado — estilo "só
limpeza", formal ou casual. Só o texto do ditado é enviado; custa cerca de um
segundo; se a API falhar ou não houver chave, o original é colado
normalmente, e o histórico guarda os dois (selo "IA" na Biblioteca).

### Notetaker de reuniões (Fase 4)

No app: bandeja → **"Iniciar gravação de reunião"**, o botão da tela Início
ou o **atalho global de reunião** (Ctrl+Alt+M por padrão; configurável). O
ícone da bandeja ganha um **ponto vermelho** enquanto grava. O pill mostra o
estado; o mesmo caminho encerra ("Encerrar e transcrever a reunião") — o
transcript abre sozinho e fica salvo em `Documentos\ISPer\Reunioes\*.md` +
SQLite em `%APPDATA%\ISPer\isper.db`. Mic = "Eu"; áudio do sistema
(loopback) = "Participantes". Avise os participantes (LGPD).

**Ao salvar**: por padrão o ISPer mostra uma **notificação do Windows**
("Reunião salva — título · duração · resumo pronto"); clicar nela abre a
Biblioteca já naquela reunião. Em Configurações → Reuniões dá para trocar por
"abrir o arquivo .md" (comportamento antigo) ou "nada". Quando a
identificação de falantes termina, chega uma segunda notificação, silenciosa.
O ISPer registra o próprio nome e ícone para notificações no registro do
usuário (`HKCU\Software\Classes\AppUserModelId\com.isper.desktop`) — por isso
funciona mesmo rodando o `.exe` sem instalador. "Testar notificação" nas
Configurações mostra uma de exemplo.

**Sair com reunião em andamento** (bandeja → Sair) encerra e salva a reunião
antes de fechar — nada se perde.

**Ao vivo**: cada bloco de ~20 s é transcrito durante a reunião — as falas
aparecem no card de reunião da tela Início conforme chegam, e a última fala
passa pelo indicador flutuante. Tudo local; nenhum áudio ou texto sai da
máquina nessa etapa.

**Título automático**: com um provider de IA configurado, o resumo pós-reunião
vem junto com um título curto do assunto ("Planejamento PCP da semana 37" em
vez de "Reunião — data"); o `.md` é regravado inteiro com título e resumo.

**Parágrafos legíveis**: falas consecutivas do mesmo falante são agrupadas
só enquanto a pausa entre elas for menor que 4 s e o parágrafo não passar de
60 s — cada parágrafo mantém o horário. Vale para o `.md`, o DOCX e a
Biblioteca.

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

**Legendas ao vivo**: o botão **CC** do indicador (ou o interruptor
"Legendas no indicador" na tela Início, durante a reunião) troca o indicador
para uma barra larga que mostra as duas últimas falas transcritas, com o
falante — útil para acompanhar uma reunião sem ficar na tela Início. Volta ao
normal pelo mesmo botão; a preferência é lembrada.

**Momentos marcados**: durante a reunião, **Ctrl+Alt+K** (configurável), o
botão **★** do indicador ou o "★ Marcar momento" da tela Início marcam o
instante atual — para "isso é importante, quero voltar aqui". Os momentos
viram a seção "Momentos marcados" do Markdown e do DOCX (com o trecho da fala
em curso), chips clicáveis na Biblioteca que rolam até a fala destacada, e
o resumo por IA dá prioridade a esses trechos. Dois toques em menos de 1,5 s
contam como um.

**Buscar dentro de uma reunião**: com a reunião aberta, a barra "buscar nesta
reunião" (ou Ctrl+F) destaca cada ocorrência no resumo e no transcript — sem
diferenciar maiúsculas nem acentos —, com contador, Enter/Shift+Enter para
navegar e "Só trechos" para ver apenas as falas que contêm o termo. Se a
reunião apareceu por causa da busca geral, o termo já vem destacado.

**Nomear participantes**: clique no nome de um falante no transcript
("Participante 1") e digite o nome real — vale para toda a reunião, o `.md`
é regravado e a cor do falante se mantém. (Reconhecer a mesma voz em reuniões
futuras fica para depois.)

**Copiar e exportar**: botões "Copiar resumo" e "Copiar transcript" (texto
puro com horários), e **Exportar SRT** (legendas) ou **DOCX** (Word) — o
arquivo é gravado ao lado do `.md` e mostrado no Explorer.

**Só o Teams**: em Configurações → Reuniões, escolha "Só o Microsoft Teams" —
o ISPer usa o *process loopback* do Windows e ignora notificações, músicas e
outros apps (se o Teams não estiver aberto, cai para o sistema e avisa).

**Quem falou o quê**: Configurações → Reuniões → "Baixar modelos (~45 MB)"
instala pyannote + 3D-Speaker (via sherpa-onnx, 100% local). A reunião é
salva e aberta **na hora** com "Participantes"; a identificação roda **em
segundo plano** (na CPU ela leva cerca de 40% da duração da reunião — o
sherpa-onnx usa uma thread só) e, ao terminar, "Participantes" vira
"Participante 1", "Participante 2"… no banco, na Biblioteca e no `.md`; o
Início mostra "identificando falantes…" na reunião enquanto isso. O número
de falantes é descoberto por agrupamento; se juntar ou separar demais,
calibre o threshold sem recompilar: `ISPER_DIARIZE_THRESHOLD=0.2` (menor =
mais falantes distintos; padrão 0.3). Para calibrar offline sem regravar:
`isper-cli diarize fixtures/duas-vozes-16k.wav`. Fechar o ISPer no meio
cancela a identificação daquela reunião (os rótulos genéricos ficam).

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

## Logs e diagnóstico

O app grava logs em `%LOCALAPPDATA%\ISPer\logs\isper.log.<data>` (um arquivo
por dia, 14 dias guardados) além do stdout. Configurações → Sistema →
**Diagnóstico** lista versão, motor, modelo, DLLs do CUDA, microfones e
caminhos, com "Copiar diagnóstico" e "Abrir pasta de logs" — é o que mandar
ao pedir ajuda.

## Testes e CI

```bash
cargo test --release -p isper-core --features cuda
```

```bash
cargo test --release -p isper-llm
```

(`--release` reaproveita o whisper.cpp já compilado; no perfil debug o
`cargo test` recompila o whisper.cpp + CUDA do zero, o que leva minutos.)
O GitHub Actions ([`.github/workflows/ci.yml`](.github/workflows/ci.yml)) roda
formatação, os testes dos crates e um `cargo check` do app sem CUDA a cada
push.

## Licença

[MIT](LICENSE). Whisper (MIT) · whisper.cpp (MIT) · Tauri (MIT/Apache-2.0) —
todos os modelos usados têm pesos abertos.
