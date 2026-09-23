---
title: "Primeira configuração"
description: "Os cinco passos que o ISPer mostra na primeira vez: idioma e aparência, microfone, modelo, atalho e IA opcional."
section: "Primeiros Passos"
order: 15
---

# Primeira configuração

Numa instalação nova, o ISPer abre com uma janela de cinco passos curtos. Ela deixa o app pronto para o primeiro ditado sem você precisar procurar nada nas Configurações. Quem já usava o ISPer antes da 0.19 não a vê.

## Os cinco passos

| Passo | O que você faz | O que fica configurado |
|---|---|---|
| Boas-vindas | Escolhe o idioma da interface e a aparência | Português ou inglês; tema claro, escuro ou seguir o Windows |
| Microfone | Fala alguma coisa e vê a barra se mexer | O microfone do ditado e do seu lado nas reuniões |
| Modelo | Aceita o modelo recomendado ou escolhe outro | O modelo Whisper, baixado ali mesmo |
| Atalho | Segura o atalho e dita no campo de teste | O atalho global do ditado |
| IA | Conecta um provedor ou escolhe "Agora não" | Resumos, decisões e tarefas ao fim das reuniões |

No fim, um resumo mostra o que ficou pronto e o que ainda falta.

## Microfone

O medidor mostra só o nível do som: nada é gravado. Se a barra não se mexe, o ISPer diz por quê:

- **silêncio absoluto**: o microfone está mudo (botão ou tecla no próprio fone) ou bloqueado em Configurações do Windows → Privacidade → Microfone;
- **sem som nenhum**: ele não está conectado ou não entrega áudio; escolha outro na lista.

Nas reuniões, o áudio dos participantes vem do próprio Windows. Não há nada a configurar para isso.

## Modelo

O ISPer recomenda o modelo de acordo com a versão instalada: o `large-v3-turbo` (quantizado em q5_0) na versão com CUDA, para placa NVIDIA, e o `small` na versão CPU. O download continua sozinho se você avançar. A página [Modelos Whisper](/docs/primeiros-passos/modelos-whisper/) compara os tamanhos.

## IA opcional

Groq e Gemini têm plano gratuito; Claude é pago. Ao colar a chave, **Guardar e testar** confere na hora se ela funciona. A chave fica no Gerenciador de Credenciais do Windows, nunca em arquivo, e só o texto da transcrição vai ao provedor. Sem IA, o ditado e as transcrições funcionam do mesmo jeito. Veja [Escolher um provedor de resumos](/docs/inteligencia-e-resumos/providers/).

## Pular, fechar e refazer

**Pular** ou fechar a janela também contam como concluído. A tela Início assume com um checklist do que ficou faltando. Para passar por tudo de novo, abra Configurações → Sistema → **Refazer a primeira configuração**.

> [!TIP/Dica]
> Tudo o que a primeira configuração ajusta também muda depois nas Configurações. Ela é só o caminho mais curto na primeira vez.
