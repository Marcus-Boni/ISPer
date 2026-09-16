---
title: "CUDA e desempenho"
description: "Escolha a variante correta e diagnostique aceleração NVIDIA, driver e uso de VRAM."
section: "Solução de Problemas"
order: 82
---

# CUDA e desempenho

Use o instalador CUDA somente em uma máquina com GPU NVIDIA compatível. Em outros computadores, instale a variante CPU.

## O aplicativo não abre

1. Confirme que baixou o arquivo `ISPer_0.15.0_x64-setup.exe`.
2. Atualize o driver NVIDIA.
3. Abra **Configurações > Sistema > Diagnóstico** e confira as DLLs CUDA detectadas.
4. Se o problema continuar, instale a variante CPU para separar falha de driver de falha do aplicativo.

## Modelo e VRAM

O Large v3 Turbo q5 é a escolha recomendada com GPU no catálogo atual. Se houver falta de memória, use Small ou Medium q5.

> [!NOTE]
> O DirectML não é um backend distribuído nesta versão. Em GPU AMD, use a variante CPU.
