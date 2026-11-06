# 0026 — O mascote do assistente é o ponto da marca, vivo, em WebGL com o three.js embutido

- **Status:** aceita
- **Data:** 05/11/2026

## Contexto

O assistente ([0021](0021-assistente-pessoal-no-isper.md)) ganhou nome, tom e
tamanho das respostas; faltava um rosto que mostrasse o que ele está fazendo
(pensando, respondendo, esperando o toque) sem ler o texto. O pedido foi algo
moderno e profissional, na linha dos assistentes do mercado.

Dois caminhos foram testados num protótipo antes de entrar no app:

- **Um bichinho (hamster) montado com formas 3D simples.** Simpático, mas as
  esferas coladas davam ar de amador.
- **O ponto de gravação da marca ganhando vida.** Uma superfície só,
  deformada por ruído num shader, com o gradiente da marca, borda luminosa e
  um rosto mínimo (olhos em pílula). Ouvindo, a superfície ondula com a voz,
  como a onda do logo. Foi o escolhido.

## Decisão

- **O mascote é o ponto da marca** (`apps/isper-app/ui/assets/mascot.js`), com
  sete estados: parado, ouvindo, pensando, respondendo, pede um toque, fechou o
  dia (a única comemoração) e sem conexão. Tudo anima por molas
  interrompíveis; cada estado tem uma intenção legível em 40 px.
- **WebGL pelo three.js 0.170.0, embutido** em `ui/assets/vendor/` (MIT, a
  licença ao lado). A CSP do app só aceita o que vem dele mesmo, e o ISPer
  funciona sem rede. Carrega sob demanda: com o mascote desligado, nem baixa.
- **Mora no cabeçalho do assistente, na tela Hoje**, e segue o que a tela já
  sabe: pensa enquanto o agente trabalha, fala quando a resposta chega, pede o
  toque com o cartão de confirmação, comemora o fechamento do dia e perde a
  cor quando falta a IA. Clicar nele é uma cutucada.
- **Desligável** em Configurações → Assistente; desligado, fica só o texto.

## Consequências

- O app cresce ~680 KB (o three.js). Só a tela Hoje o carrega.
- "Reduzir movimento" do Windows: só a pose muda, e a animação para quando
  nada muda. Fora da tela ou com a janela escondida, não desenha.
- Sem WebGL (driver, área de trabalho remota), o canvas some e o assistente
  segue igual.
- O canvas é decorativo para o leitor de tela (o texto já diz o estado); o
  estado fica em `data-state`, que o e2e `assistant.ps1` confere.

## Alternativas consideradas

- **Rive ou Lottie.** Animação de designer, mas o arquivo nasce num editor à
  parte e cada mudança volta para ele; o shader muda no código e reage à voz.
- **SVG com CSS.** Sem dependência, mas sem volume, luz nem superfície viva:
  o que se pediu foi justamente o acabamento 3D.
- **O hamster.** Ficou no protótipo (`assets/mascote/`, fora do repositório).

## Onde vive

`apps/isper-app/ui/assets/mascot.js`, `ui/assets/vendor/three.module.min.js`,
a montagem em `ui/today.html`, a opção `assistant_mascot` em `config.rs` e
em `assistant_persona`/`assistant_set_persona`.
