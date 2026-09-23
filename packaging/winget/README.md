# ISPer no winget

O pacote é **`MarcusBoni.ISPer`**, com o instalador **CPU** de cada versão —
o que funciona em qualquer PC x64. A versão com CUDA (NVIDIA) fica no site.
O porquê de um pacote só está no [ADR 0012](../../docs/adr/0012-distribuicao-portatil-e-winget.md).

```powershell
winget install MarcusBoni.ISPer
```

> **Situação:** a 0.19.0 (primeira versão do pacote) foi submetida em
> 23/09/2026 como [microsoft/winget-pkgs#439838](https://github.com/microsoft/winget-pkgs/pull/439838)
> e a 0.20.0 como [#439952](https://github.com/microsoft/winget-pkgs/pull/439952).
> O comando acima só funciona depois que o primeiro for aceito.

## Automático, a partir da versão seguinte

Cada release dispara o [`winget.yml`](../../.github/workflows/winget.yml): o
[Komac](https://github.com/russellbanks/Komac) parte do manifesto da versão
anterior que já está no `winget-pkgs`, troca URL, SHA-256, data e notas e abre
o PR sozinho, a partir do fork `Marcus-Boni/winget-pkgs`. O Komac confere se
já há PR aberto para a mesma versão, então uma submissão feita à mão não é
duplicada.

Ele precisa de um token do mantenedor, que nunca fica no repositório:

1. Crie um **PAT clássico** em GitHub → Settings → Developer settings →
   Personal access tokens → *Tokens (classic)*, com os escopos `public_repo` e
   `workflow`. O Komac não aceita token *fine-grained*.
2. Guarde como secret do repositório:

   ```powershell
   gh secret set WINGET_TOKEN --repo Marcus-Boni/ISPer
   ```

Sem o secret, ou enquanto a primeira versão do pacote não é aceita no
`winget-pkgs`, o workflow avisa no resumo da run e pula; a release não depende
dele. Para ensaiar sem abrir PR (os manifestos ficam como artefato da run):

```powershell
gh workflow run winget.yml -f tag=v0.21.0 -f dry-run=true
```

O processo à mão, abaixo, continua valendo para a primeira versão e para
quando o workflow não puder rodar.

## À mão

1. Gere e valide os manifestos (o SHA-256 vem do `SHA256SUMS.txt` da
   release, então a versão precisa estar publicada):

   ```powershell
   .\scripts\winget-manifests.ps1 -Version 0.19.0
   ```

   O resultado vai para `manifests/m/MarcusBoni/ISPer/<versão>/`, no layout
   do [`microsoft/winget-pkgs`](https://github.com/microsoft/winget-pkgs), e o
   script roda `winget validate` no fim.

2. Suba os manifestos gerados neste repositório (um PR como qualquer outro).

3. Envie ao `microsoft/winget-pkgs` — é um PR público, feito pela conta do
   mantenedor. O jeito mais curto é o
   [`wingetcreate`](https://github.com/microsoft/winget-create):

   ```powershell
   winget install Microsoft.WingetCreate
   ```

   ```powershell
   wingetcreate submit .\packaging\winget\manifests\m\MarcusBoni\ISPer\0.19.0
   ```

   Ele pede um token do GitHub na primeira vez, faz o fork e abre o PR. A
   partir da segunda versão, `wingetcreate update MarcusBoni.ISPer --version
   <v> --urls <url do cpu-setup.exe> --submit` gera e envia de uma vez.

Os bots do `winget-pkgs` instalam o pacote numa máquina limpa e conferem o
SHA-256; um revisor humano aprova. A primeira submissão costuma levar alguns
dias; as seguintes, menos.

## O que o manifesto declara

- `InstallerType: nullsoft`, `Scope: user` (sem UAC), modos interativo e
  silencioso — o instalador do Tauri aceita `/S`.
- `AppsAndFeaturesEntries` com `DisplayName: ISPer`, `Publisher: isper` e
  `ProductCode: ISPer` (a chave `HKCU\...\Uninstall\ISPer`): é o que o
  instalador grava no registro, e é por aí que o winget reconhece o app já
  instalado (inclusive o instalado pelo site).
- Schema 1.12.0, o que o modelo de PR do `winget-pkgs` pede, e comentários em
  inglês, porque os manifestos são lidos lá.
- `MinimumOSVersion: 10.0.19041.0` (Windows 10 2004): a captura de áudio só
  do Teams usa uma API dessa versão.
