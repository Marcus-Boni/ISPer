# ISPer para Android

App Android do ISPer (Fase 9, "ISPer no Bolso"). Por enquanto é o
**laboratório da 9.1**: mede no próprio celular o mesmo passe final que o
ISPer roda no PC — decodificação, VAD Silero, Whisper com busca em feixe,
falante por palavra — antes de o gravador (9.2) ser construído em cima dele.
A decisão de arquitetura está no [ADR 0014](../../docs/adr/0014-celular-nativo-com-nucleo-rust.md).

```text
Kotlin + Jetpack Compose  (apps/isper-android/app/src/main/java)
        │  bindings gerados pelo UniFFI (com.isper.mobile.core)
        ▼
isper-mobile  (crates/isper-mobile: a fachada do núcleo)
        │
        ▼
isper-core · isper-diarize · isper-models   (o mesmo Rust do desktop)
```

## O que o laboratório mede

- tempo de cada etapa e o **fator de tempo real** (tempo total ÷ duração do
  áudio: abaixo de 1, o celular é mais rápido que a reunião);
- **pico de memória** do processo (`VmHWM`);
- **bateria, temperatura e estado térmico** antes e depois;
- **WER, CER e DER** quando o áudio tem referência. A amostra embutida é o
  corpus de regressão do desktop (190 s, 3 vozes) quando ele existe na
  máquina que montou o APK.

O relatório sai na tela, em "Compartilhar o relatório" (JSON), no logcat (tag
`ISPerSpike`) e em `Android/data/com.isper.mobile/files/spike-report.json`.

## Montar o APK

Pré-requisitos: Android SDK com o NDK `28.2.13676358` (o Android Studio
instala), JDK 17+, Rust com os alvos do Android e o `cargo-ndk`:

```bash
rustup target add aarch64-linux-android x86_64-linux-android
cargo install cargo-ndk --version 4.1.2 --locked
```

```bash
cd apps/isper-android
./gradlew assembleDebug
```

O `assembleDebug` faz tudo: `buildRustLibs` compila o `isper-mobile` para
cada ABI (`cargo ndk`) e junta as `.so` do sherpa-onnx e a `libc++_shared.so`;
`generateUniffiBindings` gera o Kotlin a partir da biblioteca compilada;
`spikeAssets` embute a amostra. O APK sai em
`app/build/outputs/apk/debug/app-debug.apk`.

`-Pisper.abis=arm64-v8a` compila só para celular (metade do tempo). O padrão
inclui `x86_64` para o emulador.

> A biblioteca arm64 é compilada para **ARMv8.2 com dotprod e fp16**
> (celulares de 2019 em diante). Num aparelho mais antigo, o app avisa em vez
> de abrir o motor.

## Rodar o spike num celular

1. Instale o APK (arquivo enviado, ou `adb install -r app-debug.apk`).
2. Abra o **ISPer**, escolha um modelo e toque no tamanho para baixar
   (Hugging Face, com SHA-256 conferido). Para comparar aparelhos, comece pelo
   **Small (q5)**.
3. Deixe "Reunião de exemplo" e "Separar os falantes" ligados e toque em
   **Medir**. A tela fica acesa até terminar.
4. **Compartilhar o relatório** manda o JSON por WhatsApp ou e-mail.

Sem tocar na tela, com o celular no cabo:

```bash
adb shell am start -n com.isper.mobile/.MainActivity --ez autorun true --es model ggml-small-q5_1.bin --ez diarize true
adb pull /sdcard/Android/data/com.isper.mobile/files/spike-report.json
```

`tools/e2e/android-lab.ps1` faz isso no emulador: sobe o AVD, instala,
roda com o modelo tiny e confere o relatório (12 verificações).

## O que já foi medido

| Onde | Modelo | Áudio | Total | Falantes | WER · DER | Pico de memória |
|---|---|---|---|---|---|---|
| Emulador x86_64, 4 núcleos (24/09) | tiny q5 | corpus, 190 s | 81 s (0,43×) | 3 de 3 | 18,0% · 21,4% | 576 MB |

O emulador prova que o pipeline do PC funciona no Android. A velocidade de
verdade só se mede num celular: a próxima linha desta tabela é a do aparelho
do líder.

## Versões

AGP 9.4 com o Kotlin embutido, Gradle 9.8 (wrapper com SHA-256), Kotlin 2.4,
Compose BOM 2026.09. `compileSdk` 37 (as bibliotecas androidx atuais exigem),
`targetSdk` 36, `minSdk` 29. NDK `28.2.13676358`. As versões ficam em
`gradle/libs.versions.toml`.
