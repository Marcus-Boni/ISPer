# ISPer

Ditado por voz 100% local (estilo Wispr Flow) em Rust + whisper.cpp, evoluindo para
notetaker de reuniões do Teams. Roadmap completo em [ROADMAP.md](ROADMAP.md).

## Estrutura

```
crates/isper-core/   # motor: captura (cpal) → resample (rubato) → Whisper (whisper-rs)
crates/isper-cli/    # laboratório da Fase 1: transcrição no terminal
models/              # modelos ggml (gitignored — baixar, ver abaixo)
fixtures/            # WAVs de teste gerados com TTS do Windows (voz pt-BR Maria)
```

## Pré-requisitos de build (Windows)

| Ferramenta | Como foi instalado | Observação |
|---|---|---|
| Rust (rustup) | `rustup-init.exe -y` | toolchain `stable-x86_64-pc-windows-msvc` |
| VS Build Tools 2022 | `winget install Microsoft.VisualStudio.2022.BuildTools` + workload VCTools | compila o whisper.cpp (C++) |
| CMake | zip portátil em `%LOCALAPPDATA%\Programs\cmake-*\bin` (no PATH de usuário) | exigido pelo whisper-rs-sys |
| libclang | `pip install --user libclang` | exigido pelo bindgen; `LIBCLANG_PATH` aponta p/ `%APPDATA%\Python\Python311\site-packages\clang\native` (persistido como env var de usuário) |

## Build

**Pegadinha crítica no Windows/MSVC:** o crate `cmake` engole os flags de
otimização do modo Release — o whisper.cpp acaba compilado **sem `/O2`**
(equivale a `/Od`, ~5× mais lento). O build.rs do `whisper-rs-sys` repassa
qualquer env `GGML_*` ou `CMAKE_*` como flag do CMake, então force otimização
e SIMD antes de compilar (PowerShell):

```powershell
$env:GGML_AVX2='ON'; $env:GGML_FMA='ON'; $env:GGML_F16C='ON'; $env:GGML_BMI2='ON'
$env:CMAKE_C_FLAGS_RELEASE='/O2 /Ob2 /DNDEBUG'; $env:CMAKE_CXX_FLAGS_RELEASE='/O2 /Ob2 /DNDEBUG'
cargo build --release
```

(Se mudar essas flags, rode antes `cargo clean -p whisper-rs-sys --release`
para forçar a recompilação do C++. Para conferir os flags usados:
`Select-String -Path target\release\build\whisper-rs-sys-*\output -Pattern 'CL\.exe /c'`.)

### Build com CUDA (Fase 3)

Requer o CUDA Toolkit (`winget install Nvidia.CUDA`). Depois:

```powershell
$env:CUDA_PATH = 'C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v13.3'
$env:CUDA_PATH_V13_3 = $env:CUDA_PATH   # num shell aberto antes da instalação
$env:CMAKE_CUDA_ARCHITECTURES = '89'    # só a arquitetura da RTX 4050 (Ada) — corta MUITO o tempo
cargo build --release -p isper-app -p isper-cli --features "isper-app/cuda,isper-cli/cuda"
```

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

## Modelo

```bash
curl -L -o models/ggml-small.bin https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin
```

Outros modelos (large-v3-turbo etc.): https://huggingface.co/ggerganov/whisper.cpp

## Uso

### App de ditado (Fase 2)

```bash
cargo run --release -p isper-app
```

O app fica na **bandeja do sistema**. O atalho global é Ctrl+Alt+Espaço, ou o
primeiro livre entre Ctrl+Shift+Espaço / Ctrl+Alt+D / Ctrl+Alt+I — a dica no
menu da bandeja mostra qual foi registrado. Dois modos:

- **Push-to-talk**: segure o atalho, fale, solte → o texto é colado no app
  focado via Ctrl+V (o clipboard anterior é restaurado em seguida).
- **Mãos-livres**: toque rápido no atalho, fale à vontade → ~1,2 s de
  silêncio (ou um segundo toque) encerra e cola sozinho.

Requer `models/ggml-small.bin` (ou a env `ISPER_MODEL` apontando para outro
modelo ggml).

### CLI (Fase 1)

```bash
cargo run --release -p isper-cli -- rec 5
```

```bash
cargo run --release -p isper-cli -- file fixtures/fala-16k.wav
```

Opções: `--model <caminho>` (padrão `models/ggml-small.bin`), `--lang <pt|en|auto>`.

## Testes

```bash
cargo test -p isper-core
```
