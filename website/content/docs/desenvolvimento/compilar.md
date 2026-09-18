---
title: "Compilar a partir do código"
description: "Prepare Rust, Build Tools, CMake e libclang para compilar o ISPer em CPU ou CUDA."
section: "Desenvolvimento"
order: 90
---

# Compilar a partir do código

O ISPer usa Rust, Tauri 2 e `whisper.cpp`. O frontend desktop é HTML, CSS e JavaScript embarcados; PyInstaller não participa do build.

## Pré-requisitos

- Rust pelo rustup, respeitando `rust-toolchain.toml`.
- Visual Studio Build Tools 2022 com ferramentas C++.
- CMake e libclang.
- CUDA Toolkit somente para a variante NVIDIA.

## CPU

```powershell
cargo build --release -p isper-app --no-default-features
```

## CUDA

```powershell
cargo build --release -p isper-app
```

O repositório fixa `CMAKE_CUDA_ARCHITECTURES=89` no ambiente de desenvolvimento atual. Verifique a matriz do pacote antes de distribuir para outras GPUs.
