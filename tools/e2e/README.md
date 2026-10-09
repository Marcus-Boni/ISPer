# Testes ponta a ponta (`tools/e2e`)

Testes que rodam o **ISPer real** (o exe, com WebView2, Whisper e tudo) e
conversam com as janelas pelo Chrome DevTools Protocol: o app é aberto com
`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9223` e o
`cdp.mjs` avalia JavaScript em cada tela (chama comandos Tauri via
`window.__TAURI__.core.invoke`, lê o DOM, ouve eventos).

Desde o [ADR 0020](../../docs/adr/0020-janela-unica.md), Início, Biblioteca,
Configurações e a primeira configuração são iframes da janela principal
(`app.html`). Para os scripts nada muda: a tela continua sendo pedida por um
trecho da URL (`'settings.html'`). O `cdp-target.mjs` acha o iframe e avalia
no contexto dele, e `cdp-shot.mjs` e `cdp-keys.mjs` põem a tela à vista
antes. `'app.html'` é a própria janela (barra lateral, paleta), e
`$script:Overlay` (o indicador) é de onde se reabre a janela depois de
fechá-la. `node cdp.mjs --list` lista as telas abertas. Complementam os
testes unitários dos crates (`cargo test`), que não conseguem cobrir janelas,
atalhos globais, áudio e o atualizador.

## Pré-requisitos

- Windows com o app compilado (`cargo build --release -p isper-app`) ou instalado.
- Node 22+ (o `cdp.mjs` usa o `WebSocket` global) e Python 3 (servidor local do
  teste do atualizador).
- Um modelo Whisper instalado (Início → Baixar) para o teste de reunião.
- **Feche o ISPer instalado antes** (bandeja → Sair). Os testes só encerram o que
  eles mesmos abriram: a cópia de teste, ou o app instalado quando foi o e2e que
  o abriu (reconhecido pela porta de depuração no WebView2). Com o seu ISPer
  aberto, eles param com uma mensagem em vez de derrubá-lo — ele pode estar
  gravando uma reunião. O Windows só deixa uma instância rodar, então não dá
  para testar com ele aberto.
- No fim, o exe testado é relançado sem a porta de depuração quando é o
  instalado. Uma cópia de desenvolvimento só é relançada no CI (ou com
  `ISPER_E2E_RELAUNCH=1`), para não ficar na bandeja no lugar do app de
  verdade.

## Perfil de dados próprio

Os testes nunca usam os dados de quem os roda. O `Start-Isper` abre o app com
`ISPER_PROFILE_DIR` apontando para uma pasta nova em `%TEMP%`
(`isper-e2e-<data>-<pid>`), e o app lê e grava tudo lá dentro, no mesmo
desenho das pastas do Windows:

| No perfil | No lugar de |
|---|---|
| `AppData\Roaming\ISPer` | `config.toml`, `llm.toml` e o banco em `%APPDATA%\ISPer` |
| `AppData\Local\com.isper.desktop\logs` | os logs em `%LOCALAPPDATA%\com.isper.desktop\logs` |
| `Documents\ISPer` | `Reunioes`, `Backups`, `Importar` e o zip de diagnóstico em `Documentos\ISPer` |

Um perfil vale para todos os `Start-Isper` de um mesmo roteiro, porque um
teste que reabre o app encontra o que deixou. Ele é apagado no fim quando tudo
passa. Quando algo falha, fica, e o caminho aparece na última linha.

Com o perfil, o app também não mexe no registro de iniciar com o Windows (um
build de desenvolvimento regravaria a entrada com o caminho dele) e guarda
chaves de IA num cofre à parte, `ISPer (perfil de teste)`. Os modelos continuam
os da máquina, porque são grandes e o app só os lê. O WebView2 também continua
com a pasta dele.

Numa máquina nova, a primeira configuração abre (não há `config.toml`) e o
`Start-Isper` a conclui, como no CI. `Get-E2EDataPath`, `Get-E2EDocsPath` e
`Get-TodayLog` dão os caminhos dentro do perfil. O `Start-Isper` confere que o
log nasceu no perfil. Um exe que não conhece a variável (0.21.0 e anteriores)
usaria os seus dados, e por isso o roteiro é fechado e para ali.

## Scripts

| Script | O que cobre | Duração |
|---|---|---|
| `smoke.ps1` | Início, Biblioteca, Configurações e indicador abrem sem erros de JS (inclusive violações de CSP); as três telas ficam na janela principal (nenhuma janela própria), as visitadas continuam vivas, Ctrl+K abre a paleta e as Configurações abrem direto numa seção; modos do indicador; indicador fixo alterna e volta ao estado original; `home_status` traz detecção de chamada e insights; busca semântica e atualizador respondem | ~45 s |
| `meeting.ps1` | Reunião com a fixture de duas vozes: ao vivo, legendas, momentos (comando com debounce + atalho global), encerrar, banco, Markdown, DOCX, Biblioteca, limpeza | ~2 min |
| `updater-local.ps1` | Atualizador completo contra uma release falsa assinada com a sua chave e servida em localhost: checagem, banner, download com assinatura, download adulterado recusado, recusa durante reunião (nada é instalado) | ~6 min |
| `data.ps1` | Fase 7.4 no app real: schema do banco no Diagnóstico, retenção (aviso de confirmação, salvar e voltar), backup SQLite com o mesmo `user_version`, pacote de diagnóstico (entradas certas, sem texto ditado), log em JSON Lines | ~40 s |
| `theme.ps1` | Tema da interface: claro, escuro e "seguir o Windows" aplicados na hora nas três janelas (fundo calculado, não só o atributo), o indicador continua escuro, os seletores de tema e idioma das Configurações e da primeira configuração acompanham a troca feita na outra janela, valor inválido volta ao padrão, janela reaberta já nasce no tema salvo. `-Shots <pasta>` grava um PNG de cada janela em cada tema | ~40 s |
| `agenda.ps1` | Agenda e reuniões (Fase 10.3), contra o mesmo OptTime de mentira (`fake-opttime.mjs` responde a agenda relativa à hora do teste): sem conector a seção Agenda não aparece; com ele, os eventos do dia (o que está acontecendo ganha "agora", o livre não pede preparo, o de dia inteiro vira selo), Entrar só no online, o preparo com quem organiza, "Atualizar" lendo de novo, a tarefa de reunião da caixa de entrada abrindo a Biblioteca no minuto e as duas opções novas nas Configurações. Não clica em Entrar nem dispara o aviso de preparo | ~35 s |
| `assistant.ps1` | Assistente (Fase 10.4), contra uma IA de mentira compatível com OpenAI (`fake-llm.mjs`, roteiro fixo, em 127.0.0.1) e o OptTime de mentira (que publica 4 ferramentas: duas de leitura, uma que lança e uma que apaga): sem IA, o erro leva às Configurações; com ela, a resposta cita a tarefa e a fonte destaca a tarefa na lista; criar tarefa para no cartão (Fazer cria, Agora não e uma pergunta nova com o cartão aberto não criam); a seção Assistente das Configurações lista as 10 do ISPer e as 4 do OptTime, a que apaga não fica livre e bloquear tira a ferramenta da IA; "Fechar o dia" lê as horas no OptTime sem perguntar e "Resumo da manhã" manda o pedido pronto | ~45 s |
| `learning.ps1` | Aprendizado (Fase 10.5): duas tarefas em 3 das últimas 4 semanas no dia da semana de hoje viram o cartão "Parece uma rotina" (com 3 de 4, a hora e os dias), a que aconteceu uma vez não; "Agora não" some com a sugestão e ela não volta; "Criar rotina" cria a rotina com o selo "sugerida pelo ISPer", a evidência em `learned_from` e a tarefa de hoje. Memória nas Configurações: guardar sem repetir, corrigir com Enter, fixar, arquivar e o Desfazer do aviso. O assistente (IA de mentira, `fake-llm.mjs`) propõe guardar uma preferência, guarda só depois do toque, e a memória vai nas instruções; "lembrar" nunca fica livre | ~40 s |
| `routines.ps1` | Rotinas e o conector do OptTime (Fase 10.2), contra um OptTime de mentira (`fake-opttime.mjs`, em 127.0.0.1, sem sessão como o de produção): o token vai para o cofre do perfil de teste e o `whoami` aparece nas Configurações (token errado vira "recusado", http fora do PC é recusado, o token não fica na tela); a rotina de horas pelo modelo da tela Hoje cria a tarefa do dia uma vez só; "Conferir agora" mostra o que falta; a revisão traz as sugestões (sem projeto fica de fora) e nada é lançado antes do toque; lançar manda a chave de idempotência e só o que foi editado, fecha o dia e conclui a tarefa; uma rotina comum pelo formulário e o interruptor de pausa | ~40 s |
| `capture.ps1` | Captura de tarefas (Fase 10.1): a fala entra como texto (`capture_text`, o caminho da voz depois do Whisper) e, sem IA no perfil de teste, vira um cartão com dia e hora pelo parser; editar e Enter salvam na tela Hoje com a origem "voz"; Esc descarta; o atalho de anotar aparece nas Configurações e na tela Hoje. Não toca áudio | ~30 s |
| `today.ps1` | Tela Hoje (Fase 10): o item na barra lateral com o número de tarefas; criar, concluir, mover para amanhã, descartar e aceitar da caixa de entrada mudam a lista na hora; o Desfazer do aviso e o Ctrl+Z voltam cada mudança; o texto digitado na paleta vira tarefa; editar o título grava; tudo continua depois de reabrir | ~40 s |
| `undo.ps1` | Desfazer pela interface real da Biblioteca: excluir um ditado some com ele na hora e mostra o toast; o botão Desfazer e o Ctrl+Z trazem de volta (nada é apagado). Três ditados de exemplo entram no perfil de teste, com o Python; sem ele, o roteiro pula. A exclusão de reunião que expira fica no `meeting.ps1` | ~20 s |
| `a11y.ps1` | Acessibilidade em Início, Biblioteca (reunião aberta e Ditados), Configurações, primeira configuração, Copilot e indicador, nos temas escuro e claro: todo controle com nome acessível, todo texto no contraste WCAG AA, nenhuma rolagem horizontal; volta de Tab com teclado de verdade (`cdp-keys.mjs`) alcança todos os controles, com anel em cada foco e sem prender (também num tema de contraste do Windows, emulado); abas pelas setas; Enter abre a reunião e renomeia o falante. Verificadores em `a11y-probe.js` e `a11y-tab.js`, sem dependência externa | ~60 s |
| `portable.ps1` | Versão portátil: uma pasta montada como o zip da release (exe, DLLs e `portable.txt`) abre, o Diagnóstico diz "versão portátil" (também na tela), o atualizador recusa instalar por cima e, sem o marcador, a mesma pasta volta a ser uma cópia comum. A pasta fica em `%TEMP%` e é apagada no fim | ~25 s |
| `import.ps1` | Importar gravações (fase 9.0), com um modelo Whisper instalado: um MP3 entregue à fila (como o botão e o arrastar) vira reunião com a origem no banco e no `.md`; o mesmo áudio aponta a reunião que já existe; um `.wma` é recusado na entrada; uma mensagem de voz `.opus` vira reunião (fase 9.2); um OGG deixado na pasta vigiada vira reunião com data e título do nome e vai para `Importados`; um "mp3" que não é áudio vai para `Não importados` com o motivo; o selo na Biblioteca e a opção nas Configurações. A pasta vigiada é a do perfil de teste | ~2 min |
| `onboarding.ps1` | Primeira configuração: com `onboarding_done = false` o app abre nela e não no Início; os cinco passos montam (medidor do microfone — ou o aviso, sem microfone —, modelos com um recomendado, atalho, IA, resumo), foco no título de cada passo, Enter avança, o idioma troca na hora, concluir grava `onboarding_done = true` e abre o Início, Configurações → Sistema a reabre e o × também conta como concluída. `-Shots <pasta>` grava os passos | ~40 s |
| `i18n.ps1` | Idioma da interface: trocar para inglês e de volta vale na hora nas Configurações (HTML estático, texto montado por script, placeholders, Diagnóstico), no Início, na Biblioteca (lista, reunião aberta, falantes exibidos em inglês), no Copilot (estado ocioso e uma reunião sintética pelo próprio `render()`: cards, ações, gatilho da análise; reaberto nasce em inglês) e no indicador, nenhuma chave falta, a janela reaberta nasce no idioma salvo, valor inválido vira `auto`. `-Shots <pasta>` grava a tela em inglês | ~30 s |
| `sync.ps1` | Sincronia com o celular (fase 9.3) no app real, com o `isper-cli enviar` fazendo de celular: ligar a opção (a porta sobe), o QR, o "Permitir?" (o teste permite), a gravação com dois momentos virar reunião com a origem e os momentos, a ata voltar ao "celular", mandar de novo não reenviar, um segundo aparelho recusado, o aparelho esquecido não mandar mais. Tudo pelo loopback; precisa de um modelo Whisper e do `isper-cli` compilado | ~1 min |
| `android-sync.ps1` | Sincronia no emulador Android, com o `isper-cli receber` fazendo de PC (o emulador chega a ele por 10.0.2.2): colar o código e parear, gravar e a gravação chegar ao PC com o manifesto, a ata voltar ("Ata pronta") e abrir, desconectar (24 verificações) | ~3 min |
| `android-transcribe.ps1` | Transcrição no celular (fase 9.4) no emulador, com o modelo Tiny e a tomada simulada pelo `dumpsys battery`: nos Ajustes, o modo "Ao carregar" e o modelo; fora da tomada a gravação espera, na tomada começa; tirada da tomada no meio, para e guarda o que fez; de volta, continua ("continuando N janelas") e sai a "Ata do celular"; "Transcrever agora" na bateria; um áudio corrompido vira "Falhou no celular" e volta à fila com "Tentar de novo" (30 verificações). Precisa do `isper-cli` (faz os `.opus`) e de rede na primeira vez | ~6 min |
| `android-lab.ps1` | App Android (fase 9.1) num emulador: sobe o AVD `isper-lab` (cria se faltar), instala o APK, roda o laboratório pelo `autorun` com o modelo tiny e confere o relatório (tempos, fator de tempo real, falantes, WER e DER) | ~5 min |
| `android-recorder.ps1` | Gravador Android (fase 9.2) pela interface, no emulador (uiautomator, etiquetas `testTag`): gravar, marcar e parar (manifesto `finished`, duração, momento, `.opus` legível pelo ffprobe); matar o app no meio com `am force-stop` e ver a gravação voltar como `recovered`; gravar com a tela apagada. Utilitários em `android-common.ps1` | ~3 min |
| `soak.ps1` | Reunião longa (10 min por padrão; `-Minutes 120` para as 2 h) com a fixture em loop, medindo a memória do processo a cada 30 s: o áudio dos participantes vai para disco (`%TEMP%\ISPer\*.pcm`), então a memória privada deve ficar estável depois do aquecimento (`-MaxGrowthMB`, padrão 150). Confere também a transcrição ao vivo e a limpeza dos `.pcm`, apaga a reunião de teste e grava um CSV em `target\soak\` | 10 min a 2 h |
| `copilot-filter.ps1` | Filtro do Copilot pelo Jev: o bloco em Configurações → Inteligência (traduzido, sem erro de JS), o filtro nasce desligado e liga e desliga pelo Salvar (`config.toml`), o "Testar conexão" (`test_typesafe`) e o Copilot sem filtro fora de reunião. Com `-UseStoredKey`, a chave guardada em `typesafe.ISPer` vai ao app por `ISPER_TYPESAFE_API_KEY` (nunca impressa) e a chamada real precisa marcar o compromisso do exemplo (~US$ 0,00003); sem ela, o teste confere a mensagem de chave ausente. Não grava reunião nem toca áudio. |

```powershell
.\tools\e2e\smoke.ps1 -Exe .\target\release\isper-app.exe
.\tools\e2e\meeting.ps1 -Exe .\target\release\isper-app.exe
.\tools\e2e\updater-local.ps1
.\tools\e2e\soak.ps1 -Minutes 120 -Exe .\target\release\isper-app.exe
```

`cdp-shot.mjs <trecho-da-url> <saida.png>` captura a tela de uma janela
(mesma pré-condição do `cdp.mjs`) — para conferir à vista o que os testes
medem por número.

## Banco de testes do Copilot (sem compilar o app)

`copilot-harness.py` é a exceção da pasta: ele **não** abre o ISPer. Serve a
pasta `apps/isper-app/ui/` num servidor local e injeta um `window.__TAURI__`
de mentira em `copilot.html`, com uma reunião roteirizada. Serve para iterar
no HUD — layout, estados vazios, erro, teclado, modo acoplado de 380 px — sem
esperar um build com CUDA.

```powershell
python .\tools\e2e\copilot-harness.py   # http://127.0.0.1:3112/copilot.html
```

Serve também `/library.html`, com uma reunião de exemplo que já tem decisões
validadas — é como se testa a seção "Decisões e alertas" sem gravar nada.

No console da página: `__sim.play()` despeja a reunião inteira, `__sim.step()`
avança uma fala, `__sim.partial('…')` manda uma legenda provisória e
`__sim.scenario('no-key' | 'idle' | 'error' | 'meeting')` troca o cenário. O
mock cobre só a camada de tela — áudio, Whisper e as chamadas de IA de verdade
continuam sendo exercício do `meeting.ps1`.

> **O que o harness NÃO pega: a ACL do Tauri.** Aqui `listen()` é um mock e sempre
> funciona. No app, uma janela que não esteja em `capabilities/default.json` leva
> `plugin:event|listen not allowed by ACL` e fica sem evento nenhum — os comandos
> continuam respondendo, então a janela parece viva, carrega o estado ao abrir e
> congela a partir dali. Foi assim que o Copilot nasceu sem transcrição ao vivo.
> Janela nova: confira a ACL no app de verdade, não só aqui.

O `smoke.ps1` também roda toda noite no GitHub Actions
([`e2e-nightly.yml`](../../.github/workflows/e2e-nightly.yml)): o runner
Windows compila o app sem CUDA e o abre de verdade (WebView2 + CDP). Como o
runner não tem placa de som nem GPU, `meeting.ps1` e `soak.ps1` continuam
locais.

Duas coisas que só o runner ensinou (e que valem para qualquer máquina onde o
job rode **elevado**, como administrador): o WebView2 ignora a variável de
ambiente `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS` — e a chave em HKCU — num
processo elevado, então com `ISPER_E2E_CDP_REGISTRY=1` o `Start-Isper` grava
a porta na política `HKLM\SOFTWARE\Policies\Microsoft\Edge\WebView2\AdditionalBrowserArguments`
(valor `isper-app.exe`) e a `Restart-IsperClean` a remove; e a política precisa
ficar enquanto o app testado vive, porque cada janela nova cria um ambiente
WebView2 que tem de ter os mesmos argumentos do browser já aberto (senão
`ERROR_INVALID_STATE`, 0x8007139F). Os scripts também fixam
`$ErrorActionPreference = 'Continue'`: o pwsh do Actions roda com `Stop`, e um
roteiro de verificações não pode abortar na primeira janela que demora.

Sem `-Exe`, os scripts usam o app instalado em `%LOCALAPPDATA%\Programs\ISPer`
se existir, senão `target\release\isper-app.exe`. O instalado precisa conhecer
o perfil de teste; se não conhecer, o roteiro para no início. Cada verificação
imprime `OK`/`FALHA`, e o código de saída é 1 se algo falhou.

## Assistentes empacotados (MSIX) veem outro AppData

Se os scripts rodam a partir de um assistente de desenvolvimento **empacotado**
(o Claude Code desktop, por exemplo, é um app MSIX), tudo que eles e os
processos filhos escrevem em `%LOCALAPPDATA%`, `%APPDATA%` e no `HKCU` vai para
a pasta virtualizada do pacote (`%LOCALAPPDATA%\Packages\<pacote>\LocalCache`),
não para o AppData real do usuário — inclusive os modelos copiados à mão, os
logs, o `config.toml`, o banco e as chaves de registro que o instalador grava.
O ISPer aberto pelo usuário (pelo menu Iniciar) enxerga o AppData real e pode
estar "sem modelo" enquanto os testes dizem que está tudo certo. O repositório,
`Documentos`, `%LOCALAPPDATA%\Programs` e os processos não são virtualizados.
Para agir na visão real, lance um `.cmd` pelo `explorer.exe` (filho do Explorer,
fora do pacote) e leia a saída num caminho não virtualizado, como uma pasta do
repositório.

A virtualização nunca protegeu os dados do usuário. `Documentos` não é
virtualizado, e antes do perfil próprio um e2e rodado daqui deixou atas de
teste na `Documentos\ISPer\Reunioes` de verdade. Hoje o perfil em `%TEMP%` cobre
os dois casos. Só os modelos seguem no `%LOCALAPPDATA%` que o assistente
enxerga.

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
