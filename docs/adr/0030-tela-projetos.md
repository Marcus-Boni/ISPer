# 0030 — A tela Projetos: as reuniões organizadas por projeto, no lugar das pastas

- **Status:** aceita
- **Data:** 22/11/2026

## Contexto

A pessoa guardava cada reunião no Gerenciador de Arquivos, organizava as
pastas à mão e apagava à mão. Com os projetos da 11.1
([0025](0025-quando-der-tempo-e-projetos-locais.md)), cada reunião já sabe
de que projeto é, mas não havia um lugar para ver "tudo do projeto X" nem
para arrumar as que ficaram sem projeto. O roadmap (11.2) pedia a página do
projeto.

## Decisão

- **Uma tela própria na barra lateral, "Projetos"** (a sexta da janela
  única, [0020](0020-janela-unica.md)): à esquerda, os projetos ativos com
  quantas reuniões e tarefas abertas têm e quando foi a última; "Sem projeto"
  em "Para organizar"; os arquivados recolhidos.
- **A página do projeto**: números do projeto (reuniões, tempo em reunião,
  tarefas abertas, última reunião), as tarefas abertas, o que se validou no
  Copilot nas reuniões dele (com o link para o minuto) e as reuniões, por mês,
  com o começo do resumo. Com mais de cinco, um filtro.
- **É uma lente:** cuidar dos projetos continua nas Configurações
  ("Gerenciar projetos"), e a reunião abre na Biblioteca.
- **Organizar no lugar:** em "Sem projeto", cada reunião tem "Mover para…".
  Cada reunião aparece em exatamente um grupo: no projeto dela (ativo ou
  arquivado) ou em "Sem projeto" (inclusive se o projeto não existe mais).
- **Excluir** usa o "Excluir do histórico" de sempre, com Desfazer. O `.md`
  fica na pasta, como na Biblioteca; a retenção das Configurações apaga
  o `.md` e as exportações de mesmo nome.

## Fora, por ora

- **Horas da semana pelo OptTime:** o conector só lê o resumo de um dia por
  vez.
- **Work items:** o conector não os expõe.
- **Sugerir o lançamento de horas no projeto:** escreve no OptTime, pede o
  seu próprio desenho de confirmação ([0023](0023-escada-de-confianca.md)).

## Onde vive

`apps/isper-app/ui/projects.html`, `apps/isper-app/src-tauri/src/projects.rs`
(`projects_overview`, `project_page`, `open_projects_window`),
`MeetingStore::meeting_summary`, `AssistStore::open_tasks_in_project` e
`open_task_counts`, e o e2e `tools/e2e/projects.ps1`.
