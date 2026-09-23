# Vitrine do repositório

O GIF de demonstração do README e a imagem de prévia social do GitHub saem
daqui, das telas **do próprio app** — nada é desenhado à mão, então a vitrine
acompanha a interface quando ela muda.

| Arquivo | O que faz |
| --- | --- |
| `capture.ps1` | Abre o app real via CDP (como os e2e), põe em inglês e captura a primeira configuração (escura, clara e o passo do atalho) e o Copilot com a reunião fictícia. Idioma e tema voltam ao que estavam |
| `fake-copilot.js` | A reunião **fictícia** que o Copilot mostra, injetada pelo `addSegment()`/`render()` da própria página |
| `compose.ps1` | Renderiza `frame.html` e `social.html` no Edge headless, com as fontes do app, e junta os quadros num GIF com o ffmpeg |
| `frame.html` · `social.html` | Os modelos do quadro do GIF (1080×800, legenda + captura) e da prévia social (1280×640) |

## Privacidade

As capturas saem do app instalado de quem roda o script. Por isso só entram
telas **sem dado pessoal**:

- a primeira configuração, **sem** o passo do microfone (mostraria o nome do
  dispositivo);
- o Copilot, com a reunião de `fake-copilot.js`.

Início, Biblioteca e Configurações ficam de fora: mostram reuniões, pastas e
dispositivos de verdade. Antes de subir um GIF novo, abra cada quadro e
confira.

## Refazer

```powershell
cargo build --release -p isper-app
```

```powershell
.\tools\showcase\capture.ps1 -Exe .\target\release\isper-app.exe
```

```powershell
.\tools\showcase\compose.ps1
```

O resultado vai para `docs/media/`: `isper-demo.gif` (o README usa) e
`social-preview.png`. A prévia social não tem API: suba à mão em **GitHub →
Settings → General → Social preview**.

Precisa do Node (para o CDP, como nos e2e), do Edge (vem com o Windows) e do
ffmpeg (`winget install Gyan.FFmpeg`).
