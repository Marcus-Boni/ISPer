---
title: "Releases e integridade"
description: "Entenda variantes, checksums, SBOM, atualização automática e assinatura do instalador."
section: "Referência"
order: 98
---

# Releases e integridade

Cada release estável publica dois instaladores, CPU e CUDA, e junto com eles SBOM CycloneDX, `SHA256SUMS.txt`, manifests do atualizador e assinaturas minisign.

## Verificar no PowerShell

```powershell
(Get-FileHash .\ISPer_<versão>_x64-cpu-setup.exe -Algorithm SHA256).Hash.ToLower()
```

Troque `<versão>` pela que você baixou — ou use a [página de download](/download/), que monta o comando já com o nome do arquivo e mostra a soma ao lado, para comparar sem sair da tela.

Os parênteses e o `.ToLower()` não são enfeite: `Get-FileHash` sozinho imprime uma tabela de três colunas com o caminho cortado e o hash em maiúsculas, enquanto o valor publicado é minúsculo. Assim sai uma linha só, igual à que está no `SHA256SUMS.txt` da mesma release.

## Assinaturas diferentes

- **SHA-256** confirma que os bytes correspondem ao valor publicado.
- **Minisign** protege os pacotes aceitos pelo atualizador do ISPer.
- **Authenticode** identifica o editor para o Windows. A candidatura à SignPath está em andamento; enquanto ela não sai, os instaladores ainda podem exibir SmartScreen.

Nunca baixe instaladores de mirrors ou anexos de terceiros.
