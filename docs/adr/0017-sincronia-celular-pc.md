# 0017 — Sincronia celular → PC: iroh com a chave do PC no QR e um protocolo pequeno e retomável

- **Status:** aceita
- **Data:** 25/09/2026

## Contexto

A Fase 9.3 fecha o circuito que substitui o Plaud: o celular grava (9.2), o
PC transcreve com o passe final e a GPU, e a ata volta para o celular. O
critério do plano: gravar longe do PC, chegar perto dele, e a ata aparecer no
celular sem ninguém tocar em nada. É isso que responde aos "90% do uso é
baixar a transcrição" da conversa com o líder.

As restrições:

- **local-first e custo zero:** nada de conta nem de servidor para manter; o
  áudio é dado sensível (LGPD) e não deve passar por terceiros;
- **o PC é um Windows com firewall**, instalado por usuário, sem
  administrador;
- **a rede muda:** o celular troca de Wi-Fi e de dados, e o DHCP muda o IP
  do PC;
- **2 h de reunião são ~30 MB** (ADR 0016): o envio tem de continuar de onde
  parou se a conexão cair;
- **quem pode mandar:** só um celular que o dono do PC aprovou, e o celular
  só manda para o PC que ele escolheu.

## Decisão

1. **Transporte: [iroh](https://www.iroh.computer) 1.2** (QUIC; cada lado é
   uma chave Ed25519, e a conexão só se estabelece com a chave esperada). O
   spike, antes de decidir: 30 MB em 0,34 s entre dois endpoints na mesma
   máquina, e a mesma biblioteca compilou para Windows e para Android arm64
   com o `cargo-ndk` do projeto, sem nenhum contorno. O padrão não fala com
   ninguém de fora: `default-features = false` (sem UPnP), preset `Minimal` e
   `RelayMode::Disabled` (sem os relays nem o DNS da n0). O **relay é opção**
   (Configurações → Celular → Avançado), para quando o celular está fora da
   rede; ele só repassa pacotes cifrados.
2. **Pareamento por QR.** O PC mostra
   `isper://parear?v=1&pc=<chave>&s=<segredo>&n=<nome>&a=<ip:porta,…>&r=<relay>`:
   a chave pública do PC, um segredo de 128 bits de uso único que vale 2 min
   (5 tentativas erradas encerram o pareamento), o nome e os endereços. O
   celular conecta àquela chave e manda o segredo; o PC ainda pergunta
   "*Galaxy Tab A9* quer parear. Permitir?", porque um QR pode ser
   fotografado. Depois disso, **a autorização é a própria chave do
   celular**, guardada no banco do PC: não há senha nem token para vazar. O
   celular fixa a chave do PC (só se disca uma chave específica). "Esquecer",
   no PC, ou "Desconectar", no celular, desfazem.
   O QR é lido pelo **Google Code Scanner**, que roda no aparelho pelo Play
   services e não pede ao app a permissão da câmera. "Colar o código" é a
   saída sem ele: o PC tem "Copiar o código", e o texto é o mesmo do QR.
3. **Protocolo `isper/sync/1`** (o ALPN). Cada pedido abre um stream: um
   quadro com 4 bytes de tamanho e o JSON (até 8 MB, a ata cabe), e a
   resposta. Pedidos: `Pair`, `Hello`, `Offer`, `Upload`, `Status`, `Minutes`
   e `Forget`. **O envio é retomável:** o PC grava num `.part`, o `Offer`
   diz quanto já chegou, e o `Upload` continua dali. No último byte, o SHA-256
   é conferido (o mesmo hash que a importação da 9.0 usa para não duplicar
   reunião); se não bater, o PC descarta e o celular manda de novo. O id da
   gravação vira nome de arquivo e só aceita `[A-Za-z0-9_-]{1,64}`.
4. **No PC**, o arquivo fica em `Documentos\ISPer\Do celular\<aparelho>\` com
   o manifesto ao lado, e entra na **mesma fila de importação** da 9.0
   (`Origin::Phone`), já com a data do manifesto, os momentos (★) como
   momentos da reunião e a origem "*aparelho* · *arquivo*". O estado vai para
   o banco (schema v6: `sync_devices` e `sync_items`); o que chegou e não
   virou reunião volta para a fila na abertura do app. A ata que volta é o
   `.md` da reunião. **Desligado por padrão** (`phone_sync`): ligado, o PC
   abre a porta UDP 47823 e o Windows pergunta, na primeira vez, se o ISPer
   pode usar a rede.
5. **No celular**, o **WorkManager** roda uma rodada quando uma gravação
   termina, quando o app abre, em "Enviar agora" e a cada 15 min (o mínimo do
   Android), sempre com rede. Enquanto o PC transcreve, há outra rodada em
   90 s. Com o PC fora de alcance, a nova tentativa vem em 1, 2, 4, 8 e depois
   15 min, por um agendamento à parte: a rodada nunca termina em
   `Result.retry()`, porque a espera do WorkManager travava a fila, e um
   "Enviar agora" esperava atrás dela (visto no emulador). O celular conecta
   **sem GSO** (vários pacotes UDP num envio só). No emulador, o driver recusou
   o primeiro lote grande com EIO, e o QUIC ficou parado até a conexão cair.
   O celular manda pouco, e sem GSO nenhum driver quebra o envio. O núcleo
   grava o próprio log em `files/sync/nucleo.log`, porque o logcat não vê o
   `tracing` do Rust. O mDNS (`iroh-mdns-address-lookup`, serviço `isper-sync`) acha
   o PC quando o IP dele muda. O estado de cada gravação fica em
   `<id>.sync.json`, e a ata em `<id>.ata.md`, ao lado do áudio; a chave do
   celular fica na pasta interna do app. A ata chega com a notificação "Ata
   pronta" e abre numa tela com "Compartilhar".

## Consequências

- **Fica melhor:** a gravação sai do celular e a ata volta sem ninguém
  configurar nada além de ler um QR. Não há conta nem servidor, e o áudio não
  passa por terceiros no padrão.
- **Fica melhor:** os dois lados se autenticam pela chave, o envio continua
  de onde parou e a integridade é conferida. O mesmo protocolo serve ao iOS
  (9.7), porque vive no núcleo Rust (`isper-sync`) e não no Kotlin.
- **Fica melhor:** o `isper-cli` ganhou os dois lados (`receber` faz de PC,
  `enviar` faz de celular). Os e2e usam isso, e dá para testar o app Android
  sem o desktop e o desktop sem um celular.
- **Fica pior:** 118 pacotes a mais no `Cargo.lock` (o iroh e o que ele traz;
  tokio, rustls e companhia já estavam lá), no app desktop e no APK.
- **Fica pior:** o firewall do Windows pergunta na primeira vez. Sem o
  "Permitir", a conexão direta na rede local não chega ao PC.
- **Fica pior:** o padrão é só a rede local. Para "gravei longe, só tenho
  internet", é preciso configurar um relay.
- **Obrigatório:** o leitor de QR depende do Play services, e sem ele sobra
  colar o código.
- **Obrigatório:** a chave do PC (`%APPDATA%\ISPer\sincronia-chave.txt`) é a
  identidade dele perante os celulares. Perdida, cada celular pareia de novo,
  e nada mais se perde. Ela tem a mesma proteção do banco
  ([ADR 0010](0010-criptografia-em-repouso-adiada.md)).
- **Pendente:** a volta completa num aparelho de verdade, e o relay.

## Alternativas consideradas

- **Servidor HTTPS no PC** (axum + rustls, com o certificado autoassinado
  fixado no QR): menos dependências. Mas funciona só na rede local, exige uma
  regra de entrada TCP no firewall e a gestão do certificado, e não atravessa
  NAT.
- **iroh-blobs** para a transferência: ele mesmo se declara não pronto para
  produção. A parte retomável aqui tem umas 150 linhas.
- **Uma pasta na nuvem** (Drive, Dropbox) como caixa de correio: o áudio
  sairia dos aparelhos do usuário, e haveria contas no meio.
- **Código de 6 dígitos com PAKE** (SPAKE2) em vez do QR: dispensa a câmera,
  mas é mais criptografia nossa, com um crate sem auditoria. O QR leva a chave
  de 256 bits direto.
- **Token depois do pareamento:** desnecessário quando a chave já autentica,
  e seria mais um segredo para guardar.
- **ZXing embutido** como leitor de QR: pede a permissão da câmera e a
  biblioteca está parada. O Code Scanner não pede.

## Onde vive

- `crates/isper-sync` (`code.rs`, `proto.rs`, `server.rs`, `client.rs` e
  `tests/sync.rs`: o protocolo de ponta a ponta)
- `crates/isper-mobile/src/sync.rs` (o `PcLink` para o Kotlin)
- `crates/isper-core/src/store.rs` (schema v6)
- `apps/isper-app/src-tauri/src/phone_sync.rs`, `audio_import.rs`
  (`Origin::Phone`) e `ui/settings.html` (o cartão Celular)
- `apps/isper-android/.../sync/` (`PcSync`, `SyncWorker`) e `.../library/`
  (`PcCard`, `MinutesScreen`)
- `crates/isper-cli/src/sync.rs` (`receber` e `enviar`)
- `tools/e2e/sync.ps1` (desktop) e `tools/e2e/android-sync.ps1` (emulador)
