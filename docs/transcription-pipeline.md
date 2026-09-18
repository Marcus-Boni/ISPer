# Pipeline de transcrição do ISPer

Como o áudio de uma reunião vira uma transcrição que serve de ata: o que cada
etapa faz, por que os números são esses e como medir se uma mudança melhorou
ou piorou o resultado.

> Quem só quer rodar: [Como medir](#como-medir). Quem quer entender uma
> decisão: cada seção tem a evidência que a sustenta.

---

## O desenho em uma tela

```text
                  ┌─────────────────────────┐
                  │  CAPTURA (durante)      │
                  │  mic (cpal)  loopback   │
                  └────┬───────────────┬────┘
                       │               │
              ┌────────▼────┐   ┌──────▼──────┐
              │ corte por   │   │ corte por   │   chunk.rs
              │ silêncio    │   │ silêncio    │
              └────┬────────┘   └──────┬──────┘
                   │                   │
            ┌──────▼───────────────────▼──────┐
            │ ASR ao vivo (greedy, ~20 s)     │  engine.rs + profile.rs
            └──────┬──────────────────────────┘
                   │                    ┌──────────────────────────┐
                   ▼                    │ os DOIS canais, contínuos │
              LEGENDA / UI              │ em disco, no relógio da   │  meeting.rs
                                        │ reunião (silêncio inclusive)│
                                        └────────────┬──────────────┘
                                                     │  ao ENCERRAR
                            ┌────────────────────────▼────────────────┐
                            │ VAD Silero → regiões de fala            │  vad.rs
                            │ → janelas cortadas no silêncio          │
                            ├─────────────────────────────────────────┤
                            │ ASR final: beam search 5, contexto da   │  pipeline.rs
                            │ janela anterior, timestamps por palavra │
                            ├─────────────────────────────────────────┤
                            │ diarização sobre o MESMO áudio contínuo │  isper-diarize
                            │ + limpeza de grupos fracos + guardas    │
                            ├─────────────────────────────────────────┤
                            │ falante por PALAVRA → falas             │  align.rs
                            ├─────────────────────────────────────────┤
                            │ glossário (bruto preservado ao lado)    │  context.rs
                            └────────────────────┬────────────────────┘
                                                 ▼
                                      TRANSCRIÇÃO OFICIAL
                                   (substitui a do ao vivo)
```

Dois modos, um motor:

| | **LIVE** | **FINAL** |
|---|---|---|
| quando | durante a reunião | ao encerrar, em segundo plano |
| para que | legenda, insights, "está gravando" | ata, decisões, busca, IA |
| fatiamento | buffer cortado no silêncio | VAD sobre o áudio inteiro |
| decoder | greedy, `best_of` 5 | beam search 5 |
| contexto | nenhum | fim da janela anterior |
| timestamps | por segmento | por palavra (+ DTW) |
| falante | "Eu" / "Participantes" | por palavra, via diarização |
| custo | cabe no tempo real | ~0,4× a duração da reunião |

A transcrição ao vivo **não é jogada fora**: ela é o que aparece na hora e o
que fica se o passe final falhar. O passe final só substitui quando termina
inteiro (`store::replace_segments`, numa transação).

---

## Etapa por etapa

### 1. Captura

`meeting.rs` abre dois canais em sequência (loopback primeiro, microfone
depois — abrir juntos fazia o loopback perder os primeiros segundos quando o
mic e a caixa são o mesmo dispositivo USB).

O loopback tem keepalive, drenagem completa e watchdog próprios
(`loopback.rs`); o microfone se reabre sozinho ao estagnar.

**O que mudou nesta fase**: os dois canais são gravados inteiros em disco
(`ChannelAudio`, PCM 16 bits em `%TEMP%\ISPer`), e **a posição no arquivo é o
instante da reunião** — o que não chegou vira silêncio. Antes, só o canal dos
participantes era gravado, sem os blocos silenciosos, emendados uns nos
outros. Duas horas ocupam ~230 MB por canal, apagados quando a reunião é
descartada.

Por que isso importa: o segmentador da diarização olha janelas deslizantes de
~10 s. Numa emenda, a janela pega o fim de uma fala e o começo de outra, de
minutos depois — e o modelo vê uma troca de voz que não existiu.

### 2. Corte ao vivo (`chunk.rs`)

O buffer é cortado quando chega a 20 s **e** há silêncio de verdade onde
cortar: a janela de 100 ms mais quieta dos últimos 4 s precisa estar abaixo de
15% do volume do bloco **e** abaixo de um piso absoluto (0,02).

Sem silêncio, o buffer continua enchendo até 32 s; aí o corte é forçado, no
ponto menos ruim, e isso é contado (`MeetingResult::forced_cuts`).

> **O bug que isto conserta.** Antes, o corte era "a janela de 100 ms de menor
> energia do último 1,5 s" — sem exigir que fosse silêncio. Menor energia
> existe sempre, inclusive na oclusão de um /t/ ou /p/ no meio de uma palavra.
> Numa fala corrida o corte caía dentro de "manual", o bloco seguinte começava
> em "nual", e o modelo — sem contexto nenhum — escrevia "anual".

### 3. VAD (`vad.rs`)

O passe final usa o **Silero v5.1.2** que o whisper.cpp já embarca (0,9 MB,
baixado na primeira reunião). Ele devolve as regiões de fala; `plan_windows`
as agrupa em janelas de ASR.

**Armadilha documentada**: `FullParams::enable_vad` não funciona pelo caminho
do whisper-rs. O whisper.cpp só aplica o VAD dentro de `whisper_full`, e o
whisper-rs chama `whisper_full_with_state` — o parâmetro é aceito e
silenciosamente ignorado. Por isso rodamos o VAD nós mesmos, o que também
entrega as regiões como dado (métricas e diarização usam).

Parâmetros e de onde vêm:

| parâmetro | valor | origem |
|---|---|---|
| `threshold` | 0,5 | default do whisper.cpp |
| `min_speech_ms` | 250 | default do whisper.cpp |
| `min_silence_ms` | **300** | nosso: 100 ms (o default) picota frase — a pausa entre duas palavras passa de 100 ms |
| `speech_pad_ms` | 30 | default do whisper.cpp |
| `max_window_secs` | **30** | o Whisper monta 30 s de espectrograma por chamada, tenha ela 3 s ou 30 |
| `split_gap_secs` | **2,0** | silêncio menor fica DENTRO da janela; é a mesma folga que o whisper.cpp mantém no VAD interno |
| `context_gap_secs` | **8,0** | acima disso o assunto mudou e o contexto anterior atrapalha |

O `max_window_secs` é a decisão de custo do passe final. Medido no corpus de
regressão (190 s, 3 falantes, RTX 4050, large-v3-turbo-q5):

| janelas | mediana | ASR | WER |
|---:|---:|---:|---:|
| 2 | 93,6 s | 7,9 s | 6,69% |
| 3 | 35,9 s | 7,3 s | 6,34% |
| 5 | 35,9 s | 7,9 s | 5,99% |
| 11 | 17,5 s | 8,8 s | 5,99% |
| 20 | 5,9 s | 13,5 s | 5,99% |
| 37 | 2,7 s | 20,9 s | 5,28% |

Ou seja: **dentro do regime do VAD, o tamanho da janela quase não muda a
qualidade** (5,28%–6,69% é uma diferença de 2 a 4 palavras em 284 — ruído numa
fixture só), mas muda muito o custo. O que muda a qualidade de verdade é ter
VAD em vez do fatiamento antigo (8,45% → ~5,3%).

### 4. ASR e perfis (`profile.rs`, `engine.rs`)

Todos os parâmetros de decodificação moram em `DecodeConfig`, com três presets.
Nada é deixado "no default implícito do whisper.cpp" sem estar escrito.

| | Dictation / Live | MeetingFinal |
|---|---|---|
| estratégia | Greedy | **BeamSearch 5** |
| `best_of` | 5 | 5 |
| `temperature` / `_inc` | 0,0 / 0,2 | 0,0 / 0,2 |
| `entropy_thold` | 2,4 | 2,4 |
| `logprob_thold` | -1,0 | -1,0 |
| `no_speech_thold` | 0,6 | 0,6 |
| `token_timestamps` | não | **sim** |
| contexto entre janelas | não | **sim (240 chars)** |

Dois achados de leitura do código do whisper.cpp que viraram correção:

- o ISPer passava `Greedy { best_of: 1 }`. O default é 5, e `best_of` é o
  número de candidatos amostrados quando a temperatura sobe: com 1, o
  *temperature fallback* existe mas não tem o que escolher;
- `whisper_lang_id` não conhece "pt-br". A configuração aceita esse valor (é
  o que a pessoa escreve) e ele chegava cru ao motor; `normalize_lang` reduz
  para "pt" antes da inferência.

**DTW** (`DtwModelPreset::LargeV3Turbo` e correspondentes) fica ligado no
motor do app: os timestamps por token ficam bem mais precisos, e são eles que
decidem de quem é cada palavra. Custo medido nesta máquina: o buffer de
decodificação vai de 100,04 MB para 137,04 MB — **+37 MB de VRAM**, nada no
ditado, que nem pede timestamps.

### 5. Contexto entre janelas (`context.rs`)

O `initial_prompt` do Whisper é um prefixo de texto: o modelo continua no
estilo do que acabou de ler. Duas coisas entram nele.

**O glossário**, como frase e não como despejo de termos:

```text
Reunião com Tommasi sobre o ERP. Participantes: Ana, Bruno, Carla, Paulo Rocha.
Termos: Optsolv, SharePoint, Javé, fluxo de caixa. Siglas: ERP, MRP.
```

**O fim da janela anterior**, até 240 caracteres, começando numa fronteira de
frase.

Três travas contra "um erro no começo contamina a reunião inteira":

1. o texto só atravessa se a log-probabilidade média da janela for ≥ −1,0 (o
   mesmo limiar que o whisper.cpp usa para decidir que uma decodificação foi
   ruim);
2. só atravessa entre janelas separadas por menos de `context_gap_secs` (8 s);
3. o prompt inteiro é cortado em 420 caracteres — o whisper.cpp só aproveita
   ~223 tokens, e o que passa disso empurra para fora justamente o começo,
   que é onde está o glossário.

### 6. Diarização (`isper-diarize`)

pyannote segmentation 3.0 acha os trechos e as trocas; 3D-Speaker (ERes2Net)
extrai o *embedding* de cada trecho; o sherpa-onnx agrupa.

**Como o agrupamento funciona de verdade** (lido em `fast-clustering.cc`):
agrupamento hierárquico com **ligação COMPLETA** sobre dissimilaridade de
cosseno, cortado por altura. Ligação completa é o critério mais exigente que
existe — dois grupos só se juntam se *todos* os pares entre eles estiverem
dentro do limiar. Basta um par fora para o merge nunca acontecer.

Daí vem o comportamento que produziu "Participante 255". Medido, mesma
reunião (3 falantes de verdade) em duas durações:

| áudio | limiar | grupos brutos | falantes publicados |
|---|---|---:|---:|
| 3,2 min | 0,3 (antigo) | 7 | 4 |
| 3,2 min | **0,5 (atual)** | 5 | **3** ✔ |
| 19,2 min | 0,3 (antigo) | **34** | 21 ⚠ |
| 19,2 min | 0,5 (atual) | 13 | 9 ⚠ |
| 19,2 min | **nº de falantes informado (3)** | **2** | **2** |

A contagem de grupos cresce **com a duração da reunião**, não com o número de
pessoas: quanto mais trechos, maior a chance de um par da mesma pessoa
estourar o limiar, e o grupo nunca fecha. Extrapolando 34 grupos em 19 min
para 2 h chega-se a ~200 — exatamente a ordem de grandeza do "Participante
255" relatado (o `u8` saturava em 255; era o sintoma, não a causa).

Três respostas, nesta ordem de eficácia:

1. **informar o número de participantes** (Configurações → Reuniões). Com ele,
   o sherpa corta o dendrograma em exatamente N grupos (`cutree_k`) e ignora o
   limiar. É a única coisa que se manteve estável na reunião longa;
2. **limiar 0,5** — o default do próprio sherpa-onnx. O ISPer usava 0,3,
   calibrado numa fixture de duas vozes sintéticas, que não representa
   reunião;
3. **limpeza dos grupos fracos** (`postprocess.rs`): um grupo que somou menos
   que `max(6 s, 2% da fala)` ou menos de 2 turnos não é um participante — é um
   trecho que o agrupamento não soube encaixar, e vai para o grupo forte mais
   próximo no tempo. O critério é uma *fração* da fala justamente para não
   envelhecer com a duração.

E uma guarda: acima de 12 grupos, o resultado **não é publicado em silêncio**.
Vira aviso no log, `reliable() == false`, e o app mantém "Participantes" em vez
de inventar dezenas de pessoas. `speaker` passou a ser `u32`: saturar em 255
transformava o sintoma em rótulo.

### 7. Falante por palavra (`align.rs`)

Antes, o falante era atribuído por **segmento** do Whisper: um segmento de
10 s recebia inteiro o falante de maior sobreposição. Quando duas pessoas
falam dentro do mesmo segmento, metade da fala ia para a pessoa errada.

Agora a unidade é a palavra:

```text
ASR → palavra + início + fim
          │
turnos da diarização
          │
 falante por palavra        ← maior sobreposição; sem nenhuma, "encosta"
          │                   no turno até 0,75 s de distância; além disso,
          │                   fica sem dono (honesto > errado)
 alisamento de troca curta  ← uma ou duas palavras com falante diferente,
          │                   cercadas pelo mesmo falante dos dois lados e
          │                   durando < 1 s, voltam para o vizinho
 agregação em falas         ← quebra por troca, pausa > 2 s ou 60 s de fala
```

O alisamento é o que mata a "troca de speaker no meio de uma fala contínua".

Quando o perfil não pede `token_timestamps` (ao vivo), cada segmento entra
como uma "palavra" com o texto inteiro — a granularidade antiga, no mesmo
caminho de código, sem inventar horário nenhum.

### 8. Normalização (`pipeline.rs` + `text.rs`)

O glossário corrige por semelhança o que o ASR ainda errar (`apply_dictionary`),
e **o bruto continua ao lado**: `FinalTranscript` carrega `raw_segments`,
`raw_text` e `normalized_text`. O `bench` grava os dois em disco
(`.raw.txt` e `.norm.txt`). Nada do pós-processamento é irreversível, e nada
inventa conteúdo — a correção é por distância de edição sobre termos que a
pessoa cadastrou.

---

## Como medir

O princípio é "medir antes de otimizar": nenhuma mudança entra sem comparar
com o que havia antes, sobre o mesmo áudio, com os parâmetros gravados junto.

### Gerar o corpus de regressão

O roteiro é versionado; o áudio, não (megabytes envelhecem mal no Git).

```bash
cargo run --release -p isper-cli --bin mkfixture -- fixtures/reuniao-sintetica.txt
```

Saem três arquivos: o `.wav` (190 s, 3 falantes), o `.ref.txt` (a transcrição
de referência — é o texto que mandamos sintetizar, não um chute) e o
`.turns.tsv` (os turnos de referência, para o DER).

O roteiro cobre de propósito: monólogo longo, troca rápida de falante, nomes
próprios, siglas, números por extenso, silêncio de 6 s, interrupção e frases
compridas.

> **Limite conhecido.** As vozes vêm do SAPI do Windows, separadas por
> perturbação de velocidade (1,00 · 0,92 · 1,08 — o mesmo recurso que o treino
> de reconhecimento de locutor usa para fabricar falantes). Voz sintética
> limpa **não substitui** reunião real com áudio comprimido pelo Teams. Este
> corpus pega regressão e compara configurações; não mede qualidade absoluta.
> Para números que valham para o produto, use uma reunião real com um trecho
> corrigido à mão.

### Rodar e comparar

```bash
isper-cli bench reuniao.wav --config baseline --reference ref.txt --reference-turns turns.tsv
isper-cli bench reuniao.wav --config final    --reference ref.txt --reference-turns turns.tsv
isper-cli compare bench/baseline.report.json bench/final.report.json
```

- `--config baseline` reproduz **o pipeline antigo** (blocos de 20 s cortados
  por energia, greedy `best_of` 1, sem contexto, sem palavras);
- `--config final` é o atual;
- `--config caminho.json` roda qualquer variação — cada rodada grava o
  `.config.json` que a produziu, então experimentar é copiar e editar.

Cada rodada deixa: `.report.json` (todos os parâmetros e métricas),
`.config.json`, `.raw.txt`, `.norm.txt`, `.transcript.txt` (com falantes) e
`.words.tsv` (palavra, horário, falante, probabilidade).

`isper-cli score --reference a.txt --hypothesis b.txt` calcula WER/CER entre
dois textos sem rodar nada.

### O que o relatório traz

Modelo, dispositivo, DTW · duração e fator de tempo real · tempo por etapa
(VAD, ASR, diarização, alinhamento) · regiões de fala, silêncio descartado,
janelas · segmentos, palavras, log-probabilidade média, descartados pelo filtro
de alucinação, janelas vazias/falhas/com contexto · grupos brutos, falantes,
grupos absorvidos, turnos, turnos curtos, avisos · trocas de falante, mediana
de fala, falas sem dono.

---

## Antes e depois

Corpus de regressão (190 s, 3 falantes), RTX 4050, `large-v3-turbo-q5_0`,
glossário ligado, número de participantes **não** informado:

| | baseline | final | Δ |
|---|---:|---:|---:|
| **WER** | 8,45% | **5,28%** | **−38%** |
| **CER** | 6,85% | **4,16%** | **−39%** |
| **DER** | 28,08% | **20,60%** | **−27%** |
| falantes encontrados (verdade: 3) | 3 | 3 | — |
| substituições | 12 | 8 | −4 |
| inserções | 5 | 0 | −5 |
| segmentos | 22 | 37 | |
| palavras com horário | 0 | 276 | |
| silêncio poupado do modelo | 0 s | 56,9 s | |
| processamento | 69,0 s | 80,8 s | +17% |
| fator de tempo real | 0,36 | 0,43 | +19% |
| — só o ASR | 7,1 s | 22,6 s | ×3,2 |
| — só a diarização | 61,9 s | 55,9 s | — |

Leitura honesta dos números:

- o **texto** melhora de verdade: −38% de WER, e as 5 inserções do baseline
  (texto inventado nas emendas entre blocos) somem;
- o **DER** cai 27% com a **mesma** diarização nos dois lados — o ganho é só
  da atribuição por palavra em vez de por segmento;
- a linha "falantes encontrados" empata porque o baseline aqui já usa a
  diarização **corrigida**. O ganho da diarização está na tabela da seção 6,
  que compara limiar antigo × atual;
- o custo extra está quase todo no ASR (×3,2), e mesmo assim o passe final
  inteiro roda em 0,43× a duração da reunião nesta GPU. Uma reunião de 2 h
  leva ~52 min de processamento em segundo plano, dos quais ~35 min são
  diarização.

---

## Configuração

Padrões bons, avançado escondido. Em **Configurações → Reuniões**:

- **Quantos participantes costuma ter a reunião** — o ajuste de maior efeito.
  "Descobrir sozinho" é o padrão; informar o número estabiliza a contagem em
  reunião longa (seção 6);
- **Avançado → Refazer a transcrição ao encerrar** (`final_pass`, ligado):
  desligue se o custo de CPU/GPU em segundo plano incomodar;
- **Avançado → Limiar do agrupamento** (`diarize_threshold`, 0 = 0,5): mexer
  sem comparar costuma piorar.

Fora da interface, para calibrar sem recompilar:
`ISPER_DIARIZE_THRESHOLD`, `ISPER_DIARIZE_SPEAKERS`.

Os parâmetros do pipeline não estão espalhados: `DecodeConfig` (decoder),
`VadOptions` e `WindowOptions` (fronteiras), `AlignOptions` (falas),
`ChunkOptions` (corte ao vivo), `DiarizeOptions` (agrupamento). Todos
serializáveis — é isso que o `--config caminho.json` do `bench` edita.

---

## Testes

```bash
cargo test --workspace                                   # unidade + integração
cargo test --release -p isper-core --test pipeline -- --ignored   # com modelos
```

- **unidade**: fronteiras de janela, corte no silêncio (com testes de
  propriedade), alinhamento palavra→falante, alisamento de troca curta,
  reconstrução de falas, WER/CER/DER, limpeza de grupos, prompt e glossário,
  linha do tempo do áudio gravado;
- **integração** (`tests/pipeline.rs`): o caminho VAD → janelas → alinhamento →
  falas sem modelo nenhum, e, com os modelos instalados, o passe final inteiro
  sobre uma fixture;
- **regressão**: o corpus do `bench`, comparado por `isper-cli compare`.

---

## Pontos abertos

- **Voz real.** Tudo acima foi medido em voz sintética. O corpus certo é uma
  reunião real com um trecho corrigido à mão — a infraestrutura
  (`--reference`, `--reference-turns`) já espera por ele.
- **Memória e disco do passe final crescem com a duração.** Cada canal vai a
  disco em PCM 16 bits (115 MB/h, dois canais) e é lido de volta inteiro em
  f32 (230 MB/h), e o `sherpa-rs` copia o buffer mais uma vez para diarizar.
  Para 2 h: ~0,46 GB em `%TEMP%` e ~0,9 GB de pico de RAM. Rodou sem
  problema até 19 min (o maior corpus medido); as 2 h são extrapolação, não
  medição. Se um dia apertar, o caminho é processar a diarização em blocos
  longos com sobreposição, não carregar tudo.
- **Diarização é o gargalo**: 55 s para 190 s de áudio (0,29× tempo real), mais
  que o dobro do ASR. O `sherpa-rs` 0.6 fixa `num_threads: 1` na configuração
  de diarização e não expõe o parâmetro; subir isso exige PR no crate ou
  chamar o sherpa-onnx direto.
- **Modelo de embedding**: o atual (`3dspeaker_…_zh-cn_…`) foi treinado em
  chinês. Há alternativas multilíngues no catálogo do sherpa-onnx
  (`3dspeaker_speech_campplus_sv_zh_en_16k-common_advanced`, 27 MB). Trocar sem
  medir em voz real seria adivinhar.
- **Fala sobreposta**: o pyannote 3.0 marca sobreposição, mas o sherpa-onnx
  devolve um falante por trecho. Duas pessoas falando juntas viram uma.
- **Modelos e quantização**: não comparados nesta rodada. O `bench` aceita
  `--model`, então a comparação é uma linha de comando — falta o corpus real
  para que o resultado signifique algo.
