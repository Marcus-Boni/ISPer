# 0024 — Tarefas no bolso: o PC é a fonte, o celular manda mudanças com a hora

- **Status:** aceita
- **Data:** 10/10/2026

## Contexto

As tarefas e as rotinas moram no `isper.db` do PC
([0021](0021-assistente-pessoal-no-isper.md)). A Fase 10.6 leva as duas
para o celular, para ditar uma tarefa na rua e ver o dia sem abrir o PC. O
celular passa horas sem ver o PC, e os dois mudam a mesma tarefa nesse
tempo: o PC conclui, o celular adia.

A sincronia celular↔PC já existe para as gravações
([0017](0017-sincronia-celular-pc.md)): iroh na rede local, o celular
puxa, o PC responde. Os dados das gravações só vão num sentido. Os das
tarefas vão nos dois.

## Decisão

- **O PC é a fonte.** O celular guarda uma cópia (o último retrato do PC) e
  uma fila das mudanças feitas lá, cada uma com a hora em que foi feita.
- **Um pedido só, `Tasks`, no `isper/sync/1`:** o celular manda a fila e o
  relógio dele; o PC aplica e devolve o retrato (as abertas, a caixa de
  entrada, as feitas nos últimos 7 dias e as rotinas). O celular troca a
  cópia pelo retrato e tira da fila o que o PC respondeu.
- **O último a escrever vence, por campo.** O PC sabe quando cada campo de
  cada tarefa mudou pelo diário, que guarda o antes e o depois. Uma mudança
  do celular num campo que mudou no PC depois dela perde só nesse campo, e o
  celular fica sabendo. A mudança que chega do celular vai para o diário com
  a hora dela (`op_at`), não com a da chegada.
- **O relógio do celular é corrigido** pela diferença entre o "agora" dele,
  que vai no pedido, e o do PC.
- **Criar não duplica:** a tarefa criada no celular entra com
  `external_ref = phone:<id da operação>`, e a mesma operação mandada de
  novo devolve a mesma tarefa. Até o PC responder, ela é `local:<id>` no
  celular, e as mudanças nela, feitas offline, seguem esse id.
- **Sem apagar e sem lápide:** o que sai do retrato (descartada, feita há
  mais de 7 dias) some do celular, nunca do PC.
- **As rotinas vão só para ler.** A tarefa de cada dia continua nascendo no
  PC, que agora também a cria quando o celular sincroniza.
- **O celular parseia as datas com o mesmo código do PC** (`isper_assist::when`),
  sem IA: o que é ditado vira uma tarefa com o dia e a hora entendidos.

## Consequências

- Não há migração de banco: a hora de cada campo vem do diário, que já
  existia para o desfazer.
- O celular mostra a mudança na hora, com ou sem o PC; o PC fica com a
  palavra final, e a tela do celular é refeita a cada retrato.
- Um PC antigo responde que não sincroniza tarefas; o celular anota o motivo
  e não insiste a cada rodada.
- Relógios muito errados (horas) ainda podem dar o campo ao lado errado
  numa disputa de minutos; a correção pelo "agora" do pedido cobre o caso
  comum.
- O retrato inteiro vai a cada sincronia. Com centenas de tarefas, são
  dezenas de KB, longe do limite do quadro (8 MB).

## Alternativas consideradas

- **CRDT por campo nos dois lados** (relógio de Lamport, mescla
  simétrica): resolve sem fonte, mas pede uma tabela de versões por campo
  no PC e o mesmo motor no celular, para um caso (dois aparelhos) em que o
  PC já é a fonte natural.
- **Última versão inteira vence** (a tarefa toda, não o campo): mais
  simples, mas o adiamento feito no celular apagaria a conclusão feita no
  PC.
- **Banco SQLite no celular espelhando o do PC:** a cópia em JSON com a
  fila basta para a tela do dia e não leva a cadeia de migrações para o
  celular.

## Onde vive

`crates/isper-assist/src/phone.rs` (aplicar no PC, o retrato e a fila
aplicada no celular), `crates/isper-sync` (`Request::Tasks`,
`Host::tasks`, `Session::tasks`), `crates/isper-mobile/src/tasks.rs` (a cópia,
a fila e a tela, pelo UniFFI) e a rodada em `crates/isper-mobile/src/sync.rs`;
no PC, `apps/isper-app/src-tauri/src/phone_sync.rs` e o `isper-cli receber
--banco`.
