# Fixtures do Teams

Árvores de acessibilidade de janelas de chamada do Microsoft Teams, no
formato de `isper_core::speaker_names::tree` (`r` papel, `t` nome, `d` ajuda,
`i` AutomationId, `c` classes, `k` filhos). Os testes de
`crates/isper-core/src/speaker_names/teams.rs` leem estes arquivos.

**Todas são sintéticas**: montadas a partir do vocabulário que o Teams
mostra no painel Pessoas ("Nome, Papel, Estado do microfone", "(Você)") e
das medições publicadas pelo Yogurt para o inglês. Nenhuma tem nome, chat ou
título de reunião de verdade.

Para trazer uma árvore real (rodada de validação da seção 1 do plano), use
a sonda com a chamada aberta e o painel Pessoas visível:

```powershell
isper-cli teams-probe --fixture fixtures\teams\real-pt-br-<cenário>.json
```

O arquivo sai **anonimizado** (`speaker_names::teams::anonymize`): os nomes
viram "Pessoa A", "Pessoa B"…, o vocabulário do Teams fica, e qualquer outro
texto (título, chat, legenda) vira só o tamanho dele (`‹23›`). Confira o
arquivo antes de fazer commit.
