# ISPer no winget

O pacote é **`MarcusBoni.ISPer`**, com o instalador **CPU** de cada versão —
o que funciona em qualquer PC x64. A versão com CUDA (NVIDIA) fica no site.
O porquê de um pacote só está no [ADR 0012](../../docs/adr/0012-distribuicao-portatil-e-winget.md).

```powershell
winget install MarcusBoni.ISPer
```

## A cada versão publicada

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
