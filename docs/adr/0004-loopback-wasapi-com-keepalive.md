# 0004 — Loopback pelo WASAPI com keepalive, drenagem e watchdog

- **Status:** aceita
- **Data:** 02/09/2026

## Contexto

O notetaker não usa bot nem API do Teams: grava o microfone ("Eu") e o que
sai na caixa de som ("Participantes"), capturado por **loopback do WASAPI**.
O primeiro caminho foi o loopback do `cpal`, o mesmo crate do microfone. Na
máquina de referência (endpoint de áudio USB) ele falhava de um jeito
silencioso:

- o loopback só entrega dados enquanto o endpoint está **ativo**; quando nada
  toca, o dispositivo suspende e o stream estagna — às vezes sem voltar;
- streams dirigidos a eventos morrem junto: os eventos simplesmente param de
  chegar, e um keepalive feito pelo próprio `cpal` morre com eles;
- abrir microfone e alto-falante do mesmo dispositivo USB ao mesmo tempo
  fazia o loopback perder os primeiros ~7 s.

Uma reunião com pausas longas perdia fala sem nenhum erro no log.

## Decisão

A captura do sistema usa o crate **`wasapi`** direto, numa **única thread com
polling** (o ISPer controla a cadência), com três defesas:

1. **Keepalive integrado:** um cliente de *render* no mesmo endpoint recebe
   silêncio a cada ciclo — o dispositivo nunca fica ocioso.
2. **Drenagem completa:** `GetBuffer` devolve um pacote de ~10 ms por
   chamada, então cada ciclo drena até esvaziar.
3. **Watchdog por bytes:** com o keepalive, um loopback saudável entrega
   continuamente (silêncio incluído); mais de 1 s sem nenhum byte novo
   significa que estagnou, e o cliente é fechado e reaberto.

Além disso: o canal dos participantes abre **antes** do microfone, em
sequência; e o loopback pode ser **por processo** (só o Teams, com os filhos
WebView2), via `new_application_loopback_client`, com volta ao sistema
inteiro e aviso se o Teams não estiver aberto.

## Consequências

- Validado: 20 s de 20 capturados com stream contínuo, e reuniões reais
  gravadas desde 07/09 sem perda.
- O relógio da reunião, e não a contagem de amostras, dá os horários: o
  loopback não entrega amostras nas pausas, e contar amostras derraparia.
- O loopback captura **tudo** o que toca no PC (no modo sistema) — inclusive
  um vídeo em outra aba. Por isso o modo "Só o Microsoft Teams" existe e o
  usuário é lembrado de avisar os participantes (LGPD).
- É código de Windows puro; outra plataforma pede outra implementação.

## Alternativas consideradas

- **Loopback do `cpal`.** Descrito acima: estagnava sem erro neste endpoint.
- **Bot de reunião ou API do Teams.** Exige conta, permissões de organização
  e, em geral, pagar; e põe um participante estranho na chamada.
- **Driver de áudio virtual.** Exigiria instalar um driver com privilégio de
  administrador e mudar a saída de áudio do usuário.

## Onde vive

`crates/isper-core/src/loopback.rs` (o comentário do módulo repete estas
regras), `crates/isper-core/src/meeting.rs` (ordem de abertura, relógio),
`isper-cli --bin loopdump` para depurar. Fase 4 do `ROADMAP.md`.
