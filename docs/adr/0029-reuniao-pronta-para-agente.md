# 0029 — A reunião pronta para agente: um Markdown inteiro, sem mexer na ata

- **Status:** aceita
- **Data:** 22/11/2026

## Contexto

A pessoa quer parar de organizar transcrições em pastas e entregar uma
reunião inteira a outro agente de IA, que responda sobre ela sem pedir
contexto (Fase 11.3). A ata `.md` de sempre tem título, data, duração e as
falas, mas não diz o projeto, quem são "Eu" e "Participante N", nem traz o
que foi validado no Copilot; e o nome do arquivo é `reuniao-AAAAMMDD-HHMMSS`.

Mudar a ata custa caro: o formato é escrito em ~27 lugares e o `import` o lê
de volta para reconstruir a Biblioteca.

## Decisão

- **Um formato à parte, gerado do banco, sem IA**
  (`isper_core::agent_doc`): cabeçalho YAML (tipo, título, data ISO, início,
  duração, projeto, participantes, origem, evento da agenda, se a transcrição
  é a otimizada), uma linha de contexto ("Eu" é quem gravou; "Participante N"
  são as outras vozes; transcrição automática, com o que o "Otimizar" fez),
  o resumo (com os títulos um nível abaixo), o que foi validado no Copilot
  (tipo, responsável, prazo, minuto), as notas, os momentos marcados e as
  falas com horário.
- **Num toque:** "Copiar para agente" na reunião da Biblioteca. E "Agente" nas
  exportações, com o **nome padronizado** `aaaa-mm-dd hhhmm - Projeto -
  Assunto.md` (sem projeto, sem o meio; sem os caracteres que o Windows
  recusa), na pasta das reuniões.
- **A versão da tela:** com o alternador em Otimizada, vai a limpa
  ([0028](0028-otimizar-a-transcricao.md)).
- **A ata não muda**: nome e formato continuam os de sempre.

## Consequências

- O que o agente recebe é o que o banco sabe; um resumo ou um projeto que
  falta simplesmente não aparece (o projeto sai como `null`).
- Exportar de novo sobrescreve o arquivo de mesmo nome.

## Alternativas consideradas

- **Trocar o formato e o nome da ata:** alcança tudo de uma vez, mas mexe no
  que o import lê e nos 27 lugares que a escrevem.
- **Pedir à IA um documento "para agente":** custa e pode inventar; o que é
  só reorganizar não precisa de IA.
