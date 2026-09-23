# 0007 — Interface em HTML/CSS/JS sem build step, com tudo embutido

- **Status:** aceita (ampliada em 23/09/2026 com o idioma da interface)
- **Data:** 26/08/2026

## Contexto

A interface do ISPer são poucas janelas (Início, Biblioteca, Configurações,
Copilot, primeira configuração e o indicador flutuante) servidas pelo
WebView2. O app promete funcionar **offline** e não baixar nada em tempo de
execução. O mantenedor está aprendendo Rust; uma segunda cadeia de build
(Node, bundler, framework) dobraria o que é preciso entender para mexer numa
tela.

## Decisão

- **HTML, CSS e JavaScript puros**, sem bundler nem framework, em
  `apps/isper-app/ui/`. O Tauri embute a pasta no executável.
- **Design system próprio e pequeno:** `assets/base.css` (tokens,
  componentes, movimento, tema claro e escuro) e `assets/ui.js` (toast com
  Desfazer, count-up, abas acessíveis, formatação).
- **Nada vem da rede:** as fontes (Fraunces e Hanken Grotesk, OFL) estão em
  `ui/assets/fonts`; a CSP (`tauri.conf.json`) só permite o próprio app e o
  IPC.
- **Idioma da interface embutido (23/09):** os dicionários
  `ui/locales/{pt-BR,en}.json` entram no binário por `include_str!`
  (`i18n.rs`) e chegam a cada janela no script de inicialização
  (`window.__ISPER_UI`), porque a CSP bloqueia `fetch` ao próprio endereço.
  O mesmo dicionário serve a bandeja, as notificações e os erros no Rust.

## Consequências

- Mexer numa tela é editar um arquivo e recompilar; o build release embute a
  UI, então toda mudança de HTML pede rebuild (a UI não vem de disco).
- Sem framework, cada tela cuida do próprio estado e redesenho; as regras
  que evitaram bugs viraram convenção: nada de `innerHTML` com dado (texto do
  Whisper pode conter `<`), `replaceChildren` e nós criados à mão.
- A CSP do Tauri leva hashes, e com eles o `'unsafe-inline'` deixa de valer:
  **atributo `style=""` é bloqueado**. Estilo só por classe ou pelo CSSOM.
- Janelas criadas pela config do Tauri (o indicador) nascem sem o script de
  inicialização e pedem o dicionário por `invoke('ui_prefs')`.
- Testes de tela rodam no app real via CDP (`tools/e2e`), e um teste em Rust
  garante que toda chave citada pelas páginas existe nos dois idiomas.

## Alternativas consideradas

- **React/Vue/Svelte com Vite.** Ergonomia melhor para telas grandes, ao
  custo de Node no build, dependências de npm para auditar e uma segunda
  linguagem de ferramentas. Para seis janelas, não se paga.
- **Carregar dicionários ou fontes de uma CDN.** Quebra o funcionamento
  offline e a promessa de não falar com a rede sem pedir.
- **UI nativa (WinUI).** Contra [0002](0002-rust-tauri-whisper-cpp.md).

## Onde vive

`apps/isper-app/ui/`, `apps/isper-app/src-tauri/src/{ui,i18n,views}.rs`,
`apps/isper-app/src-tauri/tauri.conf.json` (CSP),
`apps/isper-app/src-tauri/capabilities/default.json` (janelas na ACL).
