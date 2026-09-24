# 0014 — Celular: app nativo, com o núcleo Rust exposto pelo UniFFI

- **Status:** aceita
- **Data:** 24/09/2026

## Contexto

A Fase 9 leva o ISPer ao celular para substituir o Plaud: gravar reuniões
presenciais e transcrevê-las depois, no aparelho ou no PC (o líder não
precisa de tempo real). O núcleo já existe e é Rust portátil. Do
`isper-core`, só `loopback.rs` e `calls.rs` (~600 de ~10 mil linhas) são
exclusivos do Windows. Faltava decidir **como** o celular fala com ele.

O que pesou:

- **Gravar em segundo plano, widget, bloco nas Configurações rápidas,
  compartilhar e notificação viva são código nativo em qualquer framework.**
  É a camada que decide se uma gravação se perde.
- **O Tauri 2 roda no Android**, mas tem um bug aberto de tela branca quando
  um serviço em primeiro plano mantém o processo vivo
  ([tauri#15671](https://github.com/tauri-apps/tauri/issues/15671)). É
  exatamente o cenário de um gravador. E a interface do desktop (bandeja,
  janelas, indicador flutuante) não se aproveita no celular.
- **Projetos grandes com núcleo em Rust** fazem assim: Firefox
  (application-services), Element X (matrix-rust-sdk) e Bitwarden. Todos
  usam o UniFFI, da Mozilla.

## Decisão

1. **Interface nativa por plataforma, sobre o mesmo núcleo.** Android em
   Kotlin com Jetpack Compose agora (`apps/isper-android`). iOS em SwiftUI
   depois que o Android validar o produto (9.7).
2. **Uma fachada só, `crates/isper-mobile`**, exportada pelo UniFFI (modo
   proc-macro). Os bindings Kotlin e, depois, Swift são **gerados** a partir
   da biblioteca compilada (`crates/uniffi-bindgen`, modo "library"): uma
   assinatura que muda quebra a compilação do app, não o app em uso. O
   UniFFI fica numa versão fixa (`=0.32.2`, ainda 0.x). Subir de versão é um
   PR próprio.
3. **Toda a regra fica no Rust.** A fachada expõe operações inteiras
   (baixar um modelo, transcrever um arquivo com o passe final do PC, medir)
   e o Kotlin só guarda o estado da tela. O progresso volta por uma
   interface implementada no Kotlin (`ProgressListener`).
4. **O mesmo passe final do PC**, sem ramo "mobile": o celular não tem ao vivo
   ([ADR 0006](0006-dois-modos-ao-vivo-e-passe-final.md)), então só o modo
   final vai para lá.
5. **Build pelo Gradle, com versões correntes:** AGP 9.4, Gradle 9.8 (wrapper
   com SHA-256), Kotlin 2.4 com o Kotlin embutido do AGP 9, Compose BOM
   2026.09, catálogo de versões. `minSdk` 29 (Android 10: é quando entra o
   encoder Opus que o gravador da 9.2 vai usar) e `targetSdk` 36. Três
   tarefas próprias entram como fontes geradas pela API de variantes:
   `buildRustLibs` (o `cargo-ndk`), `generateUniffiBindings` e
   `spikeAssets`.
6. **arm64 compilado para ARMv8.2 com dotprod e fp16**
   (`GGML_CPU_ARM_ARCH`), o que celulares de 2019 em diante têm e o que dá
   velocidade às matrizes quantizadas do ggml. O app confere o
   `/proc/cpuinfo` antes de carregar a biblioteca e avisa em vez de morrer
   num aparelho mais antigo.
7. **O primeiro entregável é um laboratório, não o gravador**: um app que
   mede o passe final no próprio aparelho (tempo por etapa, fator de tempo
   real, memória, bateria, temperatura, WER/CER/DER na amostra) e roda sem
   tocar na tela (`autorun`), para o spike no celular do líder e para o e2e
   no emulador. O gravador (9.2) cresce dentro deste mesmo app.

## Consequências

- **Fica melhor:** o celular roda o mesmo código medido no PC. Uma melhoria no
  pipeline vale para os dois, e o `isper-cli bench` continua sendo a régua.
- **Fica pior:** duas interfaces a longo prazo, e o Kotlin é linguagem nova no
  projeto. As interfaces são finas, porque a regra está no Rust.
- **Compilação cruzada do whisper.cpp pelo `whisper-rs-sys` 0.15 exige três
  ajustes**, todos na tarefa `buildRustLibs`:
  - um toolchain file por ABI que inclui o do NDK. Sem ele o cmake-rs só
    declara `CMAKE_SYSTEM_NAME=Android`, e o CMake não acha o NDK;
  - os bindings que vêm no crate (`WHISPER_DONT_GENERATE_BINDINGS`). O
    libclang do PC não acha os headers do clang para o Android, e os tipos são
    os mesmos em qualquer alvo de 64 bits;
  - **num PC Windows**, o build script decide pelo sistema de quem compila:
    passa `/utf-8` (flag do MSVC) ao clang e pede para ligar a `advapi32`. O
    toolchain file tira a flag, e uma `libadvapi32.a` vazia satisfaz o link.
    No runner Linux do CI isso não acontece. Vale mandar a correção ao
    projeto (hoje no Codeberg): trocar `cfg!(target_os)` por
    `CARGO_CFG_TARGET_OS` resolve.
- **Obrigatório:** o workflow `android.yml` compila o APK a cada mudança no
  núcleo ou no app, e `tools/e2e/android-lab.ps1` roda o laboratório no
  emulador. O emulador prova que o pipeline funciona no Android, mas não
  mede a velocidade de um celular: isso só no aparelho.

## Alternativas consideradas

- **Tauri 2 no celular.** Reaproveita a linguagem da interface, mas não o
  código dela. A camada de sistema seria um plugin Kotlin/Swift do mesmo
  jeito. O bug do serviço em primeiro plano está aberto.
- **Flutter com flutter_rust_bridge.** Uma interface só, mas acrescenta Dart,
  e a gravação em segundo plano continua dependendo de código nativo.
- **React Native com uniffi-bindgen-react-native.** Ainda 0.x, e acrescenta
  JavaScript e o Metro ao build de um app que é, no fundo, um gravador.
- **Kotlin Multiplatform com Compose Multiplatform no iOS.** Fica para quando
  o iOS começar (9.7). Os bindings do UniFFI para Kotlin/Native existem
  (Gobley), mas são de terceiros. A decisão da interface do iOS será tomada
  com o Android pronto.

## Onde vive

- `crates/isper-mobile` (a fachada), `crates/uniffi-bindgen` (o gerador)
- `apps/isper-android` (o app; `app/build.gradle.kts` tem as tarefas do Rust)
- `.github/workflows/android.yml`, `tools/e2e/android-lab.ps1`
- Portabilidade do núcleo: `crates/isper-core/src/loopback.rs` (stub fora do
  Windows), dependências por plataforma nos `Cargo.toml` do núcleo e da IA
