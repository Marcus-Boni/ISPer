# 0001 — Registrar as decisões de arquitetura em ADRs

- **Status:** aceita
- **Data:** 23/09/2026

## Contexto

Em um mês o ISPer passou de um protótipo de terminal a um app com ditado,
notetaker, diarização, IA, Copilot, dois instaladores e atualização
automática. As decisões que explicam o código estavam espalhadas: tabelas no
`ROADMAP.md`, parágrafos no `README.md`, comentários de módulo, mensagens de
commit e descrições de PR. Algumas só existiam na memória de quem as tomou —
por exemplo, por que o loopback não usa o `cpal`, ou por que a diarização não
roda durante a reunião.

## Decisão

Cada decisão estrutural ganha um arquivo curto em `docs/adr/`, numerado, com
contexto, decisão, consequências, alternativas descartadas e onde a decisão
vive no código. O índice e o modelo ficam em [`README.md`](README.md).

Entra num ADR o que é caro de desfazer ou fácil de desfazer sem querer:
escolha de stack, formato de dados, dependência que muda o build, política
que protege o usuário. Não entra o que o próprio código já explica.

## Consequências

- Mudar uma dessas decisões passa a começar pela leitura do ADR — e, se a
  mudança for adiante, por um ADR novo que o substitui.
- O `ROADMAP.md` continua sendo o plano e o `CHANGELOG.md` o que mudou para o
  usuário; os ADRs são o *porquê* técnico. Os três se referenciam, não se
  repetem.
- Custo: manter o índice atualizado a cada decisão nova.

## Alternativas consideradas

- **Só o ROADMAP.** Ele diz o que foi feito e quando, mas mistura plano,
  estado e justificativa; uma decisão revista (a diarização, em 18/09) some
  na edição.
- **Wiki do GitHub.** Fica fora do repositório: não passa por PR, não versiona
  junto com o código que descreve.

## Onde vive

`docs/adr/`. O formato é uma variação enxuta do
[MADR](https://adr.github.io/madr/).
