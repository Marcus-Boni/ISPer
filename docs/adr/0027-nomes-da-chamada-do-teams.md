# 0027 — Nomes dos falantes lidos da janela do Teams, só na Biblioteca e só com evidência

- **Status:** aceita (piloto: a opção fica em beta até a rodada com chamadas reais)
- **Data:** 20/11/2026

## Contexto

O passe final ([0006](0006-dois-modos-ao-vivo-e-passe-final.md)) e a
diarização ([0005](0005-diarizacao-pos-hoc.md)) dizem *quantas* vozes havia
no canal dos participantes e quem falou o quê entre elas, mas não *quem* cada
uma era: a ata sai com "Participante 1", "Participante 2", e a pessoa renomeia
à mão. O Yogurt (FrigadeHQ/yogurt) mostrou um caminho: a janela da chamada já
diz quem está nela e, às vezes, quem fala — pela árvore de acessibilidade, a
mesma que um leitor de tela lê, sem permissão especial no Windows.

Três riscos moldaram a decisão:

1. **Nome errado é pior que nome nenhum.** "Participante 2" é honesto; "Ana"
   na fala do João vai para a ata, para o resumo e para a memória de alguém.
2. **A tela não é o áudio.** "Só o Teams" na configuração não prova a origem
   do som: com o Teams fechado o loopback cai para o sistema inteiro, e ao
   estagnar ele reabre — às vezes num Teams que reiniciou.
3. **Ler outra janela é delicado.** O UI Automation atravessa processos; um
   Teams que trava não pode travar a gravação, e o texto lido (chat,
   legendas, títulos) não pode parar no log nem na nuvem.

## Decisão

- **Desligado por padrão**, em Configurações → Reuniões ("Identificar
  falantes do Teams (beta)"), e só faz algo com "Só o Microsoft Teams" como
  fonte (ou outra fonte por processo do `config.toml`, `process:<nome.exe>`,
  que é o que o e2e usa), o passe final ligado e os modelos de diarização
  instalados (a tela diz o que falta). Vale a partir da próxima gravação, como manda a
  [0011](0011-opcional-e-configuracao-nao-feature-flag.md).
- **O leitor é um subprocesso do mesmo exe** (`--isper-teams-reader`), aberto
  só depois que o áudio abriu, com o PID raiz que o loopback **de fato**
  grava (`loopback::CaptureBinding`). Roda num Job Object que o mata quando o
  app fecha ou cai, com limite de memória; recebe ambiente vazio; lê só
  janelas de chamada (com botão de sair e de microfone) da árvore de
  processos gravada; e devolve pelo stdout só leituras já interpretadas
  (quem fala, quem está, "(Você)"), carimbadas com o QueryPerformanceCounter
  — o mesmo relógio do `Instant` em que a gravação começou. Sem linha por
  5 s, é trocado; o vínculo com o áudio mudou, as leituras dos 2 s em volta
  saem e uma lacuna entra.
- **A atribuição é uma função pura** (`speaker_names::attribute`), com
  relógio e leituras injetáveis, e só dá nome quando três camadas concordam:
  no trecho (mais da metade da fala com uma pessoa só — marcada falando e
  presente uma vez no painel Pessoas, ou a única outra pessoa numa chamada de
  dois com "(Você)" conhecido); no grupo da diarização (duas falas ou mais
  apontando a mesma pessoa, com 80 % da evidência); e entre grupos (uma
  pessoa reivindicada por dois grupos não fica com nenhum — e reivindicar
  basta ter a maior parte da evidência do grupo, mesmo que seja uma fala só:
  o e2e mostrou que um grupo pequeno apontando a mesma pessoa é outra voz
  dizendo ser ela). O nome vai só às
  falas que também têm a evidência no próprio trecho. "Eu" nunca entra;
  homônimos, duas pessoas falando e marca fora do painel ficam genéricos.
  O atraso da tela (0,75 s) e a validade de uma leitura (2 s) são parâmetros
  a calibrar.
- **O nome detectado mora ao lado do rótulo** (`segments.detected_speaker`,
  schema v10), nunca no lugar dele. Só a leitura da Biblioteca
  (`get_meeting_for_library`) o devolve; a ata, o "Copiar ata", o Copiar
  transcript, as exportações, a busca, o resumo, o assistente e a sincronia
  continuam no rótulo canônico. Corrigir ou confirmar o nome na Biblioteca
  edita pela chave do falante e o transforma em nome dado à mão; "Remover
  nomes detectados" apaga só as sugestões.
- **A edição vale mais que o passe.** `meetings.speaker_rev` sobe a cada
  edição de falante; com a opção ligada, o passe final guarda a revisão ao
  salvar a transcrição provisória e só publica, na mesma transação, se a
  reunião ainda existe e ninguém editou. Desligar a opção mata o leitor e
  invalida a evidência de gravações ativas e pendentes (uma época global),
  mesmo que ela volte a ser ligada antes do passe acabar.
- **No log, só números**: leituras, atrasadas, lacunas, falas nomeadas,
  grupos divididos, nomes disputados, cobertura. Nenhum nome, título ou texto.

## Consequências

- A ata exportada não muda com a opção: o nome detectado é uma sugestão na
  tela até a pessoa confirmá-lo. Isso custa um clique por falante, e é o
  preço de nunca mandar um palpite para fora.
- O vocabulário do Teams é texto da interface dele: uma atualização pode
  mudá-lo, e aí os nomes somem (ficam genéricos) até a regra acompanhar.
  `isper-cli teams-probe` mostra o que a janela dá e grava árvores
  **anonimizadas** em `fixtures/teams` para os testes.
- Sem o painel Pessoas aberto, quase nada é nomeado: a presença valida quem
  foi marcado falando, e o Teams marca quem fala mais pelo anel do vídeo do
  que por texto.
- Diarização que parte uma pessoa em dois grupos deixa essa pessoa sem nome
  (é o caso "nome disputado"): conservador por escolha.
- O passe final de uma sessão com a opção ligada não substitui a transcrição
  se um falante foi renomeado no meio — a edição fica, e a transcrição
  provisória também. Sem a opção, nada muda.
- Um downgrade para antes do schema v10 precisa do `isper.db.v9.bak` (cópia
  automática antes de migrar) ou de um procedimento que leve as reuniões
  novas junto.

## Alternativas consideradas

- **Aplicar o nome direto no rótulo** (como o Yogurt faz). Simples, mas uma
  atribuição errada viraria ata, resumo e memória do assistente.
- **Ler o Teams dentro do processo do app.** Uma chamada UIA presa num Teams
  travado prenderia a thread de quem a fez; o subprocesso no Job Object
  isola o pior caso.
- **Sem exigir consenso no grupo da diarização.** Mais nomes, mas um segundo
  de atraso da tela basta para trocar duas pessoas que se interrompem.
- **API do Graph / bot na reunião.** Pede conta, consentimento do tenant e
  rede; o ISPer é local ([0003](0003-llm-na-nuvem-so-texto.md)).
- **Teams na web e outros apps (Zoom, Meet).** Ficam para etapas próprias: a
  árvore e a origem do áudio são outras.

## Onde vive

`crates/isper-core/src/speaker_names/` (árvore, leitura do Teams,
atribuição, UI Automation), `loopback::CaptureBinding`,
`MeetingStore::{publish_final_segments, get_meeting_for_library,
clear_detected_speakers, speaker_rev}`, `apps/isper-app/src-tauri/src/teams_names.rs`
(subprocesso, Job Object, sessão), `final_pass.rs`, `library.html`,
`settings.html`, `crates/isper-cli/src/teams_probe.rs`, `fixtures/teams/` e o
e2e `tools/e2e/teams-names.ps1`.
