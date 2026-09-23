# 0008 — Releases compiladas em runners do GitHub, em duas variantes

- **Status:** aceita
- **Data:** 14/09/2026

## Contexto

Até a 0.14, os instaladores eram gerados na máquina do mantenedor
(`scripts/release.ps1`). Três problemas:

- **assinatura de código:** o caminho gratuito escolhido, a SignPath
  Foundation, só assina artefatos cujos jobs rodaram todos em runners
  hospedados do GitHub;
- **reprodutibilidade:** o binário dependia do que estava instalado naquela
  máquina — e da CPU dela (ver abaixo);
- **quem não tem GPU NVIDIA** precisava de um instalador sem os ~400 MB de
  DLLs do CUDA.

## Decisão

- Uma tag `v<versão>` dispara o `release.yml`, que confere tag × `Cargo.toml`
  × seção do `CHANGELOG.md`, roda o CI inteiro e compila **duas variantes em
  runners do GitHub**: **GPU** (CUDA 13.3 instalado no runner, ~403 MB) e
  **CPU** (~9 MB). Cada uma tem o próprio manifesto do atualizador
  (`latest.json` e `latest-cpu.json`).
- A release sai com `SHA256SUMS.txt`, SBOM CycloneDX e a assinatura
  **minisign** de cada instalador, conferida pelo atualizador com a chave
  pública embutida; a chave privada vive só nos segredos do GitHub e na
  máquina do mantenedor.
- O job `sign` (Authenticode via SignPath) já existe e é pulado até as
  variáveis `SIGNPATH_*` existirem; a assinatura minisign vem depois dele,
  porque o Authenticode muda os bytes.
- O conjunto de instruções é **fixo em AVX2** (`GGML_NATIVE=OFF`).
- Publicar a release dispara o `portal-release-sync`, que abre o PR com o
  snapshot da página de download do portal.

## Consequências

- O incidente que justificou o AVX2 fixo: a 0.17.0 compilada no CI detectou
  a CPU do runner (com AVX-512) e morria com instrução ilegal (0xc000001d) na
  primeira inferência num Ryzen sem AVX-512. A 0.17.1 fixou as flags.
- Uma release leva de 30 a 60 min; a variante GPU é o gargalo (kernels CUDA).
- O `scripts/release.ps1` continua como caminho de reserva, na máquina do
  mantenedor, mas não serve para a SignPath.
- Enquanto a SignPath não aprova, o Windows mostra o aviso do SmartScreen na
  primeira execução; a minisign protege as atualizações, e o `SHA256SUMS.txt`
  permite conferir o download.

## Alternativas consideradas

- **Runner self-hosted na máquina do mantenedor** (com a GPU dele). Rápido,
  mas a SignPath não assina, e num repositório público um runner próprio
  executaria código de qualquer PR.
- **Certificado de assinatura pago** (ou Azure Trusted Signing). Custo
  recorrente, contra o custo zero.
- **Uma variante só, com CUDA.** Obrigaria quem não tem NVIDIA a baixar
  ~400 MB inúteis.

## Onde vive

`.github/workflows/release.yml`, `scripts/release-assets.ps1`,
`scripts/release.ps1`, `.cargo/config.toml` (flags do ggml),
`apps/isper-app/src-tauri/tauri.{gpu,cpu}.conf.json`,
[`docs/RELEASE.md`](../RELEASE.md).
