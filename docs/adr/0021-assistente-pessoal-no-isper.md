# 0021 — O assistente pessoal mora no ISPer, com tarefas, rotinas, memória e diário locais

- **Status:** aceita
- **Data:** 07/10/2026

## Contexto

As tarefas do dia eram anotadas à mão no Notion, uma por uma, inclusive as que
se repetem todo dia (registrar 8 horas no OptTime) e as que saem das reuniões.
O pedido foi um assistente rápido e amigável que capture tarefas por voz, leia
as reuniões que já aconteceram e as que vêm, aprenda as rotinas, diga o que já
foi feito no dia e deixe tudo rastreável num lugar só.

A leitura dos dois projetos em 07/10 mostrou que quase tudo já existia, mas
dividido:

- **ISPer:** ditado com Whisper local, reuniões com passe final e resumo,
  ações com dono e prazo confirmadas no Copilot (tabela `decisions`),
  `LlmProvider` com Claude, Groq e Gemini, janela única com Ctrl+K
  ([0020](0020-janela-unica.md)), celular com sincronia ([0017](0017-sincronia-celular-pc.md)).
- **OptTime** (o apontamento de horas da OptSolv): Microsoft Graph delegado
  com a agenda consentida, Azure DevOps, o "Preencher meu dia" e um servidor
  MCP com token pessoal.

Tarefas não existiam em nenhum dos dois.

## Decisão

- **O assistente é uma parte do ISPer**, não um app novo nem uma tela do
  OptTime. Ganha a tela **Hoje** na barra lateral da janela única, a
  captura rápida por atalho global (voz ou texto) e respostas pela paleta.
- **Domínio novo no crate `isper-assist`**, com quatro tabelas no mesmo
  `isper.db`:
  - **tarefa**, sempre com a origem (voz, minuto da reunião, Copilot,
    work item, Notion, rotina);
  - **rotina**, com recorrência em RRULE, um verificador e uma ação;
  - **memória**, com fatos curtos sobre o usuário, visíveis e editáveis;
  - **diário**, que só cresce, registra quem fez o quê e como desfazer, e é
    a fonte de "o que já fiz hoje".
- **O Notion é importado uma vez** pela API REST, com token de integração
  interna, e deixa de ser a fonte das tarefas.
- **Código determinístico onde dá, LLM onde agrega.** Datas, recorrência e
  verificações são código. Entender fala solta e responder perguntas ficam
  com a LLM, com saída em esquema estrito e validado.
- **O modelo é o Gemini 3.8 Flash** (`gemini-3.8-flash`), pelo provedor que
  já existe, **na cota paga**: na gratuita o Google pode usar o conteúdo para
  melhorar produtos, com revisão humana, e o assistente lida com conteúdo de
  reunião da empresa. GLM-5.3-Flash e Claude Haiku 5.5 são medidos contra ele
  no corpus da 10.1 antes de qualquer troca.

## Consequências

- O que é pessoal (tarefas, memória, diário) fica no PC do usuário, fora do
  banco da empresa. Para a LLM vai só texto, como já valia
  ([0003](0003-llm-na-nuvem-so-texto.md)).
- A tela Hoje abre sem esperar rede. O que vem do OptTime chega depois e fica
  em cache.
- O ISPer é público e genérico: o OptTime entra como um conector configurado
  ([0022](0022-conectores-mcp-e-opttime.md)), não como código fixo.
- O `LlmProvider` precisa ganhar chamada de ferramentas nos três provedores
  (10.4).
- Custo estimado com o Gemini 3.8 Flash, sem medição: ~US$ 5 por mês num uso
  diário de 10 ditados, 10 perguntas e 3 reuniões, a US$ 0,75 / 3,75 por milhão
  de tokens (preço de lançamento até o fim de 2026).
- Descartar e concluir seguem [0009](0009-nada-some-sem-o-usuario-pedir.md):
  nada some sem o usuário pedir, e o diário guarda o caminho de volta.

## Alternativas consideradas

- **Dentro do OptTime.** Já tem agenda, DevOps e um chat com ferramentas, mas
  não tem voz nem reuniões gravadas, não funciona sem rede, não tem celular,
  e tarefas pessoais ficariam no Postgres da empresa.
- **App novo.** Refaria voz, reuniões, celular, distribuição e marca, que o
  ISPer já tem.
- **Tarefas continuam no Notion.** Cada base tem um formato diferente, a API
  permite cerca de 3 chamadas por segundo e a tela dependeria da rede. O MCP
  hospedado do Notion exige OAuth com a pessoa presente.
- **Microsoft To Do.** Exigiria escopo de escrita novo no Entra da empresa e
  manteria as tarefas fora do alcance local.
- **Claude Opus 5.5 como modelo.** Melhor qualidade esperada, mas ~US$ 17 por
  mês no mesmo uso. Fica como referência de qualidade no corpus.
