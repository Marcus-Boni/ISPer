---
title: "Gravar no celular e mandar para o PC"
description: "O app Android do ISPer grava a reunião; o PC transcreve com o mesmo passe final e devolve a ata para o celular."
section: "Reuniões e Sistema"
order: 47
---

# Gravar no celular e mandar para o PC

Numa reunião presencial, o app Android do ISPer grava em cima da mesa. Com o celular pareado, a gravação vai sozinha para o seu PC, que transcreve com o mesmo passe final de uma reunião gravada nele: identifica quem falou e, se houver IA configurada, faz o resumo. A ata volta para o celular.

> [!NOTE/Observação]
> O celular fala direto com o PC, pela rede local, numa conexão cifrada de ponta a ponta, e só com o PC que você pareou. O áudio não passa por nenhum servidor. Só o texto vai ao provedor de IA, e só se você tiver configurado um.

## Parear, uma vez só

1. **No PC**, abra Configurações → Celular e ligue *Receber as gravações do ISPer no celular*. Na primeira vez, o Windows pergunta se o ISPer pode usar a rede: marque **Redes privadas**.
2. Clique em **Parear um celular**. Aparece um QR, que vale por 2 minutos.
3. **No celular**, abra o ISPer → Biblioteca → **Ler o QR do PC** e aponte a câmera para a tela.
4. O celular pergunta "Parear com *nome do PC*?". Toque em **Parear**.
5. No PC aparece "*nome do celular* quer parear com este PC". Clique em **Permitir**.

Sem câmera ou sem o leitor de QR do Google no celular: no PC, clique em **Copiar o código**, mande o texto para o celular (por uma mensagem para você mesmo, por exemplo) e, no ISPer do celular, use **Colar o código**.

## Depois disso, nada a fazer

- Ao parar uma gravação, ela vai para o PC na primeira oportunidade: na hora, se os dois estiverem na mesma rede, ou quando o celular voltar para perto do PC. O ISPer do PC precisa estar aberto (ele fica na bandeja).
- Na Biblioteca do celular, cada gravação mostra em que pé está: **Vai para o PC**, **Enviando**, **Na fila do PC**, **Transcrevendo no PC**, **Ata pronta**.
- Quando a ata chega, o celular avisa (**Ata pronta**). **Ver a ata** mostra o texto, e **Compartilhar** manda por WhatsApp, e-mail ou Teams.
- No PC, a gravação vira uma reunião na Biblioteca, como uma importada: a data é a da gravação, os momentos marcados no celular (★) viram momentos da reunião, e a origem diz de que aparelho ela veio. O áudio fica em `Documentos\ISPer\Do celular`.

Se a conexão cair no meio, o envio continua de onde parou na próxima vez. Uma gravação que já chegou não vai de novo, nem vira uma segunda reunião.

## Desfazer o pareamento

- **No PC:** Configurações → Celular → **Esquecer**, ao lado do aparelho.
- **No celular:** Biblioteca → **Desconectar**.

As reuniões que já vieram do celular continuam no PC, e as atas continuam no celular.

## Longe do PC

Por padrão, o celular só alcança o PC na mesma rede. Para mandar de qualquer lugar, é preciso um **relay** iroh, de preferência seu: configure o endereço dele no PC, em Configurações → Celular → Avançado. O relay só repassa dados cifrados, que ele não consegue ler. Os celulares já pareados aprendem o relay na próxima vez que conversarem com o PC.

> [!TIP/Dica]
> Se o PC não aparece para o celular: confirme que o ISPer está aberto no PC, que a opção em Configurações → Celular está ligada e que o Windows permitiu o ISPer em **Redes privadas** (Configurações do Windows → Privacidade e segurança → Segurança do Windows → Firewall e proteção de rede → Permitir um aplicativo pelo firewall).
