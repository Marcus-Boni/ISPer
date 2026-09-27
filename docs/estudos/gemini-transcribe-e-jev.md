# Estudo — Gemini 3.5 Transcribe e Jev no ISPer

- **Status:** estudo. Nada foi implementado, e nenhuma decisão foi tomada.
- **Data:** 26/09/2026
- **Base:** `main` em `88d4b77` (v0.23.0)
- **Pergunta:** o ISPer fica melhor, em velocidade e precisão, com o Gemini 3.5
  Transcribe (Google) e o Jev (TypeSafe)? Onde, a que custo e com que riscos?

> **Atualização de 27/09/2026 (direção do usuário):**
>
> - **O Gemini 3.5 Transcribe fica em segundo plano.** Se voltar, é para o
>   celular (§4.4); o PC continua 100% local.
> - **O foco passa a ser um classificador estruturado para o Copilot.**
> - **Entrou um candidato local e aberto, o Laya.** A análise dele e o plano
>   do Copilot estão no [§11](#11-o-copilot-com-classificador-jev-ou-laya).
>
> **Validação de 27/09/2026:** o Laya foi medido de ponta a ponta em
> reuniões reais, e **não compensa portá-lo para o Copilot**. O porte para
> Rust funciona; o que falha é a qualidade em pt-BR, e a latência também não
> passa. Os números estão no [§12](#12-validação-do-laya-270926).
>
> **Jev medido no mesmo corpus (27/09/2026):** como filtro na frente do LLM,
> pega 92% dos momentos, dispensa 55% dos parágrafos e custa ~US$ 0,007 por
> hora de reunião, sem RAM a mais. Ele entra no Copilot. Números no §12.5.

---

## Resumo

1. **O Jev não transcreve.** É um classificador de texto: devolve escolha,
   nota ou verdadeiro/falso, com probabilidade calibrada. Responde em
   70–500 ms e cobra US$ 0,042 por milhão de tokens de entrada; a saída é
   grátis. No repositório de exemplo, quem transcreve é o Gemini, e o Jev só
   julga a emoção do texto a cada 0,5 s. Por isso o Jev **não deixa a
   transcrição mais rápida nem mais precisa**. O que ele melhora são as
   **decisões tomadas sobre o texto**: o Copilot, o resumo de reuniões longas,
   a busca e o polimento.
2. **O Gemini 3.5 Transcribe deve melhorar o ISPer em três lugares**, em ordem
   de ganho:
   - **No celular**, onde 1 h de reunião leva hoje ~86 min de CPU e sai com
     WER de 10,9%.
   - **No passe final das reuniões longas.**
   - **No ditado que mistura pt-BR com inglês técnico**, graças ao
     vocabulário próprio.

   Com a RTX 4050, o ditado já é rápido: 10,4 s de fala saem em 0,6 s. Ali o
   ganho seria de precisão, não de velocidade.
3. **O custo é baixo.** No uso estimado, fica entre US$ 7 e 13 por mês (§5).
   Legenda ao vivo pela nuvem é o único uso caro, e não compensa.
4. **O bloqueio maior não é técnico.** As ADRs 0002, 0003, 0005 e 0017, o
   README, o SECURITY.md e a declaração enviada à SignPath prometem que
   **nenhum áudio sai da máquina**. Mandar áudio à nuvem pede um ADR novo e
   opt-in explícito, por tarefa. Há ainda um detalhe do Gemini: na camada
   gratuita, **o Google usa o conteúdo para melhorar seus produtos**. Para
   reunião de trabalho, só a camada paga serve.
5. **Recomendação: medir antes de decidir.** A etapa E0 (§8) não mexe no
   produto. Monta um corpus real em pt-BR e acrescenta o Gemini ao
   `isper-cli bench`, que já calcula WER, CER e DER. Se os números
   confirmarem, a nuvem entra **como opção por tarefa**, com o Whisper local
   sempre de reserva, e não como substituta.
6. **Talvez a maior melhoria de velocidade do ditado seja local e grátis.**
   Transcrever *enquanto* a pessoa fala, como as legendas das reuniões já
   fazem, deixa só o fim da fala para depois de soltar o atalho (§4.1).

---

## 1. O que cada tecnologia é (e o que não é)

### 1.1 Gemini 3.5 Transcribe

Lançado em 26/08/2026 e em *public preview*. A documentação do Google é
contraditória: a página do modelo diz "stable", e o blog diz "preview".

| | Arquivo (`gemini-3.5-transcribe`) | Ao vivo (`gemini-3.5-transcribe-live`) |
|---|---|---|
| API | Interactions API: `POST …/v1beta/interactions` | Live API: WebSocket `BidiGenerateContent` |
| Entrada | WAV, MP3, FLAC, **OGG/Opus**, M4A, WebM, L16… (inline ou Files API) | PCM 16 bits, 16 kHz, mono, em pacotes de 100 ms |
| Limite | 1 h por pedido; **30 min** com diarização ou timestamps | **10 min por sessão**; é preciso rodar sessões |
| Falantes | até 8; **3 ou mais é experimental** (a comunidade só confia até 3) | **não tem** |
| Horário por palavra | sim, mas **a documentação diz que degrada a precisão** | não |
| Vocabulário próprio | até 1.000 termos (melhor com até ~100) | sim |
| Modo `smart` (tira hesitações, formata números) | sim | a verificar (o exemplo usa `VERBATIM`) |
| Idiomas | 85+, com detecção e troca no meio da fala | idem |
| Latência | não publicada | parcial em menos de 200 ms; final ~0,4 s após o fim da fala (fontes de terceiros, não medido) |
| Preço (camada paga) | US$ 2/M tokens de áudio e US$ 12/M de texto, **≈ US$ 0,005/min** | US$ 3,50/M e US$ 21/M, **≈ US$ 0,009/min** |

**Limitações que mudam o desenho:**

- **Vocabulário próprio não combina com diarização nem com horário por
  palavra.** A API recusa a combinação. Um funcionário do Google confirmou no
  fórum, em 01/09/2026, que a restrição é proposital.
- **O modo `smart` também não combina com diarização nem com horário.**
- **O melhor texto sai em modo simples, com vocabulário e sem horários.**
  Justamente o modo que não diz *quem* falou nem *quando*.
- **A documentação mostra a chave na URL (`?key=`).** A ADR 0003 exige a chave
  em header (`x-goog-api-key`), inclusive no handshake do WebSocket.
- **Os termos de uso diferem por camada.** Nos serviços gratuitos, o Google usa
  o conteúdo e as respostas para melhorar produtos. Nos pagos, não usa, mas
  guarda registros para detectar abuso, sem prazo declarado.
- **Os limites de requisições do preview não foram publicados.**

### 1.2 Jev (TypeSafe, "System One model")

Está em *early access* desde setembro de 2026, na versão `jev-1.13.0`.

- **O que faz:** recebe um `state`, que é texto ou JSON, e um mapa de
  perguntas tipadas. Devolve, em paralelo e numa só chamada:
  - `choice`: uma opção entre até 255, com a probabilidade de cada uma;
  - `score`: uma nota numa rubrica de 2 a 10 níveis;
  - `noul`: a probabilidade de uma afirmação ser verdadeira.

  Toda resposta vem com um nível de **confiança**.
- **API:** `POST https://api.typesafe.ai/v1/systemone`, com
  `Authorization: Bearer`. Os erros são 401, 422, 429 e 529.
- **SDKs:** só Python e JS. Em Rust seria HTTP cru com `ureq`, como os
  providers atuais.
- **Contexto:** 64k tokens por pedido, sendo 32k para o `state` mais a maior
  pergunta. **Só texto**, sem áudio.
- **Limites de uso:** 250 mil tokens/s e 1.200 pedidos/min.
- **Idioma:** o principal é o inglês. Aceita outros idiomas "com desempenho
  variável". O exemplo validou o português em **16 frases escritas à mão**.
- **O que ele não faz** (página de limitações da 1.13):
  - não gera texto;
  - não conta nem calcula;
  - não compara datas;
  - lê a pergunta ao pé da letra;
  - perde precisão com contexto irrelevante;
  - **pode ser conduzido por instrução injetada no próprio texto**, o que
    importa porque o transcript vem da boca de terceiros.
- **Dados:** a TypeSafe se compromete a não treinar modelos com os dados de
  clientes. Retenção zero só no plano enterprise.

---

## 2. O que o repositório de exemplo faz

[illumi-ai/sentimento-em-tempo-real](https://github.com/illumi-ai/sentimento-em-tempo-real)
é um detector de emoção em tempo real, com React e Node.

**Transcrição**
- O navegador captura o áudio num AudioWorklet (PCM16, 16 kHz, pacotes de
  100 ms) e manda ao Gemini Live pelo SDK.
- A conexão usa um **token efêmero de uso único**, emitido pelo servidor.
- O modo é `VERBATIM`, para manter as hesitações; o "incerteza" do Jev lê
  justamente elas.

**Avaliação pelo Jev**
- Roda a cada ≥ 0,5 s e olha os últimos **10 s de fala**: até 45 palavras,
  mais 40 de contexto.
- O navegador mede o **tom de voz** (volume, altura, ritmo, pausas) em
  relação à linha de base do falante e o descreve em palavras ("mais alto que
  o normal"). Essas descrições entram no `state`.
- São 6 notas (`score`) de 4 níveis. As perguntas estão em inglês e o texto
  em pt-BR.

**Robustez**
- Roda a sessão Live antes dos 10 min, com 3 s de carência para a antiga
  fechar a última frase.
- Tenta de novo até 5 vezes, com espera de 1 a 10 s.
- Para na hora com erro fatal: cobrança, cota, chave inválida ou sem acesso.
- Fixa a versão do modelo (`jev-1.13.0`).

**Custo medido pelo autor:** ~US$ 0,014 por minuto de fala. São ~0,009 da
transcrição e ~0,005 do Jev (~120 avaliações/min de ~1.100 tokens).

**Limitações que o próprio projeto admite:** não separa falantes, erra ironia
e frases curtas e não deve ser usado para decidir sobre pessoas sem
consentimento.

**O que aproveitar:**
- a rodada de sessão com carência;
- a separação entre erro fatal e erro transitório;
- a ideia de medir a prosódia no aparelho e descrevê-la em palavras para o
  classificador;
- a versão fixada;
- o `VERBATIM` quando a hesitação carrega sinal.

**O que não aproveitar:** o desenho navegador + Node. No ISPer o áudio já
nasce em Rust (cpal/WASAPI), e o cliente também seria Rust. O token efêmero
serve para esconder a chave de um navegador. Num app desktop em que a pessoa
usa a própria chave, ele não é necessário.

---

## 3. Onde o ISPer está hoje

**Números do próprio repositório**

| Fluxo | Hoje | Fonte |
|---|---|---|
| Ditado (GPU) | 10,4 s de áudio em 0,6 s (16,1× o tempo real), turbo q5_0, greedy | `ROADMAP.md:144`, ADR 0002 |
| Ditado (instalador CPU) | sem número para o turbo; o `small` fazia 3,3× o tempo real | Fase 1 |
| Polimento do ditado | **síncrono, antes de colar, com timeout HTTP de 120 s** | `dictation.rs:140`, `providers.rs:71` |
| Legenda ao vivo | provisória a cada 1,5 s; a fala aparece em ~2–3 s | ADR 0006, `meeting.rs:146` |
| Passe final | WER 5,28%, CER 4,16%, DER 20,6% — **em voz sintética** | `docs/transcription-pipeline.md:358-372` |
| Reunião de 2 h | ~52 min em segundo plano, ~35 deles de diarização (antes da 9.1) | `transcription-pipeline.md:385` |
| Diarização | ~2,4× mais rápida desde a 9.1 (60 s → 25 s em 190 s de áudio) | ADR 0015 |
| Celular (Xiaomi, SD 855) | `small` q5 leva 1,43× a duração; WER 10,9%, DER 22,1% | ROADMAP 9.1 |
| Resumo | acima de 60 mil caracteres, o transcript **é cortado no meio** | `summary.rs:9` |

**O que falta: nenhum WER medido em reunião real em pt-BR.** Todo ganho
registrado até aqui foi medido com voz sintética da SAPI. O próprio ROADMAP
lista como próximo passo "produzir um trecho de referência corrigido à mão".

**Pontos de encaixe no código**

- **Não existe trait de motor de transcrição.** `WhisperEngine`
  (`engine.rs:36`) é uma struct concreta, usada direto em `meeting.rs`,
  `pipeline.rs`, `final_pass.rs`, `state.rs`, `settings.rs` e na CLI.
- **Tipos do whisper-rs escapam do motor:** `profile.rs:182` (`FullParams`) e
  `vad.rs:24`.
- **O pipeline depende de três campos** que um motor de nuvem não entrega
  iguais:
  - `no_speech_prob`, que alimenta o filtro de alucinação (`text.rs:155`);
  - `avg_logprob`, que decide se o texto serve de contexto (`pipeline.rs:87`);
  - `words` com horário, sem as quais não há falante por palavra
    (`align.rs:111`).
- **Já há abstrações que servem de modelo:** `Diarizer`
  (`pipeline.rs:139`), `AudioFeed`, `LlmProvider` e `Embedder`, além do
  `FakeProvider` para testes sem rede.
- **O `isper-llm` já tem provider Gemini**, só de texto (padrão
  `gemini-3.5-flash-lite`). A chave fica no Credential Manager e vai em
  header. A mesma chave serviria para a transcrição.
- **Há um único provider global para todas as tarefas** (`settings.rs:12`).
  Não existe retry nem backoff.
- **O Copilot já é um classificador feito com LLM.** `detect_trigger`
  (`copilot.rs:329`) procura frases fixas (`DECISION_CUES`, `ACTION_CUES`,
  `RISK_CUES`). O pulso de 45 s manda ao LLM uma janela de 20 min e pede JSON
  por prompt (`copilot.rs:414`), sem `responseSchema`.

---

## 4. Onde cada tecnologia ganharia

### 4.1 Ditado

A latência que conta é a do momento em que a pessoa solta o atalho até o
texto colado.

| Duração da fala | Local GPU hoje (~0,06× a duração) | Gemini Live (final ~0,4 s + rede) | Local transcrevendo durante a fala (proposta) |
|---|---|---|---|
| 5 s | ~0,3 s | ~0,4–0,8 s | ~0,1–0,3 s |
| 20 s | ~1,2 s | ~0,4–0,8 s | ~0,2–0,4 s |
| 60 s | ~3,7 s | ~0,4–0,8 s | ~0,2–0,4 s |

A primeira coluna vem do 16,1× medido. As outras duas são **estimativas**, e
a E0 mede todas. Em qualquer caso, o polimento por LLM soma o tempo dele por
cima.

**Gemini só pelo arquivo:** não compensa no ditado. Envio mais processamento
tende a perder para a GPU local.

**Gemini Live**

O áudio vai sendo enviado enquanto a pessoa fala. Ao soltar o atalho, falta
só fechar a última frase. Ganhos:

- **latência quase fixa**, qualquer que seja a duração: ajuda em ditado longo
  e, muito, no instalador CPU;
- **vocabulário próprio**, montado a partir do dicionário pessoal (até
  ~100 termos). É mais forte que o `initial_prompt` do Whisper;
- **mistura de idiomas no meio da frase.** O uso principal é ditar para o
  Claude Code ("faz o commit na branch main e roda o clippy"), e a troca de
  idioma é um ponto fraco conhecido do Whisper;
- **o modo `smart`, se funcionar no Live**, pode dispensar a chamada de
  polimento em parte dos casos.

Detalhes que simplificam:

- O ditado já tem teto de 120 s (`recorder.rs:28`). Uma sessão por ditado
  cabe folgada nos 10 min, e não é preciso rodar sessões.
- A conexão (TLS + WebSocket) abre quando o atalho é **apertado**. O áudio
  fica no buffer até ela subir.

**A alternativa local vem antes.** O mesmo `chunk_loop` e o
`transcribe_partial` das reuniões podem transcrever o ditado em blocos
enquanto a pessoa fala. Ao soltar, sobra só o último bloco. Isso ataca a
velocidade sem custo, sem rede e sem mexer em ADR. A precisão, porém, continua
a do Whisper.

**Padrão de segurança (hedge):** rodar local e nuvem em paralelo. Cola a nuvem
se ela chegar dentro de um orçamento (por exemplo, até 300 ms depois do
local). Senão, cola o local. Sem rede, é só o local. Assim a nuvem nunca deixa
o ditado mais lento do que é hoje.

**Custo:** 15 min de ditado por dia, em 22 dias, dão ~US$ 3/mês no Live.

### 4.2 Legendas ao vivo das reuniões

Seriam duas sessões Live, uma para "Eu" e outra para "Participantes", rodadas
a cada ~9 min. Custariam US$ 0,018/min, ou **~US$ 1,08 por hora**. A legenda
provisória cairia de 1,5 s para menos de 200 ms, mas continuaria sem falantes.

**Veredito: não vale no começo.** O local já entrega a fala em 2–3 s, e o
passe final refaz tudo depois. Fica como opção futura, se alguém pedir.

### 4.3 Passe final (e importação de áudio no PC)

Aqui cabem três desenhos:

| | A. Tudo na nuvem | B. Híbrido: local decide *quem* e *quando*, nuvem decide *o quê* (recomendado) | C. Seletivo: só o que o Whisper errou |
|---|---|---|---|
| Como | Gemini com diarização e horários no canal Participantes | VAD, diarização e falante por palavra continuam locais; cada fala já separada (≤ 60 s, `GROUP_MAX_SECS`) vai ao Gemini em modo simples, **com vocabulário** | Whisper como hoje; reenvia só os trechos com `avg_logprob` baixo, `no_speech_prob` alto ou termo do glossário ausente (opcionalmente, um `noul` do Jev: "este trecho parece mal transcrito?") |
| Precisão | limitada: horário degrada o texto; sem vocabulário; ≤ 3 falantes confiáveis | **a maior**: modo em que o Gemini é melhor | quase a de B nos trechos difíceis |
| Velocidade | a maior: some a diarização local | some a parte do ASR (~17 min de 2 h); a diarização local continua | igual a hoje, mais alguns segundos |
| Complicação | **alta**: pedaços de 30 min com rótulos `spk_1` que não batem entre si; teria de religar falantes pelos embeddings locais | média: as falas já existem (`build_utterances`); só troca o texto | média |
| Custo (20 h/mês) | ~US$ 12 (os dois canais inteiros) | **~US$ 6** (só a fala depois do VAD; o silêncio não é pago) | **~US$ 1** |

**B é o ponto doce.** Usa o Gemini no modo em que ele é mais forte, mantém o
que o ISPer já faz bem, que é separar canais, detectar fala e identificar
falantes, e dribla a incompatibilidade entre vocabulário e diarização.

**O canal "Eu" é o ganho mais fácil.** Ele não precisa de diarização: vai
direto, com vocabulário.

**Velocidade:** numa reunião de 2 h, a diarização já é o gargalo (0,29× o
tempo real antes da 9.1; ~2,4× mais rápida depois). Com ela, estimo o total
em ~30 min, sem medição. O Gemini tira os ~17 min do ASR, sem mexer na
diarização.

### 4.4 Celular (e o ISPer no Bolso)

**Onde o ganho percebido é maior.** Hoje, 1 h de reunião leva ~86 min de CPU,
só na tomada, e sai com WER de 10,9%. É o caso em que o Plaud, que o ISPer
quer substituir, faz tudo na nuvem.

- **O Gemini aceita Ogg/Opus direto.** A gravação da 9.2 (~15 MB/h) sobe sem
  conversão, em pedidos de até 30 min com falantes ou em pedaços cortados
  pelo VAD.
- **A ata sai em minutos**, por ~US$ 0,30 a hora.
- **Encaixa nos modos da ADR 0018.** Hoje são "na tomada", "assim que
  terminar" e "só no PC"; entraria **"na nuvem, com a sua chave Gemini"**,
  com a chave no Android Keystore.
- **A regra "a ata do PC substitui a do celular" continua valendo.**

### 4.5 Jev: onde encaixa

Em ordem de valor para o ISPer:

1. **Filtro calibrado do Copilot.**
   - *Hoje:* busca de frases fixas mais um pulso de 45 s que chama o LLM.
   - *Com Jev:* cada parágrafo fechado ganha uma `choice`
     (decisão / ação / risco / pergunta / nada) e um `noul` ("há responsável e
     prazo?"). O LLM só é chamado para **redigir** o card quando p(nada) cai
     abaixo de um limiar e a confiança está alta.
   - *Ganho:* pega paráfrases que a lista não pega ("bora fechar assim",
     "deixa comigo até quinta"), faz menos chamadas de LLM e troca a lista de
     palavras por um limiar com significado. É o padrão *confidence-gated
     routing* da própria documentação do Jev.
2. **Resumo de reuniões longas sem cortar o meio.** Hoje, acima de 60 mil
   caracteres, o miolo é descartado às cegas (`summary.rs:9`). Com o Jev, cada
   parágrafo recebe uma nota de relevância, e o resumo recebe o início, o fim
   e **os parágrafos do meio que importam**. É um *map-reduce* barato.
3. **Reordenar os resultados da busca semântica.** Os ~20 melhores trechos dos
   embeddings recebem uma nota "quanto isto responde à busca?". O *cookbook*
   de *re-ranking* do Jev mostra esse uso.
4. **Filtro do polimento do ditado.** Um `noul` ("o texto já está limpo e
   pontuado?") pula a chamada de LLM na maioria dos ditados curtos. Hoje essa
   chamada é síncrona e pode segurar a colagem.
5. **Emoção e tom, como no exemplo.** Por último:
   - o valor para o usuário é baixo ou médio;
   - o risco é alto: inferir emoção de colegas e clientes sem consentimento é
     terreno sensível (LGPD);
   - se entrar, só para a voz de "Eu", ou com aviso e consentimento.

**Onde o Jev não entra:**
- transcrição;
- filtro de alucinação: as heurísticas funcionam, e o Jev é influenciável pelo
  próprio texto;
- comandos de voz: somaria 70–500 ms a cada ditado para ganhar pouco.

**Por que não usar o LLM que já está configurado?** Um Gemini Flash-Lite com
`responseSchema` faz a mesma classificação. O Jev tem três vantagens sobre
ele: latência (sub-segundo contra 1–3 s), preço (desprezível) e
**probabilidade calibrada**, que deixa o limiar ser uma regra do código e não
um palpite. As desvantagens são duas: português pouco validado e fornecedor em
*early access*. Por isso a ideia é um trait `Classifier` com duas
implementações, Jev e LLM com schema. Sem Jev, o ISPer usa a segunda.

---

## 5. Custos (camada paga)

Perfil estimado: 15 min de ditado por dia, 20 h de reunião por mês e 10 h de
gravação no celular por mês.

| Uso | Conta | US$/mês |
|---|---|---|
| Ditado pelo Live | 330 min × 0,009 | ~3,00 |
| Passe final híbrido (B) | ~1.200 min de fala × 0,005 | ~6,00 |
| … ou seletivo (C) | ~15% disso | ~1,00 |
| Celular | 600 min × 0,005 | ~3,00 |
| Jev: Copilot, resumo e busca | ~5 M tokens × 0,042 | < 0,50 |
| *Legenda ao vivo na nuvem (não recomendada)* | 2 canais × 1.200 min × 0,009 | *~21,60* |
| **Total com B** | | **~12,50** |
| **Total com C** | | **~7,50** |

Os preços são os do preview, de 26/09/2026, e podem mudar. A E0 mede o custo
real pelo `usage` de cada resposta.

---

## 6. Arquitetura proposta (para quando for implementar)

**1. Trait de motor no `isper-core`**
- Extrair o que o pipeline usa do `WhisperEngine`. Os tipos do whisper-rs
  (`FullParams`, VAD) ficam só na implementação local.
- Tornar opcionais os campos que a nuvem não entrega, e o pipeline degrada com
  elegância: sem `no_speech_prob`, pula esse filtro; sem `avg_logprob`, todo
  texto serve de contexto; sem `words`, o falante vem da fala inteira
  (desenho B).

```rust
pub trait Transcriber: Send + Sync {
    fn name(&self) -> &str;
    /// O que este motor sabe devolver (palavras com horário? no_speech_prob?).
    fn capabilities(&self) -> Capabilities;
    fn transcribe(&self, samples: &[f32], req: &TranscribeRequest) -> Result<Transcript>;
}

/// Ao vivo: empurra áudio, recebe eventos por um canal (crossbeam, como o core).
pub trait StreamingTranscriber: Send + Sync {
    fn start(&self, opts: &StreamOptions) -> Result<Box<dyn StreamSession>>;
}
pub trait StreamSession: Send {
    fn push(&mut self, pcm16: &[i16]) -> Result<()>;
    fn events(&self) -> crossbeam_channel::Receiver<StreamEvent>; // Interim | Final | Error
    fn finish(self: Box<Self>) -> Result<String>;
}
```

**2. Crate novo `isper-stt`** (transcrição de nuvem), separado do
`isper-llm`, que é sobre linguagem.
- Reaproveita a chave Gemini do Credential Manager.
- Arquivo: `ureq` 3 na Interactions API; áudio inline se for curto, Files API
  se passar do limite, que é preciso verificar.
- Ao vivo: `tungstenite` síncrono com rustls, numa thread. É o mesmo estilo
  de threads e crossbeam do core, sem puxar mais tokio. A chave vai em header
  no handshake.

**3. Robustez que hoje falta no `isper-llm`**
- retry com backoff para 429, 5xx, 529 e tempo esgotado;
- erro fatal (401, 403, cota, cobrança) para na hora e avisa, como no
  exemplo;
- **timeout por tarefa**: ~2 s no ditado, depois cai para o local. Nunca
  120 s.

**4. Jev** em `isper-llm/src/jev.rs` (ou num crate `isper-classify`), com as
perguntas como tipos Rust. É onde o "type-safe" do Jev casa com o Rust.

```rust
pub enum Question {
    Noul   { instructions: String, if_true: String, if_false: String },
    Choice { instructions: String, options: Vec<(String, Option<String>)> }, // ≤ 255
    Score  { instructions: String, levels: Vec<String> },                    // 2..=10
}
pub enum Answer {
    Noul   { p: f32 },
    Choice { chosen: String, probs: BTreeMap<String, f32>, confidence: f32 },
    Score  { score: f32, probs: Vec<f32>, confidence: f32 },
}
```

- Versão fixada em `jev-1.13.0`.
- Perguntas em inglês e `state` em pt-BR, como no exemplo, validadas no corpus
  rotulado da E0.
- O transcript vai **como dado** num campo do `state`, nunca concatenado às
  instruções, porque o texto de terceiros pode tentar conduzir o
  classificador.

**5. Configuração** (ADR 0011: é configuração, não feature flag, e entra
desligada, em Avançado)

```toml
asr_dictation = "local"          # "local" | "gemini-live"
asr_final     = "local"          # "local" | "gemini-hybrid" | "gemini-selective"
cloud_audio_consent = false      # pedido explícito; reunião tem interruptor próprio
cloud_budget_usd_month = 10.0    # passou disso, volta para o local e avisa
classifier = "off"               # "off" | "jev" | "llm"
```

- **Provider por tarefa**, que hoje é um só global: ditado, passe final,
  Copilot e resumo podem usar fornecedores diferentes.
- **Medidor de custo:** segundos de áudio e tokens de cada chamada numa tabela
  do SQLite, com "US$ no mês" na tela e corte no orçamento.
- **Transparência:** um ícone de nuvem no indicador enquanto o áudio sai, e um
  aviso na primeira vez. O app não tem como descobrir se a chave é da camada
  paga, então o texto precisa dizer isso claramente.
- **Testes:** uma implementação falsa de `Transcriber`, como o `FakeProvider`;
  respostas reais gravadas em fixtures JSON para o CI rodar sem rede; testes
  de rede com `#[ignore]`, como no `isper-models`.

---

## 7. Privacidade, LGPD e as promessas do projeto

**O que muda se o áudio puder sair**
- **ADR 0002** rejeitou explicitamente "API de transcrição na nuvem".
- **ADR 0003:** "só texto sai".
- **ADR 0005** rejeitou diarização na nuvem.
- **ADR 0017:** o áudio não passa por terceiros.
- **ROADMAP:** "nenhum áudio sai da máquina, nunca".
- README, `SECURITY.md` e o doc do crate `isper-llm`.
- **A declaração da SignPath**, que diz que o app não transfere informação a
  menos que o usuário peça. Opt-in explícito é compatível com ela, mas o
  README tem de dizer isso com todas as letras.

**Caminho**
- Uma **ADR 0019 (proposta):** "o áudio pode ir à nuvem só a pedido explícito
  do usuário, por tarefa, com a chave dele e na camada paga". O caminho local
  continua o padrão e completo. As ADRs 0002, 0003, 0005 e 0017 ganham nota de
  que foram revistas.
- **O Jev não precisa de ADR nova.** Só manda texto, o mesmo caso que a
  ADR 0003 já aceita.

**Cuidados com terceiros**
- **Reuniões:** o áudio de colegas e clientes iria ao Google. Isso depende da
  política da empresa e do consentimento das pessoas. Recomendo deixar
  **desligado por padrão para reuniões**, com interruptor em cada reunião.
- **Camada gratuita do Gemini:** o conteúdo é usado pelo Google, então nunca
  para reunião. O app deve avisar isso na tela da chave.
- **Emoção de terceiros:** fora, a não ser com consentimento. O próprio
  exemplo desaconselha usar para decisões sobre pessoas.

---

## 8. Plano de estudo prático

### E0 — Medir antes de decidir (1 a 2 dias, sem código de produto)

**Corpus**
- ~30 ditados reais, com termos do dicionário e mistura de pt-BR e inglês.
- 3 trechos de 10 min de reuniões reais, com a referência corrigida à mão.
  Isso fecha também a pendência do ROADMAP.
- ~100 parágrafos de reunião rotulados como decisão, ação, risco, pergunta ou
  nada.

**Ferramenta:** o `isper-cli bench`, o `compare` e o `score` já existem, com
WER, CER e DER em `metrics.rs`. O protótipo acrescenta um motor `gemini`
numa branch de estudo, ou um script que grava o texto no formato que o
`score` lê.

**Medir**
- WER e CER;
- acerto dos termos do dicionário;
- erros de troca de idioma;
- latência p50 e p95: no Live, do fim da fala ao texto final; no arquivo, do
  envio à resposta;
- custo real, pelo `usage`;
- DER no canal Participantes (desenho A);
- para o Jev: precisão e recall por classe, comparando com o
  `detect_trigger` atual e com o LLM atual, mais a latência p95 medida a
  partir do Brasil.

**Critérios de decisão** (sugestão; ajuste antes de medir, para não
escolher o critério depois de ver o número)

| Onde | Adota se |
|---|---|
| Ditado pelo Live | WER pelo menos 25% menor (relativo) **ou** 20 pontos a mais no acerto de termos, **e** p95 depois de soltar ≤ p95 local + 300 ms |
| Passe final B ou C | WER pelo menos 20% menor em reunião real, e DER não pior |
| Celular | ata em ≤ 10% da duração, com WER ≤ o do `small` |
| Jev no Copilot | F1 ≥ 0,8 em decisão e ação, pelo menos 15 pontos acima do `detect_trigger`, e p95 ≤ 500 ms |

### E1 — Trait de motor e Gemini em lote (importar áudio e celular)

Menor risco, porque não é tempo real, e maior ganho percebido. Nesta etapa
entram a ADR 0019 e o medidor de custo.

### E2 — Ditado pelo Live

Com hedge, o local de reserva e o vocabulário vindo do dicionário. Antes
dele, ou junto, a versão local "transcrevendo durante a fala".

### E3 — Passe final híbrido

B ou C, conforme a E0.

### E4 — Jev

O filtro do Copilot, o resumo de reuniões longas e a reordenação da busca.
Emoção só com pedido explícito e consentimento.

---

## 9. Riscos

- **Preview:** a Interactions API é `v1beta`, os limites de requisições não
  foram publicados e a documentação se contradiz (8 ou 3 falantes, "stable"
  ou "preview"). *Mitigação:* o trait, a reserva local e as fixtures gravadas.
- **Jev em early access:** empresa nova e português pouco validado.
  *Mitigação:* o trait `Classifier` com implementação por LLM e a versão
  fixada.
- **Rede:** ditado sem internet ou latência do Brasil até os servidores.
  *Mitigação:* hedge com o local e timeout curto.
- **Custo sem controle:** *mitigação:* orçamento mensal com corte automático.
- **Chave vazando:** *mitigação:* Credential Manager, header, e o gitleaks já
  roda no CI.
- **Dois motores para sempre:** a matriz de testes cresce. *Mitigação:* testar
  por configuração (ADR 0011), com fixtures em vez de rede.

---

## 10. Veredito

- **Gemini 3.5 Transcribe:** vale um estudo sério **como opção por tarefa**.
  A ordem sugerida é celular e importação, depois o ditado com vocabulário,
  depois o passe final híbrido. Como substituto do Whisper, não: o local
  continua o padrão, funciona sem rede e é grátis.
- **Jev:** barato a ponto de o custo não importar. Tem bom encaixe no
  Copilot e no resumo de reuniões longas. O que decide é a qualidade em pt-BR,
  e só a medição responde.
- **Antes de gastar um centavo:** a transcrição local durante a fala pode
  entregar boa parte do ganho de velocidade no ditado.

---

## 11. O Copilot com classificador: Jev ou Laya

### 11.1 O que muda no Copilot

**Hoje** (`isper-llm/src/copilot.rs`)
- `detect_trigger` (`:329`) procura frases fixas e sem acento
  (`DECISION_CUES`, `ACTION_CUES`, `RISK_CUES`).
- Um acerto antecipa a rodada. Fora isso, o pulso de 45 s manda ao LLM uma
  janela de até 20 min e pede os cards em JSON por prompt (`:414`).
- Para não repetir cards, o prompt leva a lista dos que já existem.

**Consequências**
- **Card atrasado:** chega o pulso ou a frase certa, mais os segundos do LLM.
- **Paráfrase escapa:** "bora fechar assim" ou "deixa comigo até quinta"
  passam batido.
- **Custo repetido:** cada rodada reenvia minutos de transcript.

**Com classificador**

```text
parágrafo fechado (group_speech)
      │
      ▼
classificador local ou Jev (~0,1–0,5 s)
  kind    : choice { decision, action, risk, question, none }
  owner   : noul   "tem responsável e prazo explícitos?"
  urgency : score  { low, medium, high }
      │
      ├─ p(none) alta ou confiança baixa → nada (o código decide, com limiar)
      │
      └─ senão → card PROVISÓRIO na hora (tipo + trecho)
                 └→ o LLM só redige título e descrição DESTE parágrafo
                    (prompt pequeno, em segundo plano)
```

- **O card aparece em menos de 1 s** depois de a frase terminar. Hoje leva
  dezenas de segundos.
- **O LLM é chamado bem menos**, e com um trecho curto. Isso significa menos
  custo e menos texto saindo da máquina.
- **O limiar vira regra do código**, medida no corpus, no lugar da lista de
  frases. É o padrão *confidence-gated routing*.
- **O pulso continua como rede de segurança**, com intervalo maior (por
  exemplo, 3 min), para o que se espalha por vários parágrafos.
- **A troca não pesa na interface nem no banco:** os tipos de card já são
  exatamente esses quatro (`copilot.rs:32`).

**O desenho no código**

```rust
/// isper-classify: a mesma pergunta tipada, qualquer motor.
pub trait Classifier: Send + Sync {
    fn name(&self) -> &str;
    fn classify(&self, state: &str, questions: &[(&str, Question)]) -> Result<Vec<(String, Answer)>>;
}
// LayaLocal (ort + tokenizers) · JevCloud (ureq, POST /v1/systemone)
// LlmClassifier (reserva: o LlmProvider atual pedindo JSON) · FakeClassifier (testes)
```

Os tipos `Question` e `Answer` são os do §6. O Laya fala o mesmo protocolo do
Jev (`POST /v1/systemone`): mesmas perguntas, mesmas respostas. Trocar um pelo
outro vira configuração: `classifier = "off" | "local" | "jev"`.

### 11.2 O Laya

[NandhaKishorM/laya](https://github.com/NandhaKishorM/laya) ·
[convaiinnovations/laya](https://huggingface.co/convaiinnovations/laya)

É um codificador bidirecional mais uma cabeça de decisão pequena. Faz uma
passada só, sem gerar texto, e responde `choice`, `score` e `noul` com
probabilidade e confiança, no mesmo formato do Jev.

| Checkpoint | Codificador | Parâmetros | Contexto | Para quê |
|---|---|---|---|---|
| `laya` | ModernBERT-large | 421M | 512 | só inglês |
| **`laya-multilingual`** | **mmBERT-base** | **322M** | **1.024 (até 8.192)** | **100+ idiomas: o único que serve ao ISPer** |
| `laya-typed-decisions` | ModernBERT-large | 421M | 1.024 | ajustado para 4 fluxos de negócio em inglês |

**É gratuito?** Sim.
- **Licença Apache 2.0** para o código e os pesos. O mmBERT-base é MIT, e o
  ModernBERT é Apache 2.0.
- **Roda na máquina.** Não tem API, chave nem custo por uso, e **nenhum texto
  sai**: melhor que o Jev e que o Copilot de hoje. Encaixa nas ADRs 0002 e
  0003 sem ADR nova.
- **Ao redistribuir os pesos** (por exemplo, um ONNX exportado por nós),
  seguem junto a licença e o NOTICE.

**É tranquilo de colocar?** Não totalmente. Há três frentes.

**1. Não existe runtime fora do Python e do TypeScript**
- O pacote é Python (torch 2.14+, transformers 5).
- O `laya-ts` roda em ONNX: `encoder.onnx` + `head.onnx`, com `tokenizer.json`
  e `rl_agent_config.json`, e confere o resultado com o torch dentro de 1e-4.
- **O `laya-ts` é o molde de um porte para Rust.** A arquitetura é pequena
  (`rl_common.py`):
  - **Entrada:**
    `[CLS] <tipo> question: <instruções> [SEP] [MASK] opção0 [MASK] opção1 … [SEP] <state> [SEP]`.
    Cada opção leva até 48 tokens. A parte da pergunta tem orçamento de 256
    tokens no multilíngue, e o `state` fica com o resto.
  - **Cabeça:** camadas `TransformerEncoderLayer` (norm-first) mais um MLP
    (LayerNorm → Linear → GELU → Linear) em cada `[MASK]`. O resultado são
    logits por opção.
  - **Saída:** softmax com **temperatura por tipo de pergunta**. A confiança é
    `1 − entropia/log k`, e o `score` é o nível esperado.
- **Em Rust:**
  - crate `tokenizers` (Apache 2.0) para o `tokenizer.json`;
  - crate `ort` para os dois ONNX, só na CPU (sem disputar os 6 GB de VRAM
    com o Whisper);
  - o resto é montar a sequência e fazer contas.
- **Exportação:** feita uma vez, fora do app, com o
  `laya-ts/scripts/export_onnx.py`. O ISPer baixa o resultado pelo
  `isper-models`, com revisão fixada e SHA-256, como os outros modelos.
- **A DLL do ONNX Runtime já vai no instalador:** a `onnxruntime.dll` que o
  sherpa-onnx 1.13.8 traz é a 1.28.2 no `target/release`.
  - O `ort` com `load-dynamic` pode reaproveitá-la. É preciso confirmar a
    compatibilidade de versão, porque duas DLLs de mesmo nome no mesmo
    processo dão problema no Windows.
  - Em `src-tauri/resources/sherpa/` sobrou uma **1.17** antiga, que precisa
    ser trocada antes.
- **Testes:** um golden de paridade. Entradas e logits gerados pelo Python
  viram fixtures, e o Rust tem de bater com margem de 1e-3. É o padrão de
  `tests/golden`.

**2. A qualidade em pt-BR para o Copilot é desconhecida e, sem ajuste,
provavelmente fraca**
- **Tarefas de negócio sem ajuste:** no benchmark *typed-decisions* (faturas,
  incidentes, atendimento), o multilíngue acerta **0,352**. O acaso acerta
  0,318, e chutar sempre a classe mais comum acerta 0,461. Toda a capacidade
  ali "vem do fine-tuning", nas palavras do próprio README. O 0,766 que supera
  o Jev (0,727) é do checkpoint **inglês ajustado**.
- **Outras tarefas sem ajuste:** 0,651 de acerto na média de quatro famílias.
  A melhor é seguir instruções (0,863); a pior é sentimento (0,362).
- **Idiomas:** não há número por idioma. No MASSIVE (intenção), o multilíngue
  faz 0,451 fora do inglês.
- **Calibração:** o multilíngue **vem sem temperaturas ajustadas**. As
  probabilidades saem confiantes demais (ECE 0,466 antes do ajuste, 0,081
  depois, nos checkpoints ingleses). A maior vantagem do Jev, a probabilidade
  calibrada, não vem pronta: o ajuste é nosso, no corpus rotulado.
- **Limitações declaradas:** viés de posição no `score` do multilíngue, que
  quase nunca escolhe o primeiro nível (vale inverter a ordem ou usar
  `choice`), e rótulos que às vezes pesam mais que o texto no `noul`.

**3. Maturidade**
- **O repositório nasceu em 18/09/2026, há nove dias.** Tem 25,9 mil estrelas
  e 202 issues abertas. O contador de downloads do Hugging Face marca 0.
- **O "pronto para produção" do README não se sustenta em nove dias.**
- **Os dados de treino não são divulgados.** Pelo tanto que o projeto se mede
  contra o Jev e copia o protocolo dele, não dá para saber se houve destilação
  das respostas do Jev. O risco para o ISPer é baixo, mas é um ponto a
  observar.
- **Mitigação:** não depender do pacote deles em tempo de execução. Exportar o
  ONNX uma vez, fixar a revisão e o SHA-256 e guardar a cópia.

### 11.3 Custo de máquina (estimativas, a medir)

**CPU**
- A conta favorece o Laya: dos 322M parâmetros, ~197M são a tabela de
  embeddings (vocabulário de 256k), uma consulta barata. O cálculo pesado é
  de um codificador de ~125M.
- **O único número de CPU publicado:** 10 mil tickets em 825 s num Ryzen 9
  5950X com 16 threads em fp32, ou ~80 ms por entrada.
- **No Ryzen 7 7735HS, estimo 100 a 150 ms por pergunta.** Como cada pergunta
  é uma sequência, três perguntas por parágrafo dão ~0,3–0,5 s. O ritmo é de
  um parágrafo a cada 20–60 s, em segundo plano.

**RAM**
- fp32 ≈ 1,3 GB, o que é pesado para um app que hoje usa ~300 MB.
- **int8 dinâmico ≈ 330 MB**, com a perda de acerto a medir.
- Carregar só durante a reunião e soltar depois.

**GPU:** não usar. O Whisper precisa da VRAM durante a reunião, que é o mesmo
motivo da ADR 0003.

**Instalador CPU (sem NVIDIA):** o Whisper ao vivo já disputa a CPU. Lá, o
classificador roda com threads limitadas, ou só no pulso.

### 11.4 Jev ou Laya

| | Jev (nuvem) | Laya (local) |
|---|---|---|
| Custo | centavos por mês | zero (download de ~0,35–0,65 GB) |
| Texto sai da máquina | sim (ADR 0003 cobre) | **não** |
| Sem internet | não funciona | funciona |
| Latência por parágrafo | 70–500 ms mais a rede | ~0,3–0,5 s na CPU (estimativa) |
| pt-BR | "funciona", pouco validado | sem números; sem ajuste, provavelmente fraco |
| Calibração | pronta | **ajuste nosso** (temperaturas) |
| Integração em Rust | HTTP simples: horas | ONNX + montar a entrada + `ort`: alguns dias |
| Maturidade | *early access*, empresa | 9 dias, pesos abertos que podemos fixar |
| RAM no app | nenhuma | ~330 MB em int8 durante a reunião |

**Recomendação de 26/09, antes da medição:** o destino é o **Laya local**,
que combina com o ISPer (grátis, offline, nada sai). Mas ele só entra depois
de provar a qualidade. Até lá, o Jev vale como **medida de referência
rápida**. *A validação do §12 reprovou o Laya, e a recomendação vigente está
no §12.4.*

### 11.5 Plano

**C0 — O corpus do Copilot** (vale para os dois; sem código de produto)
- ~300 parágrafos de reuniões reais da Biblioteca, rotulados como decisão,
  ação, risco, pergunta ou nada, com "tem responsável e prazo?" e urgência.
- O LLM atual faz o primeiro rascunho, e você revisa. São ~25 min para 300
  parágrafos.
- Separar 30% para teste. A mesma divisão ajusta as temperaturas.

**C1 — A comparação**, num protótipo Python no scratchpad, fora do repositório

| Concorrente | Condição |
|---|---|
| `detect_trigger` de hoje | referência |
| Laya multilíngue sem ajuste | perguntas em inglês **e** em pt-BR |
| Laya multilíngue | depois de ajustar as temperaturas |
| Jev | se houver chave de *early access* |
| LLM atual com JSON | só para classificar |

- **Métricas:** F1 por classe, acerto a 50% de cobertura (o quanto o limiar
  ajuda), ECE e latência p95 na sua CPU.

**C2 — Se o Laya sem ajuste não passar**
- **Ajustar o multilíngue** com o notebook oficial, feito para 2×T4 no Kaggle
  (grátis, ~4–5 h para ~30 mil perguntas).
- **Dados de treino:** parágrafos de reunião **sintéticos** em pt-BR,
  rotulados por um LLM professor. Nada das suas reuniões reais vai para o
  Kaggle.
- **Na RTX 4050 também dá**, congelando os embeddings: sobram ~125M
  parâmetros treináveis e o otimizador cabe nos 6 GB.

**C3 — Porte para Rust** (crate `isper-classify`)
- `tokenizers` + `ort` na CPU e o golden de paridade;
- download pelo `isper-models`;
- `classifier = "off" | "local" | "jev"` em Configurações → Avançado, desligado
  por padrão (ADR 0011).

**C4 — O Copilot novo**
- cards provisórios pelo classificador e texto pelo LLM, só do parágrafo;
- pulso mais espaçado;
- e2e com a fixture de reunião.

**Critério para adotar o Laya:** fixar antes de medir.
- F1 ≥ 0,8 em decisão e ação e pelo menos 15 pontos acima do `detect_trigger`
  no conjunto de teste;
- ECE ≤ 0,1 depois das temperaturas;
- p95 ≤ 0,5 s por parágrafo na sua CPU.

Se o Jev passar e o Laya não, a decisão fica entre ajustar o Laya (C2) ou
usar o Jev como está.

---

## 12. Validação do Laya (27/09/26)

Tudo rodou fora do repositório, na máquina do usuário (Ryzen 7 7735HS, CPU,
8 threads). O material, que contém trechos das reuniões e por isso não vai
para o repositório público, ficou em `Documentos\ISPer\Avaliacao-Copilot`,
com um README para rodar de novo.

### 12.1 O corpus

- **2.465 parágrafos substantivos** (8 palavras ou mais) de 29 reuniões reais
  da Biblioteca.
- **Amostra de teste:** 450 sorteados, no máximo 45 por reunião, rotulados à
  mão com um guia fixo (decisão > ação > risco > nada). Saíram 342 nada,
  57 ações, 37 riscos e 14 decisões: **24% dos parágrafos merecem card.**
- **Segundo anotador:** um LLM, às cegas, com o mesmo guia. **κ = 0,77** no
  "merece card?" e 0,76 nas quatro classes. A tarefa é consistente, então há
  um teto alto a alcançar.
- **Destilação:** mais 1.512 parágrafos, fora da amostra, rotulados por LLM
  para treino.

### 12.2 Qualidade

O filtro responde "este parágrafo merece card?". AP é a área sob a curva
precisão × cobertura, e o acaso dá 0,24.

| Abordagem | P | R | F1 | AP |
|---|---|---|---|---|
| `detect_trigger` de hoje (frases fixas) | 0,50 | **0,05** | 0,08 | 0,25 |
| "Disparar sempre" (referência trivial) | 0,24 | 1,00 | 0,39 | 0,24 |
| Laya multilíngue sem ajuste, perguntas em inglês | 0,30 | 0,50 | 0,37 | 0,36 |
| Laya sem ajuste, perguntas em pt-BR | 0,27 | 0,75 | 0,40 | 0,36 |
| Laya sem ajuste, sem a fala anterior | 0,33 | 0,42 | 0,37 | 0,33 |
| Laya calibrado (temperatura + viés, validação cruzada) | 0,25 | 0,80 | 0,39 | 0,34 |
| Laya com a cabeça ajustada nos 360 rótulos à mão (validação cruzada) | 0,26 | 0,69 | 0,38 | 0,34 |
| Laya com a cabeça ajustada nos 1.512 destilados | 0,31 | 0,31 | 0,31 | 0,33 |
| Embeddings do Laya + regressão logística (validação cruzada) | — | — | 0,48 | 0,44 |
| TF-IDF + regressão logística (destilado) | 0,41 | 0,37 | 0,39 | 0,42 |
| **Segundo anotador (LLM lendo parágrafo a parágrafo)** | **0,77** | **0,91** | **0,83** | — |

**Por classe, sem ajuste:** F1 de 0,09 em decisão, 0,09 em ação e 0,21 em
risco. O critério era F1 ≥ 0,8 em decisão e ação.

**Leitura**
- O Laya sem ajuste tira **um sinal fraco** (AP 0,36 contra 0,24 do acaso).
  Na prática, dispara em muita coisa ou em quase nada.
- A calibração corrige a confiança (ECE 0,13 → 0,01), mas não a separação:
  **calibrado, ele empata com "disparar sempre"**.
- Ajustar só a cabeça, com o codificador congelado, não melhora, nem com os
  360 rótulos à mão nem com os 1.512 destilados. A perda mal desce
  (1,10 → 1,05).
- **Nada que usa o codificador do Laya passa de AP 0,44.** Um TF-IDF simples
  chega a 0,42. O limite está na representação do mmBERT-base para essa
  tarefa, em transcrição ruidosa de pt-BR.
- **O `detect_trigger` de hoje é pior que todos:** pega **5%** dos momentos
  que merecem card. Hoje, quem encontra os cards é o pulso de 45 s com o LLM,
  não o gatilho.

### 12.3 Custo de máquina e viabilidade técnica

| Execução | p50 | p95 | RAM a mais |
|---|---|---|---|
| torch (CPU, fp32) | 618 ms | 1.442 ms | +1,5 GB (pico +1,76 GB) |
| ONNX Runtime fp32 | 597 ms | 1.687 ms | pico +1,5 GB |
| ONNX Runtime int8 dinâmico | 469 ms | 1.371 ms | pico +1,4 GB |
| **Rust** (`ort` rc.13 + `tokenizers` 0.23 + a `onnxruntime.dll` 1.28.2 do ISPer) | 563 ms | 1.739 ms | — |

Os tempos são por parágrafo, com as 3 perguntas num lote só.

- **O porte para Rust funciona.** O ONNX sai do método do `laya-ts`
  (`encoder.onnx` + `head.onnx`, exportador dynamo) com paridade de 1e-6
  contra o torch. O protótipo em Rust:
  - monta a entrada com **ids de token idênticos** aos do Python em 60/60
    sequências;
  - chega aos **mesmos logits** (|Δ| 0,0000);
  - usa a própria `onnxruntime.dll` que o instalador já leva.
- **Pegadinhas encontradas:**
  - o `scripts/export_onnx.py` da raiz do repositório do Laya congela o
    tamanho de 16 tokens na atenção e quebra com outros tamanhos (o do
    `laya-ts` está certo);
  - o `tokenizers` 1.0-rc ainda não tem o objeto `Tokenizer`, então é preciso
    ficar no 0.23;
  - os erros do builder do `ort` rc.13 carregam o builder, que não é `Send`
    (convertê-los em texto antes do `anyhow`);
  - a quantização int8 dinâmica **estraga o modelo** (|Δlogit| mediano 1,7;
    AP cai para 0,31) e só ganha 20% de tempo;
  - `resources/sherpa/onnxruntime.dll` ainda é a 1.17 antiga, enquanto o
    `target/release` tem a 1.28.2.
- **Nem o critério de latência passa:** o alvo era p95 ≤ 0,5 s, e deu
  1,4–1,7 s. A RAM, de +1,4–1,5 GB, é cinco vezes o que o app usa hoje.

### 12.4 Veredito e o que fazer no Copilot

**O Laya não vai para o Copilot.** Os três critérios do §11.5 falham:
- F1 ≥ 0,8 em decisão e ação: deu 0,09;
- ECE ≤ 0,1 com separação útil: calibrado, empata com "disparar sempre";
- p95 ≤ 0,5 s: deu 1,4 s.

Os ajustes baratos (calibração, cabeça, destilação) também não resolvem.
Ajustar o codificador inteiro na GPU é a única via aberta, mas com teto
incerto (tudo em cima desse codificador parou em AP 0,44) e com o custo de
RAM e latência de qualquer jeito. Não recomendo.

**O ganho grande no Copilot continua de pé, e o corpus mostra onde ele
está.**
- Hoje o gatilho local pega 5% dos momentos.
- Um classificador com qualidade de LLM chega a F1 ~0,8 (o segundo anotador
  teve 0,83).

Próximos passos, na ordem:
1. **Jev**, quando houver chave de *early access*. As perguntas do
   `run_laya.py` já estão no formato do `POST /v1/systemone`, e medir no
   mesmo corpus leva uma hora. Latência de 70–500 ms, custo desprezível,
   probabilidade calibrada.
2. **O LLM já configurado, parágrafo a parágrafo**, com um prompt curto e
   saída JSON ou `responseSchema` (por exemplo, um modelo pequeno do Groq).
   Precisa ser medido no mesmo corpus com a chave do usuário. O segundo
   anotador foi um LLM grande, e um modelo pequeno deve ficar abaixo dos 0,83.
3. **Vale para os dois:** o card provisório na hora e o LLM só para redigir
   (§11.1). O desenho do trait `Classifier` continua válido; o que muda é a
   implementação padrão.

### 12.5 O Jev no mesmo corpus (27/09/26)

Com a chave do usuário e o modelo fixado em `jev-1.13.0`, as perguntas foram
as mesmas do Laya, cada parágrafo numa chamada.

| No filtro "merece card?" | P | R | F1 | AP |
|---|---|---|---|---|
| `detect_trigger` de hoje | 0,50 | 0,05 | 0,08 | 0,25 |
| Laya sem ajuste | 0,30 | 0,50 | 0,37 | 0,36 |
| **Jev, perguntas em inglês** | 0,48 | 0,90 | 0,63 [0,56–0,69] | 0,73 [0,64–0,80] |
| **Jev, perguntas em pt-BR** | 0,49 | 0,92 | 0,64 [0,57–0,70] | 0,74 [0,66–0,81] |
| Segundo anotador (LLM) | 0,77 | 0,91 | 0,83 | — |

**Calibração:** sai boa sem ajuste (ECE 0,085–0,09). Na metade das respostas
em que o Jev está mais confiante, ele acerta **96%** das 4 classes.

**Por tipo, sem ajuste**, o F1 fica em 0,42–0,43 em decisão, 0,52–0,53 em
ação e 0,49–0,54 em risco. Entre os momentos que ele pega, acerta o tipo em
~80% das vezes.

**Pontos de operação (pt-BR):** limiar em p(card) = 1 − p(nada).

| Limiar | Vai para o LLM | Recall | Precisão |
|---|---|---|---|
| ≥ 0,3 | 54% | 0,93 | 0,41 |
| **≥ 0,5** | **45%** | **0,92** | **0,49** |
| ≥ 0,7 | 34% | 0,78 | 0,56 |
| ≥ 0,9 | 19% | 0,58 | 0,74 |

**Custo e latência medidos**
- **702 tokens por parágrafo** (inglês) ou 744 (pt-BR), ou seja,
  **US$ 0,00003 por parágrafo**. Com os ~230 parágrafos por hora de reunião,
  dá **~US$ 0,007 por hora**: US$ 0,15/mês a 1 h/dia e US$ 0,51/mês a
  3,4 h/dia.
- As duas rodadas completas custaram US$ 0,027.
- **Em produção vai só a pergunta do filtro** (a `choice`): 552 tokens por
  parágrafo, medidos pelo "Testar conexão" do app, ou seja,
  **~US$ 0,005 por hora de reunião**.
- **Latência daqui:** p50 ~400 ms; p95 de 480 ms (inglês) e 1,4 s (pt-BR),
  com uma cauda de rede de até 8,6 s.

**Leitura**
- **Sozinho, o Jev não passa** no critério original do §11.5 (F1 ≥ 0,8 por
  tipo).
- **Como filtro na frente do LLM, que é o desenho do §11.1, ele passa com
  folga:** pega 92% dos momentos e dispensa 55% dos parágrafos.
- **A precisão (0,49) não permite mostrar card provisório sem confirmação.**
  O LLM continua decidindo e redigindo, só que parágrafo a parágrafo e com
  pouco contexto, e não com uma janela de 20 min a cada pulso.
- **Inglês ou pt-BR dá empate estatístico.** Ficamos com pt-BR: recall um
  pouco maior e as mesmas palavras do transcript.
- **Timeout curto e fallback são obrigatórios.** Se o Jev falhar ou demorar,
  o parágrafo vai para o LLM do mesmo jeito, e nada se perde.

---

## Fontes

- [Jev e os System One models — TypeSafe](https://typesafe.ai/blog/introducing-system-one-models-and-jev)
- [Documentação da TypeSafe](https://docs.typesafe.ai/) · [API](https://docs.typesafe.ai/api.md) · [Modelos](https://docs.typesafe.ai/models.md) · [Limitações do Jev 1.13](https://docs.typesafe.ai/model-jaggedness/jev-1.13.md) · [Jurídico](https://docs.typesafe.ai/legal.md)
- [illumi-ai/sentimento-em-tempo-real](https://github.com/illumi-ai/sentimento-em-tempo-real)
- [Gemini 3.5 Transcribe — página do modelo](https://ai.google.dev/gemini-api/docs/models/gemini-3.5-transcribe) · [Transcrição de áudio](https://ai.google.dev/gemini-api/docs/transcribe) · [Transcrição ao vivo](https://ai.google.dev/gemini-api/docs/live-api/live-transcribe) · [Preços](https://ai.google.dev/gemini-api/docs/pricing) · [Termos](https://ai.google.dev/gemini-api/terms)
- [Anúncio do Gemini 3.5 Transcribe (Google)](https://blog.google/innovation-and-ai/models-and-research/gemini-models/gemini-3-5-transcribe/)
- [Fórum: vocabulário é incompatível com diarização e horários](https://discuss.ai.google.dev/t/gemini-3-5-transcribe-documented-custom-vocabulary-diarization-timestamps-configuration-is-rejected-by-the-interactions-api/180240)
- [eesel AI — análise do Gemini 3.5 Transcribe](https://www.eesel.ai/blog/gemini-3-5-transcribe-review) · [OrcaRouter — Gemini 3.5 Transcribe vs Whisper Large v3 Turbo](https://www.orcarouter.ai/blog/gemini-3-5-transcribe-vs-whisper-large-v3-turbo)
- [Laya — GitHub](https://github.com/NandhaKishorM/laya) · [README](https://github.com/NandhaKishorM/laya/blob/main/README.md) · [research/README (medições)](https://github.com/NandhaKishorM/laya/blob/main/research/README.md) · [laya-ts](https://github.com/NandhaKishorM/laya/tree/main/laya-ts) · [scripts/export_onnx.py](https://github.com/NandhaKishorM/laya/blob/main/scripts/export_onnx.py)
- [Laya — Hugging Face (model card, `rl_common.py`, `eval/results.md`)](https://huggingface.co/convaiinnovations/laya) · [mmBERT-base (MIT)](https://huggingface.co/jhu-clsp/mmBERT-base)
