# 0019 — Filtro do Copilot pelo Jev, na nuvem, e não um classificador local

- **Status:** aceita
- **Data:** 27/09/2026

## Contexto

O Copilot (Fase 8) acha os cards de três jeitos:

- **Pulso:** a cada 45 s, a IA relê até 20 min de conversa.
- **Gatilho de frases fixas** ([`detect_trigger`](../../crates/isper-llm/src/copilot.rs)):
  antecipa a rodada quando alguém diz "fica combinado", "eu envio" etc.
- **Botão Analisar.**

Para medir o gatilho, 450 parágrafos de 29 reuniões reais da Biblioteca foram
sorteados e rotulados à mão (decisão, ação, risco ou nada). Um segundo
anotador, às cegas, concordou com κ = 0,77.

**O gatilho pegou 5% dos momentos que viram card** (F1 0,08). Na prática, quem
acha os cards é o pulso, com atraso de até 45 s e relendo 20 min de conversa
a cada volta.

A meta era um classificador por parágrafo, com duas restrições do usuário:
**não pesar na máquina** (hoje o app usa ~300 MB de RAM) e **custo
desprezível**. Os números completos estão em
[`docs/estudos/gemini-transcribe-e-jev.md`](../estudos/gemini-transcribe-e-jev.md),
§12.

## Decisão

**O Copilot ganha um filtro opcional pelo Jev, da TypeSafe.**

- **O que é o Jev:** um modelo "System One", que só decide e não escreve.
  Pergunta: "este parágrafo é decisão, ação, risco ou nada?".
- **Quando lê:** cada parágrafo fechado da conversa, pela regra da ata (mesmo
  falante, pausa de até 4 s, até 60 s), com o parágrafo anterior como
  contexto.
- **O que acontece com o resultado:**
  - com p(card) ≥ 0,5, sai uma **rodada focada**: a IA de sempre lê só os
    últimos 3 min, com os trechos marcados em destaque, e ela é quem decide e
    redige;
  - o gatilho de frases fixas se cala;
  - o pulso completo passa a sair a cada 3 min, como rede de segurança.
- **Medido nos mesmos 450 parágrafos:**
  - pega **92%** dos momentos e manda só **45%** dos parágrafos adiante;
  - AP 0,74, contra 0,24 do acaso;
  - ~0,4 s por parágrafo;
  - **custo:** a medição mandou três perguntas (702–744 tokens). Em produção
    vai só a do filtro, que dá **552 tokens por parágrafo (~US$ 0,000023)**,
    medidos pelo "Testar conexão" do app. Com os ~230 parágrafos por hora das
    reuniões do usuário, são **~US$ 0,005 por hora**.
- **O Jev não cria card sozinho**, porque a precisão dele a 0,5 é 0,49.
- **Configuração** `copilot_filter = "off" | "jev"`, **desligado por padrão**
  ([0011](0011-opcional-e-configuracao-nao-feature-flag.md)). Vale a partir da
  próxima reunião.
- **Chave** no Credential Manager (`typesafe.ISPer`), em header, pelas regras
  da [0003](0003-llm-na-nuvem-so-texto.md). Só vai o texto do parágrafo e do
  anterior.
- **Pergunta fixada:** texto, opções e ordem são exatamente os medidos. O
  modelo também é fixado (`jev-1.13.0`). Mudar qualquer um dos dois pede nova
  medição.
- **Falha aberta:**
  - rede ou 5xx: uma nova tentativa, e depois o trecho vai à IA sem marca;
  - chave recusada, sem crédito ou resposta ilegível: desliga o filtro
    **naquela reunião**, com o motivo no HUD, e volta o pulso de 45 s.

## Consequências

- **Os cards chegam segundos depois de a frase terminar, e não no pulso.**
- **A IA do resumo lê menos:** rodadas de 3 min no lugar de janelas de 20 min
  a cada 45 s. Reduz o custo dela e o texto que sai para ela.
- **Zero RAM a mais:** o cliente é um POST com o `ureq` que o crate já tem.
- **Mais um serviço recebe o texto** quando o filtro está ligado. A TypeSafe
  declara não treinar com os dados; retenção zero só no plano enterprise.
- **Sem internet, ou com o filtro desligado**, o Copilot é o de antes.
- **Testes:** o `FakeClassifier` (`isper-llm`, feature `testing`) permite
  testar o filtro sem rede. A montagem do pedido tem golden da ordem das
  opções, que o `serde_json` do projeto não preserva sozinho.

## Alternativas consideradas

- **Laya (local, Apache 2.0, mesmo protocolo do Jev).** Medido no mesmo
  corpus:
  - sem ajuste: F1 0,37 e AP 0,36;
  - calibrado: empata com "disparar sempre";
  - com a cabeça ajustada (360 rótulos à mão ou 1.512 destilados por LLM):
    não melhora;
  - p95 de 1,4–1,7 s na CPU e +1,5 GB de RAM;
  - o porte para Rust funcionava (`ort` + `tokenizers` com a
    `onnxruntime.dll` que o instalador já leva, com ids e logits idênticos
    aos do Python), mas não havia o que portar.
- **Um LLM parágrafo a parágrafo.** O segundo anotador (um LLM grande) teve
  F1 0,83, mais que o Jev. Só que custaria segundos e tokens de LLM por
  parágrafo, justamente o que o filtro existe para cortar.
- **Mais frases fixas.** Barato, mas o teto é baixo: as paráfrases ("bora
  fechar assim", "deixa comigo até quinta") não cabem numa lista.

## Onde vive

- `crates/isper-llm/src/systemone.rs`: cliente do Jev, `Classifier`, pedido e
  resposta.
- `crates/isper-llm/src/copilot.rs`: `filter_questions`, `read_filter`,
  `FocusHint` e o bloco do prompt.
- `apps/isper-app/src-tauri/src/copilot_filter.rs`: parágrafos, leitura e
  estado.
- `apps/isper-app/src-tauri/src/copilot.rs`: o loop e a rodada focada.
- Configurações → Inteligência → Filtro do Copilot.
