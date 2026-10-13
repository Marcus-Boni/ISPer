# 0025 — "Quando der tempo" e projetos locais: listas sem data, e o projeto como lente que não depende do OptTime

- **Status:** aceita
- **Data:** 12/10/2026

## Contexto

Duas coisas ficavam de fora da tela Hoje ([0021](0021-assistente-pessoal-no-isper.md)):

- **O que não tem data.** Anotações para estudar ou ler "quando der tempo",
  sem pressa e sem dia. Tratadas como tarefa, elas lotam o dia, o contador
  fica vermelho e a pessoa passa a ignorar a lista inteira; deixadas numa
  nota solta, somem de vista.
- **Os projetos.** A pessoa trabalha em vários ao mesmo tempo, alguns com
  horas no OptTime e outros só dela (estudos, o próprio ISPer). Tarefas,
  rotinas e reuniões não diziam de qual projeto eram.

O OptTime tem a lista de projetos em que se aponta hora
(`opt_time_list_projects`), mas é uma integração opcional
([0022](0022-conectores-mcp-e-opttime.md)): o ISPer precisa funcionar sem ela.

## Decisão

### Listas "Quando der tempo" (11.0)

- **Listas livres, do ISPer** (tabela `lists`): a pessoa cria "Estudar",
  "Ler", "Ideias". Arquivar uma lista não apaga nada.
- **O item de uma lista é uma tarefa sem dia com `list_id`.** Herda de graça o
  diário, o desfazer, o celular e o assistente.
- **O item sai do dia:** não entra em "Para hoje", na caixa de entrada, em
  "Depois" nem no número da barra lateral. Zero pressão.
- **Agendar dá um dia ao item**, e ele vai para o Hoje como qualquer tarefa,
  sem perder a lista: voltar para "algum dia" o devolve à lista; concluir o
  tira de vez.
- **Estimativa opcional** (15, 30, 60 ou 120 min), para caber num tempo livre.
- **Volta na hora certa, sugerido e nunca imposto:** com o OptTime conectado,
  as janelas livres da agenda entre agora e o fim do expediente viram a
  sugestão de um item que caiba nelas (o que espera há mais tempo primeiro;
  "Outra" pula para o próximo). Sem agenda, nada é sugerido sozinho; o
  assistente responde "o que dá para fazer em 30 min?".
- **Revisão contra o cemitério:** o item sem mudança há 60 dias ganha o selo
  "parado"; "Revisar parados" mostra só esses, com manter, agendar ou
  descartar. Manter conta como mudança (vai para o diário).

### Projetos locais (v9 agora, tela na 11.1)

- **O projeto é do ISPer** (tabela `projects`: nome, cor, arquivado). Uma
  integração só preenche: `external_ref = opttime:<id>` liga um projeto local
  a um do OptTime, e um projeto sem ela é tão completo quanto.
- **O projeto é uma lente, não um gerenciador:** tarefas (e itens de lista),
  rotinas e reuniões ganham `project_id`; os ditados, não.
- **O schema entra inteiro agora (v9):** as listas da 11.0, os projetos e as
  três colunas de projeto, numa migração só, para a 11.1 não mexer de novo no
  banco.

### Projetos na prática (11.1, decidido em 12/10)

- **A reunião já sai marcada, e a pessoa corrige.** Casada com um evento da
  agenda, ela ganha o projeto da reunião anterior da mesma série (ou do
  mesmo evento); sem série com projeto, o nome ou um apelido que aparece no
  assunto como palavra inteira, o mais longo primeiro. Empate entre dois
  projetos não marca nada. Acertando, zero toques; errando, um.
- **"Sugerido" vem do diário, não de uma coluna nova.** A marca do ISPer vai
  para o diário com o assistente como autor; enquanto a última marca não for
  da pessoa, a tela mostra "sugerido". Marcar o mesmo projeto confirma.
- **As tarefas da reunião herdam o projeto** e acompanham a troca: as ainda
  abertas que estavam com o projeto de antes mudam junto, cada uma no diário.
- **Do OptTime, a pessoa escolhe quais trazer** (os projetos ativos em que ela
  aponta horas). Um projeto local sem ligação e de mesmo nome é ligado, e o
  código de lá vira apelido; trazer de novo não duplica.
- **Os projetos se cuidam nas Configurações → Projetos**; o Hoje e a
  Biblioteca têm o selo, o seletor e o filtro. O assistente filtra tarefas e
  reuniões por projeto (`projeto`) e lista os projetos (`projetos`).
- **As rotinas ficam sem projeto por enquanto:** a coluna existe, mas o
  critério da 11.1 é de tarefas e reuniões.

## Consequências

- O "Depois" da tela Hoje deixa de mostrar as tarefas sem dia que estão numa
  lista; o "Depois" do celular só aprende a separar as listas quando ele
  ganhar a tela delas.
- A migração v9 só acrescenta (tabelas e colunas que aceitam vazio); o banco
  de antes fica numa cópia `isper.db.v8.bak`, como em toda migração.
- A sugestão de tempo livre depende da agenda; sem o OptTime, as listas
  continuam inteiras, só não aparecem sozinhas.

## Alternativas consideradas

- **Lista fixa "Para estudar":** mais simples, mas a pessoa já separa estudo,
  leitura e ideias.
- **Um tipo novo, separado das tarefas:** teria de refazer o diário, o
  desfazer, a sincronia com o celular e as ferramentas do assistente.
- **Projeto como texto livre (`area`):** o campo existe desde a v7, mas sem
  identidade não dá para renomear, juntar apelidos nem ligar ao OptTime.
- **Importar do Notion:** dispensada; a lista é pequena e cabe num copiar e
  colar.

## Onde vive

`crates/isper-core/src/store.rs` (migração v9), `crates/isper-assist`
(`model.rs`, `store/lists.rs`, `spare.rs` para o tempo livre,
`store/projects.rs` para os projetos e o da reunião), `crates/isper-mcp`
(`opt_time_list_projects`), `crates/isper-agent` (as ferramentas
`quando_der_tempo` e `projetos`, o filtro `projeto`), a seção "Quando der
tempo" da tela Hoje, a seção Projetos das Configurações e o `isper-cli listas`
e `projetos`.
