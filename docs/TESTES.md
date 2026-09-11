# Testes do ISPer — estratégia e roteiro de validação

Como o ISPer é testado, o que cada camada prova e o que ainda depende de uma
pessoa com um fone na mão. Complementa a seção "Testes e CI" do
[README](../README.md#testes-e-ci) e o [`tools/e2e/README.md`](../tools/e2e/README.md).

## Pirâmide

| Camada | Onde | O que prova | Roda |
|---|---|---|---|
| Unitários | `crates/*/src/**`, `apps/isper-app/src-tauri/src/**` (`#[cfg(test)]`) | lógica pura: agrupamento de falas, exportações, dicionário e comandos de voz, catálogo de modelos, parsing das APIs de IA, configuração do app (`AppConfig::normalize`), migração de pastas, posição do indicador, atalhos, definição da CLI | `cargo test`; CI a cada push e PR |
| Propriedade (`proptest`) | `store.rs`, `meeting.rs`, `recorder.rs` | invariantes para QUALQUER entrada: a busca é literal (conferida contra o `LIKE … ESCAPE` do próprio SQLite), o corte de bloco cai no silêncio e nunca passa do buffer, o VAD não confunde ruído constante nem estalo com fala e encerra exatamente ao completar o silêncio | idem |
| Golden | `crates/isper-core/tests/golden.rs` + `tests/golden/` | Markdown, SRT e DOCX byte a byte; qualquer mudança de formato aparece como diff no PR (`ISPER_UPDATE_GOLDEN=1 cargo test -p isper-core --test golden` regenera) | idem |
| Integração sem rede | `crates/isper-llm/src/testing.rs` (`FakeProvider`) | resumo, título, polimento e insights do prompt ao pós-processamento, inclusive erros do provider | idem |
| Integração com rede | `isper-models` (`#[ignore]`) | API do Hugging Face e download com checksum | à mão: `cargo test --release -p isper-models -- --ignored` |
| Ponta a ponta | `tools/e2e/*.ps1` (o app real, via CDP) | janelas e indicador, reunião com áudio, exportações, atualizador, memória em reunião longa | `smoke.ps1` toda noite no CI (`e2e-nightly.yml`); os demais à mão |
| Manual | roteiro abaixo | o que precisa de hardware: fone, suspensão, outro app em modo exclusivo, monitores | antes de cada release |

Regras que valem para tudo: `cargo clippy --workspace --all-targets -- -D warnings`
com `clippy::unwrap_used` (o `unwrap()` só existe dentro de testes),
`cargo fmt --check`, `cargo deny` e gitleaks — e o `main` protegido só aceita PR
com os três checks verdes.

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

O que os testes automatizados já cobrem desses casos: o mecanismo de stall e
reabertura (`meeting::tests::fonte_que_para_de_entregar_e_reaberta_e_a_reuniao_continua`),
a tradução dos erros (`audio::tests::erros_de_dispositivo_ganham_dica_em_portugues`)
e a posição do indicador (`overlay::tests`). O que só a mão prova é o
comportamento do driver de verdade — por isso o roteiro.
