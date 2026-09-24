# Registros de decisão de arquitetura (ADRs)

Cada arquivo aqui registra **uma** decisão que molda o ISPer: o problema, o
que foi escolhido, o que isso custa e o que ficou de fora. O objetivo é que
quem chega ao projeto — inclusive o próprio mantenedor daqui a um ano —
entenda *por que* o código é como é antes de mudá-lo.

| # | Decisão | Status | Data |
|---|---|---|---|
| [0001](0001-registrar-decisoes.md) | Registrar as decisões de arquitetura em ADRs | aceita | 23/09/2026 |
| [0002](0002-rust-tauri-whisper-cpp.md) | Rust + Tauri 2 + whisper.cpp, tudo local | aceita | 25/08/2026 |
| [0003](0003-llm-na-nuvem-so-texto.md) | IA de linguagem na nuvem, só com o texto | aceita | 26/08/2026 |
| [0004](0004-loopback-wasapi-com-keepalive.md) | Loopback pelo WASAPI com keepalive, drenagem e watchdog | aceita | 02/09/2026 |
| [0005](0005-diarizacao-pos-hoc.md) | Diarização depois da reunião, sobre o áudio contínuo | aceita | 01/09/2026 · revista 18/09/2026 |
| [0006](0006-dois-modos-ao-vivo-e-passe-final.md) | Dois modos de transcrição: ao vivo e passe final | aceita | 18/09/2026 |
| [0007](0007-interface-sem-build-step.md) | Interface em HTML/CSS/JS sem build step, com tudo embutido | aceita | 26/08/2026 · ampliada 23/09/2026 |
| [0008](0008-release-em-runners-do-github.md) | Releases compiladas em runners do GitHub, em duas variantes | aceita | 14/09/2026 |
| [0009](0009-nada-some-sem-o-usuario-pedir.md) | Nada some sem o usuário pedir: retenção "para sempre" e Desfazer | aceita | 14/09/2026 · ampliada 23/09/2026 |
| [0010](0010-criptografia-em-repouso-adiada.md) | Criptografia do banco em repouso adiada | aceita | 13/09/2026 |
| [0011](0011-opcional-e-configuracao-nao-feature-flag.md) | O que é opcional vira configuração, não feature flag de compilação | aceita | 23/09/2026 |
| [0012](0012-distribuicao-portatil-e-winget.md) | Distribuição além do instalador: zip portátil e um pacote no winget | aceita | 23/09/2026 |
| [0013](0013-importar-audio-de-fora.md) | Importar áudio de fora: decodificação local, o mesmo passe final e a origem no banco | aceita | 23/09/2026 |

## Como escrever um ADR

1. Copie o modelo abaixo para `NNNN-titulo-curto.md`, com o próximo número.
2. Escreva no passado o que já aconteceu e no presente o que vale agora.
   Números medidos valem mais que adjetivos; aponte o arquivo ou o PR onde a
   decisão vive no código.
3. Um ADR aceito **não é editado para mudar de ideia**: escreva um novo que o
   substitua e marque o antigo como `substituída por NNNN`. Correções de fato
   (um link, um número errado) podem ser feitas no próprio arquivo.

```markdown
# NNNN — Título da decisão

- **Status:** proposta | aceita | substituída por NNNN | descartada
- **Data:** DD/MM/AAAA

## Contexto
O problema, as restrições e o que foi medido.

## Decisão
O que foi escolhido, em uma frase, e os detalhes que importam.

## Consequências
O que fica melhor, o que fica pior e o que passa a ser obrigatório.

## Alternativas consideradas
Cada uma com o motivo de ter ficado de fora.

## Onde vive
Arquivos, módulos, PRs.
```
