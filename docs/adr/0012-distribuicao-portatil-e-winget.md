# 0012 — Distribuição além do instalador: zip portátil e um pacote no winget

- **Status:** aceita
- **Data:** 23/09/2026

## Contexto

Até a 0.19 o ISPer só saía como instalador NSIS (GPU e CPU). A fase 7.6 pedia
mais dois caminhos: um **zip portátil**, para quem não pode ou não quer rodar
instalador (máquina corporativa, teste rápido), e o **winget**, o
gerenciador de pacotes do Windows.

Três fatos do projeto pesaram:

- o **atualizador** do Tauri baixa e roda o instalador NSIS, que instala em
  `Programs`: numa cópia fora dali, ele criaria uma segunda instalação em vez
  de atualizar a pasta;
- os **dados** (config, banco, modelos, logs, reuniões) já vivem em pastas do
  perfil do usuário, resolvidas num só lugar (`paths.rs`, `isper-models`,
  `isper-llm`), e mudá-las mexeria em todos esses caminhos;
- os dois instaladores gravam o **mesmo registro de desinstalação** ("ISPer",
  publicador "isper", derivado do identificador `com.isper.desktop`).

## Decisão

**Zip portátil**, uma variante por instalador (`ISPer_<v>_x64-portable.zip` e
`ISPer_<v>_x64-cpu-portable.zip`), gerado no `release.yml` a partir do mesmo
exe e das mesmas DLLs que o instalador leva:

- "portátil" quer dizer **sem instalar**, não "dados na pasta": os dados
  ficam nas mesmas pastas da versão instalada, e o zip e o instalador podem
  ser trocados um pelo outro sem perder nada;
- um arquivo **`portable.txt`** ao lado do exe marca a cópia portátil
  (`paths::is_portable`). É explícito de propósito: "não há desinstalador ao
  lado" também valeria para um build de desenvolvimento;
- na cópia portátil o atualizador **avisa** da versão nova, mas não instala:
  o botão vira "Baixar a nova versão" e abre a página de download; o
  Diagnóstico mostra "versão portátil (zip) — atualização manual".

**winget: um pacote só**, `MarcusBoni.ISPer`, com o instalador **CPU**
(9 MB, funciona em qualquer PC x64). A descrição aponta a versão com CUDA no
site. Os manifestos são gerados a cada versão por
`scripts/winget-manifests.ps1`, com o SHA-256 do `SHA256SUMS.txt` publicado, e
validados com `winget validate`. O manifesto declara o registro real
(`DisplayName: ISPer`, `Publisher: isper`) para o winget reconhecer o app
instalado.

**O publicador do instalador não muda** ("isper" em vez de "Marcus Boni"): o
NSIS do Tauri guarda a pasta de instalação numa chave que inclui o
publicador, e trocá-lo faria a próxima atualização instalar uma segunda
cópia em vez de atualizar a existente.

## Consequências

- Quem baixa o zip atualiza à mão (baixar, fechar, substituir os arquivos);
  apagar o `portable.txt` faria o atualizador instalar uma segunda cópia — o
  próprio arquivo avisa.
- O zip GPU tem ~400 MB, como o instalador.
- Quem instala pelo winget recebe a versão CPU; quem tem NVIDIA e quer a
  velocidade da GPU instala pelo site. O atualizador interno mantém cada
  variante na sua trilha (`latest.json` × `latest-cpu.json`), com ou sem
  winget.
- Enquanto a SignPath não assina, o exe dentro do zip não tem Authenticode;
  quando assinar, o zip precisa ser montado com o exe assinado.
- A submissão de cada versão ao `microsoft/winget-pkgs` é um PR público feito
  pelo mantenedor; o repositório guarda os manifestos gerados em
  `packaging/winget/`.

## Alternativas consideradas

- **Portátil de verdade, com os dados ao lado do exe.** Útil para pendrive,
  mas exigiria redirecionar todos os caminhos de dados e ainda assim o
  WebView2 e as notificações gravam no perfil do usuário. Fica para quando
  houver pedido.
- **Dois pacotes no winget (CPU e CUDA).** O winget correlaciona o app
  instalado pelo registro; com os dois pacotes declarando o mesmo "ISPer",
  ele não saberia qual está instalado e poderia "atualizar" trocando a
  variante.
- **Pacote do winget com o instalador GPU.** 400 MB de DLLs do CUDA inúteis
  para quem não tem NVIDIA, e não há garantia de que a variante CUDA rode sem
  o driver da NVIDIA.
- **Detectar o portátil pela falta de `uninstall.exe`.** Pegaria também os
  builds de desenvolvimento e o teste do atualizador.

## Onde vive

`apps/isper-app/src-tauri/src/paths.rs` (`is_portable`),
`apps/isper-app/src-tauri/src/updater.rs` (`install`, `open_download_page`),
`packaging/portable/portable.txt`, `.github/workflows/release.yml` (passo
"Versão portátil (zip)"), `scripts/winget-manifests.ps1`,
`packaging/winget/`.
