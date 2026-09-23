# 0003 — IA de linguagem na nuvem, só com o texto

- **Status:** aceita
- **Data:** 26/08/2026

## Contexto

Depois de transcrever, o notetaker precisa resumir, extrair decisões e
tarefas, dar título à reunião e, mais tarde, polir ditados, gerar insights
ao vivo e alimentar o Copilot. Isso pede um LLM. Rodá-lo na máquina do
usuário competiria com o Whisper pelos 6 GB de VRAM justamente durante as
reuniões, e o usuário não queria pesar a máquina.

A promessa de privacidade do ISPer, porém, é sobre o **áudio**: ele nunca sai
da máquina.

## Decisão

Os recursos de IA de linguagem usam **API de nuvem**, com três regras:

1. **Só texto sai.** O áudio fica na máquina; vai ao provider apenas o
   trecho do transcript necessário para a tarefa.
2. **Opt-in e trocável.** Sem provider configurado, tudo funciona — só sem
   resumo. O provider fica atrás do trait `LlmProvider` (`isper-llm`), com
   Claude, Groq e Gemini, e o modelo é configurável por provider.
3. **Chave no Credential Manager** do Windows, nunca em arquivo; a da Gemini
   vai em header, nunca na URL.

Exceção posterior (10/09): para **embeddings** da busca semântica, além da
Gemini, qualquer endpoint compatível com OpenAI serve — inclusive um
**Ollama local**, que deixa a busca 100% na máquina para quem quiser.

## Consequências

- O app continua leve: a GPU fica inteira para o Whisper.
- Existe custo por uso para quem escolhe um provider pago; Groq e Gemini têm
  camada gratuita, e o padrão é não ter provider.
- Toda chamada de IA pode falhar: o transcript é salvo antes, e a falha do
  resumo, do polimento ou do Copilot nunca derruba a gravação (o polimento
  cola o texto original).
- O trait permite testar prompts com um provider falso, sem rede.
- O que a IA escreve sai no idioma que o prompt pede (pt-BR), não no da
  interface — pendência registrada no ROADMAP.

## Alternativas consideradas

- **LLM local (Ollama/llama.cpp) para tudo.** Disputaria a VRAM com o Whisper
  durante a reunião e deixaria a máquina lenta; continua possível para
  embeddings, onde o custo é pequeno.
- **Um único provider fixo.** Prende o usuário a um fornecedor e a um preço;
  os catálogos de modelos mudam rápido.
- **SDK oficial.** Não há SDK oficial da Anthropic em Rust; o HTTP cru com
  `ureq` mantém as dependências pequenas e o comportamento explícito.

## Onde vive

`crates/isper-llm/` (`providers`, `settings`, `insights`, `copilot`),
`apps/isper-app/src-tauri/src/settings.rs` (chave e teste), Fase 5 do
`ROADMAP.md`.
