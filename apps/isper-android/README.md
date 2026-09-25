# ISPer para Android

App Android do ISPer (Fase 9, "ISPer no Bolso"). Três abas:

- **Gravar** (9.2): grava a reunião inteira, com a tela apagada, em Ogg/Opus
  a 32 kbit/s ([ADR 0016](../../docs/adr/0016-gravacao-no-celular-ogg-opus.md));
- **Biblioteca**: as gravações, para ouvir, compartilhar, medir e apagar (com
  Desfazer), e o que chega de outros apps pelo "Compartilhar". No topo, o PC
  pareado (9.3): as gravações vão para ele, e a ata volta
  ([ADR 0017](../../docs/adr/0017-sincronia-celular-pc.md));
- **Laboratório** (9.1): mede no próprio celular o mesmo passe final que o
  ISPer roda no PC — decodificação, VAD Silero, Whisper com busca em feixe,
  falante por palavra.

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

## O gravador

- **Começar:** o botão da aba Gravar, o widget "Gravar reunião" na tela
  inicial ou o bloco "Gravar" nas Configurações rápidas (puxe a barra de
  notificações e edite os blocos). Na primeira vez, o Android pede o
  microfone e as notificações.
- **Durante:** a notificação mostra o cronômetro e tem **Marcar** (um
  momento para achar depois), **Pausar** e **Parar**. Pode apagar a tela e
  usar outros apps. Numa ligação, o Android silencia o microfone: a gravação
  continua, e o trecho fica marcado.
- **Onde fica:** `Android/data/com.isper.mobile/files/Gravacoes`, um `.opus`
  e um `.json` (o manifesto) por gravação, com ~15 MB por hora. Se o app
  morrer no meio, o áudio até a queda fica, e a gravação aparece como
  recuperada na próxima abertura.
- **Levar para o PC:** pareado com um PC (a seção abaixo), a gravação vai
  sozinha. Sem pareamento, **Compartilhar** na Biblioteca manda o `.opus` por
  WhatsApp, e-mail ou Drive, e o ISPer do PC importa o arquivo como qualquer
  outro.

> Alguns fabricantes (Xiaomi, Samsung, Motorola) matam apps em segundo plano
> mesmo com a notificação. Se uma gravação longa parar sozinha, libere o
> ISPer em Bateria → Sem restrições.

`tools/e2e/android-recorder.ps1` testa o gravador num emulador pela
interface (21 verificações): gravar, marcar e parar; matar o app no meio e
recuperar; gravar com a tela apagada.

## O PC pareado (Fase 9.3)

- **Parear:** no PC, Configurações → Celular → *Parear um celular*; no app,
  Biblioteca → **Ler o QR do PC** (o leitor do Google, sem a permissão da
  câmera) ou **Colar o código** (o PC tem "Copiar o código"). O PC pergunta
  "Permitir?".
- **Mandar:** o WorkManager roda uma rodada ao parar uma gravação, ao abrir o
  app, em **Enviar agora** e a cada 15 min, sempre com rede. O PC fora de
  alcance faz o Android tentar de novo mais tarde; enquanto o PC transcreve,
  há outra rodada em 90 s. O mDNS acha o PC se o IP dele mudar.
- **Onde fica:** a chave do celular e o PC pareado ficam na pasta interna do
  app (`files/sync`); ao lado de cada gravação, `<id>.sync.json` diz em que pé
  ela está no PC, e `<id>.ata.md` é a ata que voltou.
- **Ata pronta:** notificação e a tela da ata, com **Compartilhar** (o texto
  vai para o WhatsApp, o e-mail ou o Teams).

`tools/e2e/android-sync.ps1` testa isso num emulador, com o `isper-cli
receber` fazendo o papel do PC: colar o código, parear, gravar, a gravação
chegar ao PC, a ata voltar e abrir, desconectar.

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
| Xiaomi, Snapdragon 855/860 (SM8150) (24/09) | small q5 | corpus, 190 s | ~272 s (1,43×) | 3 de 3 | 10,9% · 22,1% | — |

O emulador prova que o pipeline do PC funciona no Android. A velocidade de
verdade só se mede num celular: o Xiaomi foi o primeiro. Falta o aparelho do
líder.

## Assinatura

O Android só atualiza um app por cima se a versão nova vier assinada com a
**mesma chave**. Com a chave de debug de cada run do CI, instalar o APK novo
exigia desinstalar o antigo, e isso apagava as gravações. O workflow assina
com a chave do ISPer quando os segredos do repositório existem:

| Segredo | Conteúdo |
|---|---|
| `ANDROID_KEYSTORE_B64` | o `.jks` em base64 |
| `ANDROID_KEYSTORE_PASSWORD` | a senha do `.jks` (a mesma da chave, alias `isper`) |

Criar a chave é à mão, uma vez, por quem mantém o repositório. O `keytool`
vem com o JDK e com o Android Studio (`jbr\bin`):

```bash
keytool -genkeypair -keystore isper-android.jks -alias isper -keyalg RSA -keysize 4096 -validity 36500 -dname "CN=ISPer" -storetype PKCS12
```

Depois, em GitHub → Settings → Secrets and variables → Actions, crie os dois
segredos. Guarde o `.jks` e a senha num gerenciador de senhas: sem eles, a
próxima versão não atualiza por cima. O `.gitignore` recusa `*.jks`.

Num build local, as variáveis `ISPER_ANDROID_KEYSTORE` (caminho do `.jks`) e
`ISPER_ANDROID_KEYSTORE_PASSWORD` fazem o mesmo. Sem elas, vale a chave de
debug da máquina.

**Trocar de chave apaga as gravações do aparelho.** O Android recusa a
instalação nova (`INSTALL_FAILED_UPDATE_INCOMPATIBLE`) mesmo que a antiga
tenha sido desinstalada com "manter os dados": é preciso apagar os dados.
Antes de trocar, compartilhe as gravações para o PC. O `hasFragileUserData`
só protege quem desinstala por engano e reinstala com a mesma chave:
aí o Android oferece manter os dados, e as gravações voltam.

## Versões

AGP 9.4 com o Kotlin embutido, Gradle 9.8 (wrapper com SHA-256), Kotlin 2.4,
Compose BOM 2026.09. `compileSdk` 37 (as bibliotecas androidx atuais exigem),
`targetSdk` 36, `minSdk` 29. NDK `28.2.13676358`. As versões ficam em
`gradle/libs.versions.toml`.
