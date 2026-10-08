# 0022 — Conectores por MCP, com o OptTime como porta corporativa

- **Status:** aceita
- **Data:** 07/10/2026

## Contexto

O assistente ([0021](0021-assistente-pessoal-no-isper.md)) precisa da agenda
do Outlook, dos work items do Azure DevOps e das horas do OptTime. O ISPer não
falava com nenhum serviço externo além dos provedores de LLM.

Caminhos possíveis para cada fonte em 07/10/2026:

- **Agenda:** um app próprio do ISPer no Entra dependeria do admin do tenant
  da OptSolv. O OptTime já tinha `Calendars.Read` e `OnlineMeetings.Read`
  consentidos e um token de fundo que se renova sozinho
  (`getBackgroundMicrosoftToken`).
- **Azure DevOps:** o MCP remoto da Microsoft ficou GA em 05/08/2026, mas só
  aceita clientes da Microsoft (falta registro dinâmico de cliente no Entra).
  O OptTime já tinha o PAT de cada pessoa.
- **Horas:** o OptTime já tinha um servidor MCP hospedado (`/api/mcp`,
  protocolo `2025-06-18`) com 16 ferramentas e token pessoal.

## Decisão

- **O ISPer é cliente MCP** (crate `rmcp`, o SDK oficial em Rust, transporte
  HTTP "streamable"). Conectores são configuração: URL, token no Credential
  Manager e política de permissões por ferramenta.
- **O OptTime é a porta corporativa.** Tudo que é da empresa (agenda,
  DevOps, horas) passa pelo MCP hospedado dele, com um token pessoal do
  preset "Assistente pessoal (ISPer)" (`time:read`, `time:write`,
  `calendar:read`). Nada novo no Entra.
- **Ferramentas do OptTime usadas pelo assistente** (OptTime v1.11.0,
  08/10/2026): `opt_time_get_my_agenda`, `opt_time_list_my_work_items`,
  `opt_time_suggest_daily_entries` (a mesma lógica do "Preencher meu dia" da
  web), `opt_time_apply_suggestions` (transação e `idempotencyKey`),
  `opt_time_get_today_summary` (`isWorkday`, `targetMinutes`) e
  `opt_time_whoami`. Especificação no repositório do OptTime:
  `docs/superpowers/specs/2026-10-07-isper-assistente-porta-corporativa-design.md`.
- **MCP como porta única, não o REST.** O catálogo traz nome, descrição,
  `inputSchema`, `outputSchema` e `annotations`. O agente repassa as
  ferramentas para a LLM sem release nova do ISPer, e as telas e rotinas leem
  o `structuredContent`. O REST v1 do OptTime espelha as mesmas ferramentas
  para o pacote npm dele, e o ISPer não o usa.
- **O token nunca passa por chat nem por arquivo.** Fica no Credential
  Manager pelo `keyring` (serviço `ISPer`, usuário `opttime`, alvo
  `opttime.ISPer`), como a chave do Jev ([0019](0019-filtro-do-copilot-pelo-jev.md)).
- **O ISPer consulta; o OptTime não empurra.** Desktop atrás de NAT não
  recebe webhook. A agenda tem cache de 60 s no servidor; o ISPer consulta a
  cada poucos minutos e na hora de cada verificação.

## Consequências

- A agenda e o DevOps só funcionam para quem tem conta no OptTime. Sem
  conector, o assistente segue com tarefas, rotinas locais e reuniões
  gravadas.
- Erros do conector têm código e dica de correção (`MICROSOFT_NOT_CONNECTED`,
  `AZURE_DEVOPS_NOT_CONFIGURED`, `INSUFFICIENT_SCOPE`, `IDEMPOTENCY_CONFLICT`),
  que a tela de conectores mostra como estão. Eles chegam em
  `_meta["opt-time/error"]` (`code`, `message`, `hint`, `details`), e não no
  `structuredContent`: o SDK oficial valida o `structuredContent` contra o
  `outputSchema` mesmo quando `isError` é verdadeiro (corrigido no OptTime em
  08/10, `d5a919c`).
- Outros servidores MCP (Notion, um DevOps que aceite o ISPer no futuro)
  entram pelo mesmo cliente, sem código novo por integração.
- Os esquemas de entrada do OptTime são simples (sem `$ref`, `oneOf`, `anyOf`,
  `allOf`). O ISPer ainda limpa o que o provedor não aceitar, como
  `additionalProperties` no Gemini.

## Alternativas consideradas

- **App próprio do ISPer no Entra.** Depende de consentimento do admin e
  duplicaria o refresh de token que o OptTime já faz.
- **REST v1 do OptTime.** Tipado e com cache HTTP, mas sem catálogo de
  ferramentas para a LLM nem marcações de leitura e escrita. Seriam dois
  contratos para manter no ISPer.
- **Conector MCP da API da Anthropic** (o servidor da Anthropic chama o MCP).
  Prende o assistente a um provedor e manda o token do OptTime para fora do
  PC.
- **MCP remoto oficial do Azure DevOps.** Não aceita o ISPer como cliente em
  08/10/2026.
