# 0028 — "Otimizar": uma versão limpa de cada fala, ao lado da original

- **Status:** aceita
- **Data:** 22/11/2026

## Contexto

A transcrição automática traz o que a fala tem: hesitações ("é…", "hum",
"tipo"), repetições, falsos começos e palavras que soam parecidas e saem
erradas. Para ler a reunião, ou para entregá-la a outro agente de IA, isso
atrapalha. A pessoa pediu um botão que deixasse a reunião limpa
(Fase 11.3, [ROADMAP](../../ROADMAP.md)).

O resumo já sai da IA, mas não substitui a transcrição: quem quer saber o que
foi dito precisa das falas. E a transcrição é a prova do que foi dito; o
ISPer não inventa ([0021](0021-assistente-pessoal-no-isper.md)).

## Decisão (do usuário, 22/11)

- **Limpeza fiel, não reescrita.** A IA tira hesitações, repetições e falsos
  começos e corrige erro evidente de transcrição, pontuação e maiúsculas.
  Não resume, não junta nem divide falas, não troca nomes, termos, números
  nem datas. O trecho ambíguo fica como está, com `[?]`: marcar, não
  reinterpretar.
- **Ao lado, nunca no lugar.** Cada fala ganha `segments.clean_text`
  (schema v11): vazia é a fala que era só hesitação; nula, a que fica como
  está. A original não muda. O passe final apaga e regrava as falas, e a
  versão limpa vai junto (otimiza-se de novo).
- **Só no botão.** "Otimizar" na reunião da Biblioteca; nada roda sozinho, e
  o custo só existe quando a pessoa pede.
- **Fala por fala, pelo id.** As falas vão numeradas pelo id do banco, em
  lotes de ~8 mil caracteres (a reunião longa não esbarra no limite do
  resumo), com saída estruturada; voltam pelo mesmo id. Horário e falante não
  passam pela IA. Um guarda-corpo descarta a resposta que cresceu ou encolheu
  demais (fica a original), o id que não foi pedido e a fala longa que voltou
  vazia. Se o passe final trocar as falas no meio, nada é gravado.
- **Onde a versão limpa vale:** a tela (com o alternador Otimizada/Original),
  copiar, exportar (`-otimizada` ao lado do arquivo) e o assistente, que cita
  a fala limpa quando ela existe. A ata `.md`, o resumo e a busca continuam
  na original.

## Consequências

- Só o texto das falas sai da máquina, para o provedor de IA configurado. Uma
  reunião de 1 h custa cerca do dobro de um resumo.
- Otimizar antes do passe final é trabalho perdido: o passe troca as falas.
- O "formato para agente" (cabeçalho estruturado, nome padronizado do `.md`)
  ainda é da 11.3, à parte.

## Alternativas consideradas

- **Reescrita fluida** (ata editada): lê melhor, mas se afasta do que foi
  dito e arrisca mudar o sentido.
- **Rodar sozinho depois do passe final:** tudo pronto sempre, com custo em
  toda reunião.
- **Um texto limpo inteiro numa coluna da reunião:** perde o horário e o
  falante de cada fala, e a reunião longa não cabe numa chamada.

## Onde vive

`crates/isper-llm/src/cleanup.rs` (`clean_transcript`), `crates/isper-core`
(`store.rs`: migração v11, `segments_for_cleanup`, `set_clean_texts`),
`apps/isper-app` (`library.rs`: `optimize_meeting` e o `clean` do
`export_meeting`; `library.html`: o botão e o alternador),
`crates/isper-agent/src/local.rs` (`said`) e o e2e `tools/e2e/optimize.ps1`.
