# 0015 — O crate oficial do sherpa-onnx no lugar do sherpa-rs, com threads

- **Status:** aceita
- **Data:** 24/09/2026

## Contexto

A diarização ([ADR 0005](0005-diarizacao-pos-hoc.md)) usava o sherpa-onnx
pelo `sherpa-rs` 0.6, um wrapper da comunidade. Dois problemas:

- **Uma thread só.** O `sherpa-rs` fixa `num_threads: 1` na segmentação e nos
  embeddings e não expõe o parâmetro. A diarização era o gargalo do passe
  final: 30 min de áudio levavam 12,8 min.
- **Descontinuado.** O autor encerrou o `sherpa-rs` em favor do crate
  oficial da k2-fsa, `sherpa-onnx`. A Fase 9.1 precisa do sherpa-onnx também
  no Android e no iOS, e o crate oficial baixa as bibliotecas pré-compiladas
  dessas plataformas.

As bibliotecas nativas são **as mesmas**: as DLLs do Windows que o crate
oficial 1.13.8 baixa têm os mesmos bytes das que o `sherpa-rs` baixava. A
troca muda o wrapper Rust, não o motor.

## Decisão

1. `isper-diarize` passa a usar `sherpa-onnx = "=1.13.8"` com a feature
   `shared` (DLLs no Windows). O pacote estático do Windows é compilado com
   `/MT` e não se mistura com o resto do app, que usa `/MD`. No Android e no
   iOS o próprio crate só oferece o modo compartilhado.
2. **Threads configuráveis** (`DiarizeOptions::threads`, com a variável
   `ISPER_DIARIZE_THREADS` para medir). O padrão, 0, é a escolha automática
   `auto_threads()`: metade dos núcleos lógicos, entre 1 e 8. Medido no
   corpus de 190 s (3 falantes) num Ryzen 7 7735HS, com 8 núcleos e 16
   threads, e saída idêntica em todas as rodadas (DER 27,37%, 3 falantes):

   | threads | 1 | 4 | 8 | 16 | automático (8) |
   |---|---|---|---|---|---|
   | segundos | 63,6 | 30,8 | 22,7 | 31,3 | 24,4 · 26,2 · 25,2 |

   Com o `sherpa-rs`: 59,6 s e 58,1 s. Passar dos núcleos físicos piora,
   porque o SMT disputa as mesmas unidades de ponto flutuante. A metade dos
   núcleos lógicos é a melhor aproximação portátil dos físicos, e num celular
   de 8 núcleos sem SMT dá 4, que em geral são os rápidos.
3. **Caminhos explícitos** para quem não tem as pastas do PC:
   `diarize_with_models`, `download_models_to` e `model_paths_in`. É o que o
   celular usa.
4. O pré-compilado passa a morar em `target/sherpa-onnx-prebuilt`, e não mais
   em `%LOCALAPPDATA%\sherpa-rs`. Os workflows guardam essa pasta no cache.
   Se o cache trouxer a saída do build script sem a pasta, apagam a saída
   para o build script rodar de novo. É a mesma armadilha do LNK1181 da época
   do `sherpa-rs`.

## Consequências

- **Fica melhor:** a identificação de falantes ficou ~2,4× mais rápida no PC
  (de ~60 s para ~25 s nos 190 s do corpus), sem mudar o resultado. As
  reuniões ganham "Participante 1, 2…" bem antes. A estimativa de 5–8× do
  roadmap não se confirmou: parte do tempo é o agrupamento, que não usa
  threads.
- **Fica melhor:** o mesmo crate serve ao celular.
- **Muda o instalador:** a `cargs.dll` sai. Ela vinha do pacote do
  `sherpa-rs` e nenhum binário a importava (o exe importa a
  `sherpa-onnx-c-api.dll`, que importa a `onnxruntime.dll`). Ficam quatro
  DLLs do sherpa-onnx.
- **Obrigatório:** a versão fica fixa (`=1.13.8`). Subir de versão é um PR
  próprio, medido com `isper-cli diarize --reference-turns`.

## Alternativas consideradas

- **Continuar no `sherpa-rs` e mandar um PR para expor `num_threads`.** O
  projeto foi descontinuado, e o celular precisaria do crate oficial de
  qualquer jeito.
- **Chamar a API C do sherpa-onnx direto (`sherpa-onnx-sys`).** Seria
  reescrever o wrapper seguro que o crate oficial já mantém.
- **Linkagem estática no Windows.** Exige tudo em `/MT`, e o whisper.cpp e o
  Rust do app estão em `/MD`: o link reclama (LNK4098) e o CRT duplicado é
  fonte de bugs sutis.

## Onde vive

- `crates/isper-diarize/src/lib.rs` (`diarize_with_models`, `auto_threads`)
- `crates/isper-cli/src/main.rs` (`diarize --reference-turns`)
- `.github/workflows/{ci,coverage,e2e-nightly,release}.yml`,
  `scripts/release.ps1`, `apps/isper-app/src-tauri/tauri.conf.json`
