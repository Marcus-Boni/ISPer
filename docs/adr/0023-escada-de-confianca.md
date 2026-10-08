# 0023 — Escada de confiança: ler é livre, escrever pede um toque, automático só com verificador

- **Status:** aceita
- **Data:** 07/10/2026

## Contexto

O assistente ([0021](0021-assistente-pessoal-no-isper.md)) lê e escreve em
sistemas de fora pelo MCP ([0022](0022-conectores-mcp-e-opttime.md)).
Registrar horas no OptTime alimenta a folha de pagamento, então um lançamento
errado custa caro. Ao mesmo tempo, pedir confirmação para tudo faz o
assistente virar um formulário a mais.

Os produtos de referência resolvem isso por degraus. O Claude Code permite,
pergunta ou nega por ferramenta. O TimeBot do OptTime tem os modos "sempre
perguntar", "inteligente" e "piloto automático", com log de ações e
desfazer. Os planejadores com IA (Sunsama, Morgen) sugerem e deixam a decisão
com a pessoa.

## Decisão

- **Degrau 1, ler é livre.** Ferramentas com `readOnlyHint: true` (agenda,
  resumo do dia, work items, tarefas, reuniões) rodam sem perguntar.
- **Degrau 2, escrever pede um toque.** Qualquer ferramenta que muda algo
  fora do ISPer vira um cartão de confirmação com o que vai acontecer. As
  com `destructiveHint: true` sempre perguntam.
- **Degrau 3, automático só se o usuário liberar,** e só para rotinas com
  verificador. Exemplo: às 17:00 de dia útil, se
  `opt_time_get_today_summary` mostra o dia completo, a rotina "Registrar 8h"
  se marca como feita sem aviso. Se falta, o assistente avisa e a aplicação
  das sugestões espera o toque.
- **Permissão por ferramenta, por conector:** perguntar, liberar ou nunca.
  O padrão vem das `annotations` do servidor e o usuário muda em
  Configurações.
- **Tudo vai para o diário,** com a origem e, quando der, o desfazer.
  Escritas no OptTime levam `idempotencyKey`, para um repique de rede não
  duplicar lançamento.
- **Aprender é sugerir.** Padrões viram sugestão de rotina com a evidência ao
  lado. O assistente nunca liga uma rotina, muda uma permissão ou grava uma
  memória sem o usuário aceitar.
- **Cada lembrete tem um dono só.** Com a rotina das 8 horas ligada, o aviso
  das 17:30 do OptTime (`eveningDigestEnabled`) deve ficar desligado.

## Consequências

- O primeiro uso é mais lento que um piloto automático, de propósito. A
  confiança cresce por rotina, com o histórico do diário à vista.
- O ISPer precisa da tela de permissões e do cartão de confirmação antes do
  agente com ferramentas (10.4). As rotinas da 10.2 já usam o degrau 2.
- `opt_time_whoami` informa `eveningDigestEnabled`, então a tela do conector
  pode lembrar de desligar o aviso duplicado.

## Alternativas consideradas

- **Sempre perguntar.** Seguro, mas o assistente não tiraria trabalho.
- **Piloto automático por padrão.** Rápido, mas um erro de projeto ou de
  duração vai direto para a folha.
- **Confiança decidida pela LLM** (o modelo julga o risco). Não é previsível
  nem auditável. A decisão fica com o usuário e com as marcações do servidor.
