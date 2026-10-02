# 0020 — Uma janela principal com barra lateral, e as telas em iframes vivos

- **Status:** aceita
- **Data:** 02/10/2026

## Contexto

Cada tela do ISPer era uma janela própria: Início, Biblioteca, Configurações,
primeira configuração e Copilot. Abrir as Configurações a partir do Início
punha outra janela na barra de tarefas. Com duas ou três abertas, o usuário
alternava entre janelas do mesmo app pelo Alt+Tab, e cada uma tinha a sua
cópia da navegação (os botões Biblioteca, Configurações e tema no topo).
Os apps de desktop que servem de referência no Windows (Teams, Slack,
Spotify, o próprio Configurações do Windows, Linear, Notion) têm **uma
janela** com navegação lateral. Janela à parte fica para o que precisa
flutuar sobre outro app, como o mini player ou a janela da reunião.

As telas são HTML sem build step ([0007](0007-interface-sem-build-step.md)),
grandes (50 a 75 KB cada) e escritas para ter o documento só para si: ids
globais, `document.addEventListener`, estado em variáveis do script.

## Decisão

- **Uma janela principal (`main`, `ui/app.html`)** com barra lateral
  (Início, Biblioteca, Copilot, Configurações), a gravação de reunião sempre
  à vista, a paleta de comandos (Ctrl+K) e os atalhos da janela.
- **Cada tela é um iframe do `app.html`**, criado na primeira visita e
  mantido vivo. Trocar de tela é instantâneo e não perde o que estava
  aberto, digitado ou rolado: a reunião selecionada na Biblioteca, um campo
  editado nas Configurações. A Biblioteca é pré-carregada logo depois da
  primeira tela, e as outras quando o mouse passa no item.
- **A ponte é o `boot.js`.** O Tauri só injeta o `__TAURI__` e o script de
  inicialização no frame principal (`for_main_frame_only`). A tela, ao se
  ver num iframe da mesma origem, usa o `__TAURI__` e o `__ISPER_UI` da mãe.
  Os comandos e eventos são os mesmos de antes, e os ouvintes registrados
  pela tela saem com ela (`pagehide`). As telas não sabem que mudaram de
  casa; só perderam a navegação própria e o "Esc fecha a janela".
- **O Rust navega, não abre janelas.** `open_home`, `open_library_at`,
  `open_settings_section` etc. viram `views::navigate`, que traz a janela e
  emite `isper-nav`. A página manda `shell_view` quando troca de tela sozinha,
  e o título nativo acompanha ("ISPer — Biblioteca" no Alt+Tab). Eventos que
  iam para `home`, `library`, `settings` e `onboarding` vão para `main`.
- **A primeira configuração ocupa a janela inteira** (sem barra lateral).
  Sair dela por qualquer caminho continua contando como concluída: Concluir,
  Pular, o × da janela, ou a bandeja levando a outra tela.
- **Ficam à parte, de propósito:** o indicador flutuante e o **Copilot**. O
  Copilot é um painel para acoplar ao lado do Teams (360 px, sempre no topo).
  Dentro da janela principal, ele cobriria o que precisa ficar à vista. Na
  barra lateral, ele tem o ícone de "abre ao lado".
- **A janela lembra tamanho, posição e se estava maximizada**
  (`main_window` no `config.toml`, conferida contra os monitores ligados) e
  se a barra lateral estava recolhida (`sidebar_collapsed`). Ela nasce
  escondida e aparece quando a primeira tela pintou, sem o clarão branco do
  WebView2 vazio.

## Consequências

- Uma janela na barra de tarefas. Fechar a janela deixa o ISPer na bandeja,
  como antes, e reabrir pela bandeja traz a janela como estava.
- Ficou fácil levar a uma seção certa: as Configurações têm um índice lateral
  e abrem direto em `celular`, `inteligencia`, `modelos`… O aviso do
  celular pedindo pareamento, o Copilot sem chave e as pendências do Início
  usam isso.
- Soltar áudio em qualquer tela funciona: a janela mostra a área de soltar e
  leva os arquivos à Biblioteca.
- As telas não podem mais mexer na janela pelo `getCurrentWindow()`, porque
  a janela agora é de todas. Quem fecha ou redimensiona é a janela principal.
- Os testes e2e falam com as telas como antes, por um trecho da URL. O
  `tools/e2e/cdp-target.mjs` acha o iframe dentro do `app.html` e avalia no
  contexto dele. Captura de tela e teclado põem a tela à vista antes.
- Memória: as telas vivas custam como as janelas abertas custavam, numa
  janela só, e fechar a janela libera tudo.

## Alternativas consideradas

- **Juntar tudo num documento só (SPA sem framework).** Seria o caminho
  "limpo", mas exigiria reescrever cerca de 270 KB de telas para não
  colidirem em ids, estilos e ouvintes globais. O risco de regressão é alto
  para o que o usuário ganha, que é o mesmo da decisão escolhida.
- **Navegar a janela entre páginas (`location.href`)**, com a barra lateral
  repetida em cada uma. Troca de tela com recarga e sem estado: perderia a
  reunião aberta e o que estivesse digitado nas Configurações.
- **Vários WebViews numa janela (multiwebview do Tauri).** Atrás da feature
  `unstable`, com posicionamento manual dos WebViews a cada redimensionamento.
  O iframe resolve o mesmo com o layout do próprio navegador.
- **Título personalizado (sem a barra nativa).** Perde o Snap Layouts do
  Windows 11 no botão de maximizar e a acessibilidade da barra nativa, que
  já acompanha o tema (`set_theme`).
- **Copilot dentro da janela principal.** Ver acima: ele existe para ficar
  ao lado da reunião.

## Onde vive

`apps/isper-app/ui/app.html`, `ui/assets/{shell.js,shell.css,boot.js}`,
`apps/isper-app/src-tauri/src/views.rs` (navegação, geometria, comandos
`shell_*`), `capabilities/default.json` (janelas `overlay`, `main` e
`copilot`), `tools/e2e/cdp-target.mjs`.
