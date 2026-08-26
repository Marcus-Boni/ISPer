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

O whisper.cpp é compilado pelo CMake sem SIMD por padrão no MSVC (fica ~5× mais
lento). O build.rs do `whisper-rs-sys` repassa qualquer env `GGML_*` como flag
do CMake — então habilite AVX2 antes de compilar:

```bash
set GGML_AVX2=ON& set GGML_FMA=ON& set GGML_F16C=ON& set GGML_BMI2=ON& cargo build --release
```

(No PowerShell: `$env:GGML_AVX2='ON'` etc. Se mudar essas flags, rode antes
`cargo clean -p whisper-rs-sys --release` para forçar a recompilação do C++.)

Na Fase 3, a feature `cuda` do whisper-rs ativa a RTX 4050 (requer CUDA Toolkit).

## Modelo

```bash
curl -L -o models/ggml-small.bin https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin
```

Outros modelos (large-v3-turbo etc.): https://huggingface.co/ggerganov/whisper.cpp

## Uso

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
