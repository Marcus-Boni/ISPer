# 0009 — Nada some sem o usuário pedir: retenção "para sempre" e Desfazer

- **Status:** aceita (ampliada em 23/09/2026 com o Desfazer)
- **Data:** 14/09/2026

## Contexto

A fase 7.4 trouxe retenção de dados: apagar reuniões e ditados depois de N
dias, em linha com o princípio da LGPD de guardar só o necessário. A
primeira versão sugeria um prazo. O usuário vetou: **o padrão tem de ser
nunca apagar**, e nada pode sumir de repente para quem usa o app. Uma
reunião gravada é, muitas vezes, o único registro de uma conversa.

O mesmo raciocínio vale para exclusões manuais: o "clique de novo para
confirmar" protegia mal (vira reflexo) e, uma vez confirmado, não tinha
volta.

## Decisão

- **Retenção padrão: para sempre.** Um prazo (30, 90, 180 dias ou 1 ano) só
  vale depois de escolhido **e confirmado** na tela; encurtar o prazo pede a
  mesma confirmação.
- **Backup antes de qualquer expurgo:** a varredura grava uma cópia íntegra
  do banco em `Documentos\ISPer\Backups` antes de apagar (ficam as três
  últimas), então o que saiu continua recuperável.
- **Exclusão com Desfazer:** apagar reunião, ditado ou modelo faz o item
  sumir da tela na hora e mostra *Desfazer* (ou Ctrl+Z) por 7 s; o app só
  apaga de fato quando a janela passa. Sair do ISPer nesse intervalo aplica o
  que estava pendente.
- **O arquivo `.md` da reunião não é apagado** ao excluir do histórico; ele
  continua na pasta de Reuniões.

## Consequências

- Nenhum dado sai do disco por uma configuração que o usuário não escolheu.
- O banco cresce indefinidamente no padrão; o Diagnóstico mostra o tamanho e
  o backup manual existe para quem quiser arquivar.
- Excluir fica em dois tempos no backend (agendado e confirmado): listas e
  buscas precisam esconder o que está pendente (`undo::hidden_*`), e o
  encerramento do app precisa aplicar o que ficou na fila.

## Alternativas consideradas

- **Retenção padrão de 90 dias.** Mais alinhada à minimização da LGPD, mas
  apagaria reuniões de quem nunca abriu as Configurações.
- **Lixeira permanente.** Mais complexa (outra tela, outra política de
  expiração) do que o problema pedia; o backup automático cobre o expurgo.
- **Confirmação por diálogo.** Interrompe e vira reflexo; o Desfazer não
  interrompe e dá tempo de perceber o erro.

## Onde vive

`apps/isper-app/src-tauri/src/data.rs` (retenção e backups),
`apps/isper-app/src-tauri/src/undo.rs` (fila com token, `UNDO_WINDOW`),
`apps/isper-app/ui/assets/ui.js` (`UI.undoable`), fases 7.4 e 7.5 do
`ROADMAP.md`.
