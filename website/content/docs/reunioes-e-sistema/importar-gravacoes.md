---
title: "Importar gravações"
description: "Transcreva no ISPer gravações de fora — do Plaud, do celular, de uma reunião gravada — em MP3, M4A, WAV, FLAC, OGG ou Opus."
section: "Reuniões e Sistema"
order: 46
---

# Importar gravações

O ISPer também transcreve áudio que **não** foi gravado por ele: o que o Plaud exporta, o gravador do celular, a gravação de uma reunião presencial ou do Teams. Cada arquivo vira uma reunião na Biblioteca, com o mesmo passe final de uma reunião gravada pelo ISPer, a identificação de quem falou e, se houver IA configurada, o resumo.

> [!NOTE/Observação]
> Tudo roda no seu computador, como nas reuniões gravadas: o áudio não sai da máquina. Só o texto vai ao provedor de IA, e só se você tiver configurado um.

## Formatos

| Formato | De onde costuma vir |
|---|---|
| MP3 | Plaud, gravadores de voz |
| M4A, MP4, AAC | Gravador do iPhone e de muitos Androids; vídeos de reunião (vale a trilha de áudio) |
| WAV | Plaud, gravadores profissionais |
| FLAC, OGG | Gravadores e apps de áudio |
| Opus | Mensagens de voz do WhatsApp (`.opus`, às vezes `.ogg`) e o gravador do ISPer no celular |

## Pela Biblioteca

1. Abra a Biblioteca.
2. Clique em **Importar áudio** e escolha um ou mais arquivos, ou arraste os arquivos para a janela.
3. A faixa no alto mostra o que está acontecendo: lendo o áudio, transcrevendo (com a porcentagem), identificando quem falou, gerando o resumo.
4. Quando termina, a reunião aparece na lista com o selo **importada**, e o Windows avisa.

Um arquivo por vez: a transcrição usa a mesma placa de vídeo do ditado. Se houver uma reunião sendo gravada, a importação espera ela acabar. **Cancelar** interrompe o arquivo atual e os que estão na fila.

## Pela pasta Importar

Tudo o que você puser em `Documentos\ISPer\Importar` vira reunião sozinho, com o ISPer aberto. Serve para deixar a pasta de exportação do Plaud, ou uma pasta sincronizada com o celular (OneDrive, Google Drive), apontando para lá.

- O ISPer espera o arquivo terminar de ser copiado antes de ler.
- Depois de virar reunião, o arquivo vai para a subpasta **Importados**.
- Se não der para ler (arquivo corrompido, formato que o ISPer não lê), ele vai para **Não importados**, com um `.txt` ao lado dizendo por quê.

Nada é apagado. Para desligar, desmarque Configurações → Reuniões → *Transcrever o que eu puser na pasta Importar*. O botão **Abrir a pasta**, ali mesmo, leva até ela.

## Do Plaud

O aparelho do Plaud não aparece mais como pendrive no computador. No app do Plaud, exporte o áudio da gravação em **MP3** ou **WAV** e salve o arquivo no computador: direto na pasta `Importar`, ou em qualquer lugar, para depois arrastar na Biblioteca.

## O que sai

- **Falantes:** todos estão no mesmo áudio, então as falas saem como "Participante 1", "Participante 2"… (não há o canal "Eu" de uma reunião gravada pelo ISPer). Renomeie os falantes na Biblioteca como numa reunião comum.
- **Data:** a do nome do arquivo, quando ele traz data e hora (`2026-09-22 15-30-46.mp3`, `REC_20260922_153046.m4a`); senão, a data do arquivo.
- **Título:** o nome do arquivo, quando ele descreve a conversa ("Reunião com fornecedor.mp3"). Um nome de gravador ("REC_0012", "Nova gravação 3") dá lugar ao título sugerido pela IA ou ao padrão com a data.
- **Origem:** a ata diz de que arquivo a reunião veio (`> Importada do arquivo …`), e a Biblioteca também.
- **O mesmo arquivo duas vezes** não vira duas reuniões: o ISPer reconhece o áudio e aponta a reunião que já existe.

> [!TIP/Dica]
> Informar o número de participantes em Configurações → Reuniões também vale para as gravações importadas, e é o que mais ajuda a identificação de falantes numa conversa longa.
