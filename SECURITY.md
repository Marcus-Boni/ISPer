# Segurança

O ISPer é um app de ditado e um notetaker de reuniões que roda **100 % na sua
máquina**: o áudio nunca sai dela. Este documento diz o que está no escopo de
segurança do projeto, como relatar um problema e como o próprio app se protege.

## Como relatar uma vulnerabilidade

**Não abra uma issue pública.** Use o relato privado do GitHub:

**https://github.com/Marcus-Boni/ISPer/security/advisories/new**

Só o mantenedor lê o relato. Inclua, se puder:

- a versão do ISPer (tela Início ou Configurações → Diagnóstico) e a variante
  (GPU ou CPU);
- o que acontece, como reproduzir e qual seria o impacto;
- logs relevantes (Configurações → Diagnóstico → *abrir pasta de logs*) —
  **retire trechos de transcrição** antes de enviar: eles são dados pessoais
  seus e dos participantes.

Compromisso: resposta inicial em até **7 dias**. Um problema confirmado sai
numa release de correção assim que possível, com o crédito a quem relatou (se
quiser). O projeto é mantido por uma pessoa, no tempo livre — pedimos
paciência com prazos longos.

## Versões com suporte

Só a **última release** publicada em
[Releases](https://github.com/Marcus-Boni/ISPer/releases) recebe correções. O
app avisa quando há versão nova e atualiza com um clique; não há branches de
manutenção para versões antigas.

## O que está no escopo

- O app instalado (`isper-app.exe`) e a CLI (`isper-cli`): captura de áudio,
  transcrição, banco local, exportações, janelas.
- O **atualizador**: baixa o `latest.json` das releases do GitHub e só instala
  um pacote cuja assinatura minisign bata com a chave pública embutida no app
  ([`tauri.conf.json`](apps/isper-app/src-tauri/tauri.conf.json), `plugins.updater.pubkey`).
- A integração opcional com provedores de IA (Claude, Groq, Gemini, Ollama):
  só o **texto** do transcript ou do ditado viaja, e só se você configurar um
  provedor; a chave de API fica no Gerenciador de Credenciais do Windows.

Fora do escopo: vulnerabilidades em dependências que não são exploráveis pelo
ISPer (relate ao projeto upstream — e avise aqui se achar que nos afeta),
problemas que exigem que o atacante já tenha acesso de administrador ou
sessão aberta na máquina, e o comportamento das APIs de IA de terceiros.

## Como o app se protege

- **Local por padrão**: sem provedor de IA configurado, nada sai da máquina.
  Transcrições e ditados ficam em SQLite e Markdown dentro do seu perfil de
  usuário.
- **Segredos fora de arquivos**: chaves de API no Gerenciador de Credenciais
  (crate `keyring`), nunca em `config.toml`, no repositório ou em logs.
- **Atualizações assinadas**: a chave privada do atualizador só existe na
  máquina de quem publica; o app rejeita pacotes cuja assinatura não confere.
- **Janelas com CSP restritiva** (`default-src 'self'`) e comunicação com o
  backend só pelo IPC do Tauri; nenhuma página externa é carregada.
- **Cadeia de suprimentos**: `cargo deny` (vulnerabilidades conhecidas do
  RustSec, licenças e origens das dependências), gitleaks (segredos no
  histórico), Dependabot com atualizações de segurança e toolchain do Rust
  fixo — tudo no CI, exigido para qualquer mudança entrar em `main`.

## O que ainda não temos

O instalador **não é assinado com Authenticode**: na primeira instalação o
SmartScreen do Windows pede confirmação ("Mais informações → Executar assim
mesmo"). A integridade do que você baixa é garantida pela assinatura minisign
do atualizador e pelas somas SHA-256 publicadas com cada release; a assinatura
de código pelo caminho gratuito da SignPath Foundation está no
[ROADMAP](ROADMAP.md) (fase 7.3).

---

**English summary.** Report vulnerabilities privately at
https://github.com/Marcus-Boni/ISPer/security/advisories/new (please do not
open a public issue). Only the latest release is supported. ISPer processes
audio locally; only transcript text is sent to an AI provider, and only if you
configure one. Updates are minisign-signed; the installer is not yet
Authenticode-signed.
