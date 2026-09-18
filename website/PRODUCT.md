# ISPer Portal

## Product truth

ISPer is an open-source Windows desktop application for local Whisper transcription, global voice dictation and meeting capture. Transcription and speaker diarization run locally. Optional summaries use Groq, Gemini or Claude and send transcript text only when the user configures a provider. Semantic search can use Gemini or an OpenAI-compatible endpoint, including local Ollama.

## Audiences

- Professionals who need reliable dictation in any Windows application.
- Teams that record meetings but cannot send confidential audio to a transcription SaaS.
- Technical users evaluating CPU/CUDA requirements, source builds and privacy boundaries.

## Surface modes

- `/`: **Persuade** — make the local mechanism understandable and earn a Windows download.
- `/download`: **Operate** — select CPU or CUDA, verify integrity and download the correct installer.
- `/docs`: **Read** — find an answer and complete a setup or troubleshooting task.

## Primary outcomes

1. A new visitor understands that audio transcription is local and knows that optional cloud AI handles text separately.
2. A Windows user selects the correct installer without assuming AMD/DirectML or portable ZIP support.
3. A user completes installation, first dictation, meeting capture or troubleshooting from versioned documentation.

## Boundaries

- The portal never records audio, invokes local desktop endpoints or accepts API keys.
- DirectML, portable ZIP and local Ollama summaries are not advertised as shipped features.
- GitHub Releases is the canonical binary source; Cloudflare Pages serves only the static portal.
- Product claims must remain traceable to the repository, published release or labeled demonstration data.
