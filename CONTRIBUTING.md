# Como contribuir

Obrigado pelo interesse. O ISPer é um projeto pessoal, mantido no tempo livre,
que também serve para aprender Rust — então contribuições pequenas, focadas e
bem explicadas são as que mais ajudam. Este guia diz como montar o ambiente,
o que o CI exige e como uma mudança entra em `main`.

## Antes de começar

- **Bugs e ideias**: abra uma issue com o modelo certo (há um para bug e um
  para melhoria). Para algo grande, descreva a ideia numa issue antes de
  codificar — evita trabalho que não vai ser aceito.
- **Segurança**: nunca em issue pública. Veja [SECURITY.md](SECURITY.md).
- **Conduta**: valem as regras do [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).

## Ambiente

Windows 10/11 x64. O [README](README.md#pré-requisitos-de-build-windows)
lista os pré-requisitos (VS Build Tools, CMake, libclang). A versão do Rust é
a de [`rust-toolchain.toml`](rust-toolchain.toml) — o `rustup` a instala
sozinho na primeira compilação.

Sem GPU NVIDIA, compile sem CUDA (é o que o CI faz):

```bash
cargo build --release --no-default-features
```

Use sempre `--release`: no perfil debug o `cargo build` recompila o whisper.cpp
do zero e leva vários minutos. O app precisa de um modelo Whisper; a tela
Início oferece o download.

## O que o CI exige

Toda mudança passa por estes quatro comandos, e o `main` só aceita PR com os
checks verdes:

```bash
cargo fmt --all -- --check
```

```bash
cargo clippy --workspace --all-targets --release --no-default-features -- -D warnings
```

```bash
cargo test --release --no-default-features -p isper-core -p isper-llm -p isper-models -p isper-cli -p isper-app
```

```bash
cargo deny check
```

Regras que o clippy cobra e vale conhecer antes:

- `unwrap()` só dentro de testes (`clippy::unwrap_used`). Em produção use `?`,
  um fallback ou `expect` com o motivo. Para `Mutex`, o app tem
  `lock_or_recover()`.
- `dbg!`, `todo!` e `unimplemented!` não entram em `main`.
- `#[cfg(test)] mod tests` é o último item do arquivo.

## Testes

Mudou comportamento? Vem com teste. A estratégia completa está em
[`docs/TESTES.md`](docs/TESTES.md); em resumo:

- lógica pura → teste unitário no próprio módulo (Portuguese naming, como os
  existentes: `fn corte_cai_no_silencio()`);
- invariantes para qualquer entrada → `proptest`;
- formato de exportação → os testes *golden* em
  `crates/isper-core/tests/golden/` (`ISPER_UPDATE_GOLDEN=1 cargo test
  --release -p isper-core --test golden` regenera; revise o diff);
- camada de IA → o `FakeProvider` de `crates/isper-llm/src/testing.rs`, sem
  rede;
- janelas, áudio e atualizador → os roteiros ponta a ponta em
  [`tools/e2e`](tools/e2e/README.md), que rodam o app real.

## Estilo do código e das mensagens

- Comentários e documentação em **português**; nomes de identificadores em
  inglês, como o restante do código. Explique o *porquê* (a pegadinha, a
  decisão), não o *o quê*.
- Edição 2024, `rustfmt.toml` do repositório, sem `style=""` inline nos HTML
  (a CSP bloqueia).
- Commits pequenos, um assunto por commit, mensagem em ASCII com prefixo:
  `feat:`, `fix:`, `refactor:`, `test:`, `docs:`, `ci:`, `deps:`, `chore:`.
  O corpo explica a motivação.
- Nada de segredos, modelos (`models/`) ou binários grandes no repositório.

## Fluxo de mudança

`main` é protegida: sem push direto, histórico linear, PR obrigatório com os
três checks do CI. Passo a passo:

```bash
git switch -c minha-mudanca
```

Faça os commits, rode os quatro comandos acima e abra o PR:

```bash
gh pr create --fill
```

Descreva **o que muda e como testar**; o modelo de PR do repositório tem a
lista. Uma mudança visível para quem usa entra no `CHANGELOG.md`, na seção
`[Unreleased]`. O merge é por *rebase* (ou *squash* para uma série de commits
de rascunho), sem commit de merge.

## Onde as coisas moram

| Pasta | O que é |
|---|---|
| `crates/isper-core` | motor: áudio, VAD, reunião (mic + loopback), banco SQLite, exportações, embeddings |
| `crates/isper-llm` | providers de IA, resumo, polimento, insights, embeddings |
| `crates/isper-models` | catálogo e download dos modelos Whisper |
| `crates/isper-diarize` | quem falou o quê (sherpa-onnx) |
| `crates/isper-cli` | laboratório do motor no terminal |
| `apps/isper-app/src-tauri` | o app (Tauri 2), um módulo por responsabilidade |
| `apps/isper-app/ui` | as janelas: HTML/CSS/JS sem build step |
| `tools/e2e` | testes ponta a ponta no app real |
| `scripts/release.ps1` | instaladores e release |

O [ROADMAP](ROADMAP.md) mostra o que já foi feito e o que vem a seguir — é o
lugar certo para ver se a sua ideia já está planejada.
