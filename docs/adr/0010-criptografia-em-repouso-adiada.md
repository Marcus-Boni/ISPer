# 0010 — Criptografia do banco em repouso adiada

- **Status:** aceita
- **Data:** 13/09/2026

## Contexto

O banco (`%APPDATA%\ISPer\isper.db`) guarda transcrições de reuniões e
ditados — conteúdo potencialmente sensível. A fase 7.4 listava criptografia
em repouso opcional com SQLCipher.

O `rusqlite` só oferece SQLCipher compilando o OpenSSL junto
(`bundled-sqlcipher-vendored-openssl`): Perl no build, minutos a mais no CI a
cada release e, principalmente, **uma chave para guardar e recuperar** —
perdê-la é perder todas as reuniões, contra
[0009](0009-nada-some-sem-o-usuario-pedir.md).

## Decisão

Adiar. O banco continua sem criptografia própria, protegido pelas ACLs do
perfil do usuário no Windows; quem precisa de proteção do disco usa o
BitLocker, que cobre o disco inteiro (inclusive os `.md` e os backups, que um
banco criptografado não protegeria).

## Consequências

- Build e release seguem simples; nenhuma chave nova para o usuário perder.
- Numa máquina compartilhada com a mesma conta, ou com o disco lido fora do
  Windows sem BitLocker, as transcrições ficam legíveis.
- **Volta ao plano** se surgir demanda concreta: máquina compartilhada,
  exigência de compliance ou pedido de usuário. O desenho provável é a chave
  no Credential Manager, com backup obrigatório antes de ativar.

## Alternativas consideradas

- **SQLCipher agora.** Custo alto de build e de risco (chave perdida) para um
  ganho pequeno enquanto o banco vive no perfil do usuário.
- **Criptografar só colunas** (texto das falas) com uma chave do Credential
  Manager. Quebraria a busca por texto no SQLite e a busca semântica.

## Onde vive

Fase 7.4 do `ROADMAP.md` (a decisão está registrada no item).
