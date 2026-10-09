# Testes do ISPer — estratégia e roteiro de validação

Como o ISPer é testado, o que cada camada prova e o que ainda depende de uma
pessoa com um fone na mão. Complementa a seção "Testes e CI" do
[README](../README.md#testes-e-ci) e o [`tools/e2e/README.md`](../tools/e2e/README.md).

## Pirâmide

| Camada | Onde | O que prova | Roda |
|---|---|---|---|
| Unitários | `crates/*/src/**`, `apps/isper-app/src-tauri/src/**` (`#[cfg(test)]`) | lógica pura: tarefas, rotinas, checklist do dia, diário e desfazer do assistente (`isper-assist`, sobre um `isper.db` migrado pelo `isper-core`, com relógio parado e fuso explícito), recorrência das rotinas (`isper_assist::recur`), as ações de uma reunião na caixa de entrada sem repetir (`isper_assist::meeting`), os tipos do OptTime, a agenda e o casamento de gravação com evento (`isper_mcp::opttime::match_event`) e o erro de ferramenta do MCP (`isper-mcp`), a extração das ações de "Eu" com o minuto conferido e a reunião longa em pedaços (`isper_llm::meeting_actions`, com o provider falso), a conversa com ferramentas nos três dialetos (`isper_llm::chat`: o que volta intacto, o esquema que o Gemini aceita) e o laço do agente (`isper_llm::agent`, com o `FakeChat`: ler roda, escrever para no toque, bloqueada não roda, limite de rodadas, nova tentativa, citação), as ferramentas do ISPer e a permissão das do OptTime (`isper-agent`), agrupamento de falas, exportações, dicionário e comandos de voz, catálogo de modelos, parsing das APIs de IA, configuração do app (`AppConfig::normalize`), migração de pastas, posição do indicador, atalhos, definição da CLI | `cargo test`; CI a cada push e PR |
| Propriedade (`proptest`) | `store.rs`, `meeting.rs`, `recorder.rs`, `isper-assist` (`when.rs`, `recur.rs`) | invariantes para QUALQUER entrada: a recorrência acha o primeiro dia em que cai sem pular nenhum, a busca é literal (conferida contra o `LIKE … ESCAPE` do próprio SQLite), o corte de bloco cai no silêncio e nunca passa do buffer, o VAD não confunde ruído constante nem estalo com fala e encerra exatamente ao completar o silêncio | idem |
| Golden | `crates/isper-core/tests/golden.rs` + `tests/golden/` | Markdown, SRT e DOCX byte a byte; qualquer mudança de formato aparece como diff no PR (`ISPER_UPDATE_GOLDEN=1 cargo test -p isper-core --test golden` regenera) | idem |
| Integração sem rede | `crates/isper-llm/src/testing.rs` (`FakeProvider`) | resumo, título, polimento e insights do prompt ao pós-processamento, inclusive erros do provider | idem |
| Integração MCP sem rede | `crates/isper-mcp/tests/fake_opttime.rs` | o cliente MCP contra um OptTime de mentira em HTTP de verdade (127.0.0.1): `initialize` do protocolo 2025-06-18, `whoami`, resumo do dia, sugestões com fonte desconhecida, `apply` com a chave de idempotência, erro de ferramenta com código e dica do `_meta`, catálogo com as marcações, token recusado, servidor lento e fora do ar | `cargo test`; CI a cada push e PR |
| Integração com rede | `isper-models` (`#[ignore]`) | API do Hugging Face e download com checksum | à mão: `cargo test --release -p isper-models -- --ignored` |
| Agente num banco sintético | `isper-cli agente semear --db <novo.db>` e `isper-cli agente avaliar crates/isper-cli/testdata/agente-perguntas.jsonl --db <novo.db> --sem-opttime` (`crates/isper-cli/src/agente.rs`) | a chamada de ferramentas de cada provider contra a API de verdade, sem dado real: a resposta traz o fato e cita a fonte do tipo certo, e quanto demora (p50/p95). Em 09/10: Gemini 3.5 Flash-Lite 10/10 (p50 2,7 s), Groq gpt-oss-120b 8/10 no limite de taxa da cota gratuita | à mão, com chave de API |
| Agente em dias reais (critério da 10.4) | `isper-cli agente avaliar <perguntas.jsonl>` sobre o banco do app; as 20 perguntas ficam fora do repositório (têm dado real) | acerta e cita a origem | à mão, com chave **na cota paga** |
| Ações de uma reunião real | `isper-cli reunioes acoes <id>` (`crates/isper-cli/src/reunioes.rs`), sobre o banco (só leitura; com `--db`, um banco reconstruído das atas por `isper-cli import`) | as ações de "Eu" que a caixa de entrada receberia, com o minuto, a frase e o prazo resolvido, e o tempo da chamada | à mão, com chave de API **na cota paga** (a transcrição é da empresa) |
| Conector do OptTime em produção | `isper-cli opttime` com `quem-sou`, `dia`, `sugestoes`, `agenda` e `ferramentas` (`crates/isper-cli/src/opttime.rs`), só leitura, com o token de `opttime.ISPer` | o MCP hospedado responde, o token tem os escopos do preset, a Microsoft e o DevOps estão conectados, o resumo e as sugestões chegam no formato que o ISPer lê | à mão, com o token guardado (`isper-cli llm set-key opttime`) |
| Avaliação da extração de tarefas | `isper-cli tarefas avaliar <corpus.jsonl>` (`crates/isper-cli/src/tarefas.rs`); o corpus fica fora do repositório, em `Documentos\ISPer\Avaliacao-Tarefas` (tem fala de verdade), e nasce de `isper-cli tarefas corpus-ditados --desde AAAA-MM-DD` | precisão, recall e F1 das tarefas (casadas pelo título), dia, hora e prazo certos nas casadas, latência p50/p95 — por provider e modelo; o critério da 10.1 é recall ≥ 0,9, precisão ≥ 0,85 e fala → cartões em menos de 5 s no p95 | à mão, com chave de API: `isper-cli tarefas avaliar corpus.jsonl --provider gemini --model gemini-3.8-flash --saida relatorio.json` |
| Integração com o áudio do Windows | `crates/isper-core/tests/loopback_device.rs` (`#[ignore]`, só Windows) | o loopback abre e entrega áudio mesmo em silêncio, e a detecção de chamada lê as sessões de áudio — o que conferir ao trocar a versão do `wasapi` | à mão: `cargo test --release --no-default-features -p isper-core --test loopback_device -- --ignored` |
| Integração com modelos | `crates/isper-core/tests/pipeline.rs`, `crates/isper-diarize/tests/` (`#[ignore]`; sem os modelos, pulam) | o passe final nas fixtures, a retomada de onde parou e a conferência da voz da diarização (a mesma voz partida em dois volta a ser uma; vozes parecidas continuam separadas) | à mão: `cargo test --release -p isper-core --test pipeline -- --ignored` e `cargo test --release -p isper-diarize -- --ignored` |
| Ponta a ponta | `tools/e2e/*.ps1` (o app real, via CDP; o Android num emulador, via adb) | janelas e indicador, reunião com áudio, exportações, atualizador, memória em reunião longa, dados, tema, desfazer, idioma, primeira configuração, acessibilidade, versão portátil, importação de gravações, tela Hoje, captura de tarefas, rotinas com o conector do OptTime, a agenda com o preparo e o assistente (contra um OptTime de mentira em 127.0.0.1, `fake-opttime.mjs`, e, no assistente, uma IA de mentira compatível com OpenAI, `fake-llm.mjs`); no Android, o laboratório, o gravador (gravar, cair e recuperar, tela apagada) a sincronia com o PC (o `isper-cli` faz o outro lado) e a transcrição no celular (ao carregar, parar e continuar, na hora, falha anotada) | smoke, dados, tema, desfazer, idioma, primeira configuração, acessibilidade e versão portátil toda noite no CI (`e2e-nightly.yml`, sem áudio nem GPU); reunião, importação, atualizador e soak à mão, antes de lançar (precisam de modelo Whisper) |
| Manual | roteiro abaixo | o que precisa de hardware: fone, suspensão, outro app em modo exclusivo, monitores | antes de cada release |

Regras que valem para tudo: `cargo clippy --workspace --all-targets -- -D warnings`
com `clippy::unwrap_used` (o `unwrap()` só existe dentro de testes),
`cargo fmt --check`, `cargo deny` e gitleaks — e o `main` protegido só aceita PR
com os três checks verdes.

No Windows, o binário de teste do `isper-mobile` carrega o
`sherpa-onnx-c-api.dll` ao abrir, e o sistema procura a DLL primeiro na pasta
do executável. Se o teste morrer antes de começar (`0xc0000020` quando acha
uma DLL vazia, `0xc0000135` quando não acha nenhuma), copie as DLLs de
`target/sherpa-onnx-prebuilt/*/lib` para `target/release/deps`, por cima de
alguma antiga que esteja lá. O CI faz isso antes de rodar os testes.

## Cobertura

[`coverage.yml`](../.github/workflows/coverage.yml) roda `cargo llvm-cov` a
cada push em `main`, publica o resumo no sumário do job, guarda o lcov como
artefato e envia ao [Coveralls](https://coveralls.io). É uma **tendência**, não
um gate: nenhum PR é bloqueado por cobertura; a pergunta que ela responde é "o
que entrou este mês veio com testes?". Ativação única: entrar no Coveralls com
a conta do GitHub e adicionar o repositório.

## Soak de 2 h

`tools/e2e/soak.ps1 -Minutes 120` grava uma reunião de duas horas com a fixture
de duas vozes em loop e mede a memória do processo a cada 30 s. O esperado: a
memória privada estabiliza depois do aquecimento (crescimento ≤ 150 MB), o
áudio dos participantes aparece em `%TEMP%\ISPer\*.pcm` durante a reunião e
some quando a diarização termina. Até a 0.13.0 esse áudio ficava em RAM
(`Vec<i16>`, ~115 MB por hora de reunião) — este teste é o que impede a
regressão.

## Roteiro de validação manual (áudio e tela)

Cada caso diz como reproduzir e o que deve acontecer. Anote o resultado
(✅/❌, versão e data) ao rodar antes de uma release.

| # | Caso | Como reproduzir | Comportamento esperado |
|---|---|---|---|
| A1 | Fone USB/Bluetooth desconectado no meio da reunião | Iniciar uma reunião com o fone como microfone; desconectar aos ~1 min; falar por mais 1 min | O log mostra `Eu: sem áudio por 2 s — captura reaberta` e a fala seguinte aparece na transcrição (pelo microfone padrão do sistema). A reunião não cai |
| A2 | Fone desconectado sem outro microfone | Como A1, numa máquina sem microfone interno | Log `não consegui reabrir` no máximo a cada 30 s; o canal dos participantes continua; ao encerrar, a reunião é salva com o que houve |
| A3 | Fone desconectado durante o ditado | Segurar o atalho, desconectar o fone, continuar falando | Em 2 s o ditado encerra sozinho com o que foi capturado e o texto é colado; o log diz `microfone sem áudio por 2 s` |
| A4 | Suspensão/hibernação com reunião ativa | Iniciar reunião, suspender o PC por 2 min, retomar, falar | Loopback (watchdog) e microfone (stall) reabrem; os carimbos de tempo seguem o relógio da reunião; a transcrição continua |
| A5 | Outro app com o dispositivo em modo exclusivo | Em Som → propriedades do dispositivo → Avançado, deixar o controle exclusivo ligado e abrir um player em modo exclusivo (WASAPI exclusivo/ASIO); iniciar reunião | A reunião não inicia e o erro traz a dica `outro aplicativo está usando o dispositivo de áudio em modo exclusivo — …` |
| A6 | Formato do dispositivo não aceito | Forçar 8 bits / 8 kHz nas propriedades do microfone | Erro com a dica de escolher 16 ou 24 bits a 44,1 ou 48 kHz |
| A7 | Fone Bluetooth que demora a começar | Iniciar ditado ou reunião logo depois de conectar o fone | Até 6 s de espera pelo primeiro pacote, sem encerrar por stall |
| T1 | Monitor secundário desligado | Arrastar o indicador para o segundo monitor, fechar o ISPer, desligar o monitor, abrir o ISPer | O indicador aparece no rodapé do monitor principal e o log diz `posição lembrada do indicador está fora dos monitores atuais` |
| T2 | Troca de escala (DPI) | Indicador no canto inferior direito; mudar a escala do Windows de 100 % para 150 %; reabrir | O indicador é empurrado para dentro da tela — nunca fica cortado ou invisível |
| T3 | Indicador entre dois monitores | Deixar metade em cada monitor e reabrir | Fica inteiro no monitor com que mais se sobrepunha |
| D1 | Retenção apaga o que passou do prazo | Com uma reunião antiga na Biblioteca (ou uma data editada no banco), escolher "Guardar reuniões e ditados por" = 30 dias, confirmar no aviso e salvar | Ao escolher o prazo aparece o aviso com "Sim, apagar…" e "Cancelar" (cancelar volta para o valor salvo); depois de salvar, em segundos a reunião some da Biblioteca e o `.md` (e `.srt`/`.docx`, se existirem) sai de `Documentos\ISPer\Reunioes`; antes disso um `isper-antes-da-retencao-<data>.db` apareceu em `Documentos\ISPer\Backups`; o log traz `backup automático antes da retenção` e `retenção aplicada` com as contagens. Reuniões sem data legível não são tocadas; com o padrão "para sempre" nada acontece |
| D2 | Backup e restauração | "Fazer backup do banco"; fechar o ISPer; copiar o arquivo de `Documentos\ISPer\Backups` por cima de `%APPDATA%\ISPer\isper.db`; abrir | O Explorer abre no backup; depois da cópia, a Biblioteca mostra exatamente o que havia no momento do backup; o Diagnóstico mostra o mesmo `schema v2` |
| D3 | Pacote de diagnóstico sem texto ditado | Fazer um ditado; "Exportar diagnóstico (.zip)"; abrir o zip | `logs/isper.log.<hoje>` não contém a frase ditada (a linha virou `[linha com texto ditado removida do diagnóstico]`); `config.toml` e `llm.toml` não têm chave de API; `versoes.txt` traz ISPer, Tauri, WebView2 e Windows |
| D4 | Banco de versão mais nova | Com o app fechado, `PRAGMA user_version = 99` no `isper.db` (DB Browser for SQLite); abrir o ISPer | O app não altera o banco; o log diz `o banco está na versão 99, mais nova do que este ISPer entende`; voltar `user_version` para 2 restaura tudo |

O que os testes automatizados já cobrem desses casos: o mecanismo de stall e
reabertura (`meeting::tests::fonte_que_para_de_entregar_e_reaberta_e_a_reuniao_continua`),
a tradução dos erros (`audio::tests::erros_de_dispositivo_ganham_dica_em_portugues`),
a posição do indicador (`overlay::tests`), as migrações, a retenção, o backup
e as métricas do banco (`store::tests`, inclusive um banco legado criado à
mão e um de versão futura) e a redação do log exportado (`data::tests`). O
e2e `tools/e2e/data.ps1` faz D2 e D3 no app real (backup válido, zip com as
entradas certas e sem texto ditado, log em JSON Lines) e roda toda noite no
CI junto com o smoke. O que só a mão prova é o comportamento do driver de
verdade e a restauração de um backup — por isso o roteiro.

## Acessibilidade — roteiro com o NVDA

O `tools/e2e/a11y.ps1` roda no app real, nos temas escuro e claro, e cobre o
que dá para medir sem ouvir:

- todo controle visível tem nome acessível;
- todo texto passa no contraste WCAG AA;
- nenhuma janela abre rolagem horizontal no tamanho padrão;
- a volta de Tab, com teclado de verdade via CDP, alcança todos os controles
  de Início, Biblioteca, Configurações e primeira configuração, mostra o anel
  em cada foco e não prende;
- as abas da Biblioteca trocam pelas setas;
- Enter abre a reunião e renomeia o falante.

O que ele não prova é como um leitor de tela *fala* cada coisa. Para isso
existe este roteiro, para rodar antes de uma release com o
[NVDA](https://www.nvaccess.org/) (gratuito). Com o NVDA aberto, use só o
teclado: Tab e Shift+Tab, Enter, Espaço, as setas e Esc. `NVDA+T` lê o título
da janela.

| # | Caso | Como reproduzir | O NVDA deve falar |
|---|---|---|---|
| N1 | Início | Abrir o ISPer; Tab pelo cabeçalho, pelos botões de reunião e pelo rodapé | "Indicador, botão de alternância" (pressionado ou não), "Biblioteca, botão", "Configurações, botão"; o interruptor do rodapé como caixa de seleção com o texto dele; cada reunião recente como botão com o título |
| N2 | Primeira configuração | Configurações → Sistema → *Refazer a primeira configuração*; avançar com Enter em *Começar* e *Continuar* | A cada passo, o título ("Boas-vindas ao ISPer", "Fale alguma coisa", …, como título 1). No microfone, ao falar: "Ouvindo você". Nos modelos: "Small, recomendado, botão de opção, marcado"; as setas trocam de modelo. Em *Pular configuração*: o Início abre |
| N3 | Avisos (toasts) | Na Biblioteca, remover um ditado; depois, testar a IA com uma chave inválida | A remoção é lida sem interromper ("Ditado removido do histórico"), e *Desfazer* é alcançável; o erro é lido na hora, interrompendo o que estava sendo falado |
| N4 | Biblioteca | Tab até a lista; Enter numa reunião; Tab até o título e até o nome de um falante; Enter; Esc | Cada reunião como botão (a aberta como "atual"); o título como "renomear a reunião …, botão"; o falante como "renomear o falante Participante 1, botão"; Enter abre "novo nome do falante, editar" e Esc desiste |
| N5 | Abas | Na Biblioteca, Tab até as abas; seta para a direita e para a esquerda | "Reuniões, guia, selecionado, 1 de 2"; a seta leva a "Ditados, guia, selecionado, 2 de 2" e a lista troca |
| N6 | Configurações | Tab por todas as seções; em Reuniões, Enter em *Avançado* | Todo campo e toda lista com o rótulo ("intervalo entre as rodadas de insights, caixa de combinação"); *Avançado* como "recolhido" e, depois do Enter, "expandido", com os controles dele na sequência |
| N7 | Em inglês | Configurações → Aparência e idioma → Idioma da interface = English; repetir N1 e N5 | Tudo em inglês ("Library, button", "Meetings, tab, selected"). Com a troca automática de idioma do NVDA ligada, a voz passa a ler em inglês |
| N8 | Temas de contraste do Windows | Configurações do Windows → Acessibilidade → Temas de contraste (ou Alt+Shift esquerdo+Print Screen); Tab pelo Início e pelas Configurações | O texto segue legível nas cores do tema, e o foco aparece como um contorno ao redor de cada controle (o anel normal do ISPer não sobrevive nesse modo; o contorno o substitui) |

Anote ✅/❌, a versão do NVDA e a do ISPer. Uma falha aqui vira issue com o
que foi falado e o que era esperado.
