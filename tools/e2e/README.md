# Testes ponta a ponta (`tools/e2e`)

Testes que rodam o **ISPer real** (o exe, com WebView2, Whisper e tudo) e
conversam com as janelas pelo Chrome DevTools Protocol: o app é aberto com
`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9223` e o
`cdp.mjs` avalia JavaScript em cada janela (chama comandos Tauri via
`window.__TAURI__.core.invoke`, lê o DOM, ouve eventos). Complementam os
testes unitários dos crates (`cargo test`), que não conseguem cobrir janelas,
atalhos globais, áudio e o atualizador.

## Pré-requisitos

- Windows com o app compilado (`cargo build --release -p isper-app`) ou instalado.
- Node 22+ (o `cdp.mjs` usa o `WebSocket` global) e Python 3 (servidor local do
  teste do atualizador).
- Um modelo Whisper instalado (Início → Baixar) para o teste de reunião.
- Os testes **fecham qualquer ISPer aberto** e relançam, no fim, o exe testado
  sem a porta de depuração.

## Scripts

| Script | O que cobre | Duração |
|---|---|---|
| `smoke.ps1` | Início, Biblioteca, Configurações e indicador abrem sem erros de JS; modos do indicador; resposta do atualizador | ~40 s |
| `meeting.ps1` | Reunião com a fixture de duas vozes: ao vivo, legendas, momentos (comando com debounce + atalho global), encerrar, banco, Markdown, DOCX, Biblioteca, limpeza | ~2 min |
| `updater-local.ps1` | Atualizador completo contra uma release falsa assinada com a sua chave e servida em localhost: checagem, banner, download com assinatura, download adulterado recusado, recusa durante reunião (nada é instalado) | ~6 min |

```powershell
.\tools\e2e\smoke.ps1 -Exe .\target\release\isper-app.exe
.\tools\e2e\meeting.ps1 -Exe .\target\release\isper-app.exe
.\tools\e2e\updater-local.ps1
```

Sem `-Exe`, os scripts usam o app instalado em `%LOCALAPPDATA%\Programs\ISPer`
se existir, senão `target\release\isper-app.exe`. Cada verificação imprime
`OK`/`FALHA`; o código de saída é 1 se algo falhou.

## Cuidados

- **`meeting.ps1` toca áudio nos alto-falantes e o loopback captura tudo que
  estiver tocando no PC.** Não rode durante uma reunião real nem com música. O
  título da reunião de teste vai para o provider de IA configurado, como numa
  reunião normal. A reunião e os arquivos gerados são apagados no fim (use
  `-KeepMeeting` para inspecionar).
- `updater-local.ps1` gera um build de **teste** do app que aceita `http` no
  endpoint de atualização e o refaz no fim; com `-SkipRebuild`, lembre de rodar
  `cargo build --release -p isper-app` antes de distribuir qualquer coisa.
- Depurar à mão: com o app aberto pelo `Start-Isper`, `node tools\e2e\cdp.mjs
  home.html "document.title"`. `await` só dentro de `(async () => { ... })()`.
- O `cdp.mjs` imprime o `JSON.stringify` do resultado; quando a própria página
  devolve `JSON.stringify(...)`, o texto vem duplamente codificado — `EvJson`
  no `common.ps1` já resolve isso.

## Como o teste da atualização instalada foi feito

O caminho completo (app instalado → checagem automática → banner → download →
instalador → app volta na versão nova) precisa de uma versão publicada mais nova
que a instalada, então não está automatizado aqui. Ele foi validado na 0.11.0 →
0.11.1: instalação silenciosa com `ISPer_<v>_x64-setup.exe /S /D=<pasta>`,
publicação da versão seguinte com `scripts/release.ps1 -Publish` e observação
do banner e da troca de versão via CDP.
