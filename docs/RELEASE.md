# Release do ISPer — como uma versão sai

Duas variantes de instalador (GPU/CUDA e CPU), assinatura do atualizador,
SBOM, somas SHA-256 e, quando a SignPath Foundation aprovar, assinatura
Authenticode. Este documento diz quem faz o quê e por quê; o
[CHANGELOG](../CHANGELOG.md) diz o que mudou em cada versão.

## Caminho principal: o GitHub Actions

1. Bump de versão por PR: `apps/isper-app/src-tauri/Cargo.toml` (fonte
   única; o `tauri.conf.json` não repete), `Cargo.lock` (`cargo metadata`) e a
   seção `## [<versão>] - <data>` no `CHANGELOG.md`. O `[Unreleased]` volta a
   ficar vazio.
2. Merge em `main` com os três checks verdes.
3. Tag e push:

```bash
git tag v0.15.0 && git push origin v0.15.0
```

4. O workflow [`release.yml`](../.github/workflows/release.yml) faz o resto:

| Job | O que faz | Onde |
|---|---|---|
| `consistencia` | tag × Cargo.toml × seção do CHANGELOG | windows-latest |
| `ci` | a bateria do CI (fmt, testes, clippy, deny, gitleaks) | ci.yml |
| `build` (cpu, gpu) | `tauri build` de cada variante; a GPU instala o CUDA Toolkit 13.3 no runner (Jimver/cuda-toolkit, sub-pacotes nvcc/nvvm/crt/cudart/cublas/thrust) e compila o whisper.cpp com Ninja no ambiente do MSVC; as DLLs do sherpa-onnx vêm do pré-compilado que o `sherpa-rs` baixa e o runtime do VC vem do Visual Studio do runner | cpu: windows-latest · gpu: windows-2022 (o CUDA 13.3 não suporta o Visual Studio 2026 do windows-latest; o 13.4 é o primeiro que suporta) |
| `sbom` | `cargo cyclonedx` do app, filtrado para Windows | ubuntu-latest |
| `sign` | assinatura Authenticode via SignPath — pulado até a aprovação | ubuntu-latest |
| `publish` | `scripts/release-assets.ps1` (assinatura minisign, `latest*.json`, `SHA256SUMS.txt`, SBOM) e `gh release create` | windows-latest |

Um `workflow_dispatch` sem marcar "publish" é um **ensaio**: compila as duas
variantes e guarda os artefatos por 30 dias, sem tocar nas releases.

Por que runners hospedados, e não um runner na máquina do mantenedor: a
SignPath Foundation só assina artefatos cujos jobs rodaram todos em runners do
GitHub, e um runner self-hosted num repositório público executaria código de
qualquer PR. O custo é o tempo: a variante GPU compila os kernels CUDA do zero
na primeira vez (o `rust-cache` guarda o `target/` entre releases).

## Caminho de reserva: a máquina do mantenedor

`scripts/release.ps1` faz o mesmo localmente (precisa do toolkit CUDA, do
Node e do `cargo-cyclonedx`), e `-Publish` cria a release. A tag dispara o
`release.yml` mesmo assim; o job `publish` vê a release existente e só anexa
o SBOM, se faltar — nunca substitui instaladores já publicados, porque o
`latest.json` e as instalações por aí apontam para eles.

```powershell
.\scripts\release.ps1            # gera dist\v<versão>\ (GPU + CPU)
.\scripts\release.ps1 -Publish   # e publica
```

## Segredos e variáveis

| Nome | Tipo | Para quê |
|---|---|---|
| `TAURI_SIGNING_PRIVATE_KEY` | segredo | chave minisign do atualizador (o conteúdo de `%USERPROFILE%\.tauri\isper.key`); só o job `publish` a recebe |
| `SIGNPATH_API_TOKEN` | segredo | token da API da SignPath |
| `SIGNPATH_ORGANIZATION_ID`, `SIGNPATH_PROJECT_SLUG`, `SIGNPATH_POLICY_SLUG` | variáveis | identificam a organização, o projeto e a política de assinatura na SignPath; enquanto não existem, o job `sign` é pulado |

A chave privada do atualizador **nunca** entra no repositório. Quem a cadastra
como segredo é o mantenedor, pela interface do GitHub ou por
`gh secret set TAURI_SIGNING_PRIVATE_KEY < %USERPROFILE%\.tauri\isper.key`.

## O que sai em cada release

- `ISPer_<v>_x64-setup.exe` (GPU, ~400 MB) e `ISPer_<v>_x64-cpu-setup.exe`
  (CPU, ~9 MB), com `.sig` (assinatura minisign que o atualizador confere).
- `latest.json` e `latest-cpu.json`: os manifests que o app consulta.
- `ISPer_<v>_sbom.cdx.json`: SBOM CycloneDX 1.5 do app — cada crate e
  versão que entra no binário. Serve para responder "essa vulnerabilidade
  upstream me afeta?" sem abrir o `Cargo.lock`.
- `SHA256SUMS.txt`: somas de todos os arquivos acima. Para conferir um
  download no PowerShell:

```powershell
(Get-FileHash .\ISPer_0.15.0_x64-cpu-setup.exe -Algorithm SHA256).Hash
```

## Assinatura Authenticode: o estado e o caminho

O instalador ainda não é assinado com Authenticode; o SmartScreen pede
confirmação na primeira instalação. A decisão (ROADMAP 7.3) é o caminho
gratuito da **SignPath Foundation**, que emite certificados OV para projetos
open source e assina no pipeline deles. O que ela exige, e onde estamos:

| Exigência | Estado |
|---|---|
| Licença OSI, sem dual-licensing | MIT ✅ |
| Projeto publicado e mantido, com página de download que descreve o que ele faz | Releases + README ✅ |
| Build em CI, todos os jobs em runners do GitHub | `release.yml` ✅ |
| Artefato enviado ao SignPath como artefato do mesmo run | job `sign` ✅ (aguarda as variáveis) |
| Metadados de versão e nome de produto nos binários | `productName`/versão do Tauri ✅ |
| `SECURITY.md` e contato de segurança | ✅ |
| Sem componente proprietário no pacote assinado (bibliotecas de sistema são aceitas) | CPU: só o runtime do VC ✅ · GPU: embute as DLLs do CUDA (NVIDIA, redistribuíveis) — a candidatura deve declarar isso e começar pela variante CPU |

Passos para a candidatura (ação do mantenedor): formulário em
https://signpath.org (Open Source → apply), informando repositório, página de
releases, descrição do app e o `SECURITY.md` como contato; depois da
aprovação, criar o projeto e a política na SignPath, cadastrar o token e as
três variáveis no repositório e publicar a próxima versão — o job `sign` passa
a rodar sozinho. A reputação no SmartScreen acumula com o tempo, a partir do
certificado; a assinatura não elimina o aviso de imediato.
