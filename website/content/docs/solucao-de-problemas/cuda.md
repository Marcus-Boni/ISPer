---
title: "CUDA e desempenho"
description: "Escolha a variante correta e diagnostique aceleração NVIDIA, driver e uso de VRAM."
section: "Solução de Problemas"
order: 82
---

# CUDA e desempenho

A variante CUDA desta versão roda em placas NVIDIA GeForce RTX 40 e RTX 50 (e nas RTX Ada e PRO). Em qualquer outro computador, inclusive com RTX 20 ou RTX 30, instale a variante CPU.

## O aplicativo fecha na primeira transcrição

Se o ISPer abre, mas fecha quando você começa a ditar ou a gravar uma reunião, a placa não roda o código CUDA desta versão: é o caso das RTX 20 e RTX 30. Instale a variante CPU, que funciona em qualquer PC com AVX2.

## O aplicativo não abre

1. Confirme que baixou a variante CUDA — o arquivo termina em `_x64-setup.exe`. Se terminar em `_x64-cpu-setup.exe`, é a variante CPU.
2. Atualize o driver NVIDIA.
3. Abra **Configurações > Sistema > Diagnóstico** e confira as DLLs CUDA detectadas.
4. Se o problema continuar, instale a variante CPU para separar falha de driver de falha do aplicativo.

## Modelo e VRAM

O Large v3 Turbo q5 é a escolha recomendada com GPU no catálogo atual. Se houver falta de memória, use Small ou Medium q5.

> [!NOTE/Observação]
> O DirectML não é um backend distribuído nesta versão. Em GPU AMD, use a variante CPU.
