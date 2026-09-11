<!-- Obrigado pelo PR. Descreva o que muda e como testar; a lista abaixo é o que o CI e a revisão vão olhar. -->

## O que muda

<!-- Uma ou duas frases: o problema e a solução. Se fecha uma issue: "Fecha #123". -->

## Como testar

<!-- Passos para conferir na mão (ou o teste automatizado que prova a mudança). -->

## Checklist

- [ ] `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --release --no-default-features -- -D warnings` e `cargo test --release --no-default-features -p isper-core -p isper-llm -p isper-models -p isper-cli -p isper-app` passam localmente
- [ ] Comportamento novo ou corrigido vem com teste (unitário, `proptest`, golden ou e2e — ver `docs/TESTES.md`)
- [ ] Nenhum `unwrap()` fora de testes; nenhum segredo, modelo ou binário grande no diff
- [ ] Mudança visível para quem usa está no `CHANGELOG.md`, em `[Unreleased]`
- [ ] Documentação atualizada onde fizer diferença (README, comentários de módulo, `docs/`)
