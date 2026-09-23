# ISPer

100% local voice dictation (in the style of Wispr Flow) built with Rust and
whisper.cpp, growing into a notetaker for Microsoft Teams meetings. The full
roadmap is in [ROADMAP.md](ROADMAP.md) (in Portuguese).

**Official site: [isper.pages.dev](https://isper.pages.dev)** —
[download the installer](https://isper.pages.dev/download/) ·
[documentation](https://isper.pages.dev/docs/)

> This is the English translation of [README.md](README.md). The Portuguese
> README is the reference; if the two disagree, it wins.

This README is for people who build ISPer: toolchain, CUDA, tests, releases.
If you only want to use it, the site has the guided install, the checksums to
verify the download and the user guides.

The app itself speaks **Portuguese (Brazil) and English**: Settings → System →
*Interface language* follows Windows by default and switches on the fly,
without a restart. The same tab has *Appearance*: light, dark or follow
Windows.

## Layout

```
crates/isper-core/   # engine: capture (cpal) → resample (rubato) → Whisper (whisper-rs)
crates/isper-cli/    # the Phase 1 lab: transcription in the terminal
crates/isper-models/ # model catalog and download (SHA-256 from Hugging Face)
crates/isper-llm/    # AI providers (Groq, Gemini, Claude) and the post-meeting summary
crates/isper-diarize/# who said what (sherpa-onnx: pyannote + 3D-Speaker)
apps/isper-app/      # Tauri 2 app: src-tauri (Rust) + ui (HTML/CSS/JS, no build step)
  src-tauri/src/     # main.rs (bootstrap) + one module per concern: state,
                     # shortcuts, tray, dictation, meetings, views, overlay,
                     # settings, library, home, notify, updater, config, calls
                     # (Teams call detection), insights (live AI), search
                     # (semantic search), ui (theme), i18n, undo
  ui/assets/         # design system: base.css (tokens, components, motion),
                     # ui.js (toasts with Undo, count-up…), i18n.js and local OFL fonts
  ui/locales/        # interface dictionaries (pt-BR.json, en.json)
scripts/release.ps1  # signed GPU and CPU installers + latest*.json and, with -Publish, the release
tools/e2e/           # end-to-end tests on the real app via CDP (smoke, meeting, updater, theme, i18n…)
CHANGELOG.md         # changes per version; the version's section becomes the release notes
models/              # ggml models (gitignored — download them, see below)
fixtures/            # test WAVs generated with Windows TTS (pt-BR voice "Maria")
```

The interface is vanilla and offline: the fonts (Fraunces and Hanken Grotesk,
OFL license) ship inside the app — nothing is downloaded at runtime.
Animations only use `transform`/`opacity` and honor Windows'
`prefers-reduced-motion` ("Animation effects" off → a static interface).

**Translations.** Every piece of text lives in `apps/isper-app/ui/locales/`,
one JSON file per language, embedded in the binary. Static markup uses
`data-i18n` (text), `data-i18n-html` (text with `<b>`, `<code>`, `<kbd>`) and
`data-i18n-attr="attr:key"`; scripts call `t('key', { vars })`. Plurals are
`{ "one": …, "other": … }` objects resolved with `Intl.PluralRules`. Tests
make sure both languages have the same keys, the same placeholders and the
same plural shape, and that every key a page uses exists. Speaker labels
("Eu", "Participantes", "Participante N") are data and stay in Portuguese in
the database and in the `.md` files; only their display is translated.

## Build prerequisites (Windows)

| Tool | How it was installed | Notes |
|---|---|---|
| Rust (rustup) | `rustup-init.exe -y` | the version comes from [`rust-toolchain.toml`](rust-toolchain.toml) (1.98.0, with rustfmt and clippy): rustup installs it on the first `cargo`; CI uses the same one |
| VS Build Tools 2022 | `winget install Microsoft.VisualStudio.2022.BuildTools` + the VCTools workload | builds whisper.cpp (C++) |
| CMake | portable zip in `%LOCALAPPDATA%\Programs\cmake-*\bin` (on the user PATH) | required by whisper-rs-sys |
| libclang | `pip install --user libclang` | required by bindgen; `LIBCLANG_PATH` points to `%APPDATA%\Python\Python311\site-packages\clang\native` (persisted as a user environment variable) |

## Build

That's it:

```bash
cargo build --release
```

CUDA is **on by default** (`default = ["cuda"]` in `isper-cli` and
`isper-app`); without an NVIDIA GPU, use `--no-default-features`. The
optimization and SIMD flags live in [`.cargo/config.toml`](.cargo/config.toml)
— cargo applies them to the build script automatically, from any shell.

**Why they exist (a critical Windows/MSVC gotcha):** the `cmake` crate
swallows the Release optimization flags — left alone, whisper.cpp is built
**without `/O2`** (the same as `/Od`, ~13× slower). The `whisper-rs-sys`
build.rs forwards any `GGML_*` or `CMAKE_*` environment variable to CMake,
and `.cargo/config.toml` relies on that. If you change those flags, run
`cargo clean -p whisper-rs-sys --release` first; to check the flags in use:
`Select-String -Path target\release\build\whisper-rs-sys-*\output -Pattern 'CL\.exe /c'`.

`GGML_NATIVE` is **off** on purpose: with it on, whisper.cpp targets the CPU
of the machine that builds it. A CI runner with AVX-512 produced a binary that
crashed with an illegal instruction on CPUs without it (0.17.0). The release
build targets AVX2, which every supported x64 CPU has.

### CUDA build (Phase 3)

Requires the CUDA Toolkit (`winget install Nvidia.CUDA`). Then:

```bash
cargo build --release
```

(In a shell opened **before** installing CUDA, export `CUDA_PATH` and
`CUDA_PATH_V13_3` pointing to the toolkit — new shells get them from the
installer. `CMAKE_CUDA_ARCHITECTURES=89` — only the RTX 4050 architecture —
is already in `.cargo/config.toml` and cuts build time A LOT.)

Gotchas we hit:
- `The CUDA Toolkit directory '' does not exist` → MSBuild didn't find
  `CUDA_PATH_V13_3` in the environment (shells opened before the install
  don't have it) — export it as above.
- The CUDA installer may skip `Nvda.Build.CudaTasks.v13.3.dll` — copy (as
  admin) the files from
  `<toolkit>\extras\visual_studio_integration\MSBuildExtensions\` to
  `<BuildTools>\MSBuild\Microsoft\VC\v170\BuildCustomizations\`.
- The CUDA 13 runtime DLLs (`cudart64_13.dll`, `cublas64_13.dll`…) live in
  `<toolkit>\bin\x64` — the installer puts them on the machine PATH, but
  shells opened before the install need the path added by hand (symptom: the
  exe dies instantly with STATUS_DLL_NOT_FOUND, no message).
- Cargo can't replace `isper-app.exe` while the app is running
  (`Access denied`) — quit it from the tray before rebuilding.

With CUDA, the app prefers `models/ggml-large-v3-turbo-q5_0.bin`
automatically. Measured on an RTX 4050: 10.4 s of audio transcribed in 0.6 s
(16× real time).

## Models

ISPer has a **model manager** (the `isper-models` crate): catalog, download
with progress and **SHA-256 checked against the one Hugging Face
publishes**, all in `%LOCALAPPDATA%\com.isper.desktop\models`. With no model
installed, the app opens Settings by itself so you can download one.

```bash
cargo run --release -p isper-cli -- models list
```

```bash
cargo run --release -p isper-cli -- models download ggml-large-v3-turbo-q5_0.bin
```

In the app: Settings → **Whisper models** (download, remove and choose; the
switch is hot). During development, files in `models/` at the repository root
are recognized too.

## Usage

### Dictation app (Phase 2)

```bash
cargo run --release -p isper-app
```

The app lives in the **system tray** and opens with the **Home screen**: a
window with the engine status (loaded model, GPU, active shortcut), the
meeting audio source, diarization and AI, the record-meeting button (with a
timer), a checklist of what is missing or optional, totals and recent
meetings (a click opens the Library on that meeting). It comes back with a
left click on the tray icon or by launching ISPer again; when it starts with
Windows the app stays quietly in the tray. To keep it from opening on a
manual launch, untick "Show this screen when opening ISPer" in the footer (or
in Settings → System).

The global shortcut is Ctrl+Alt+Space, or the first free one among
Ctrl+Shift+Space / Ctrl+Alt+D / Ctrl+Alt+I — the hint in the tray menu shows
which one was registered. Two modes:

- **Push-to-talk**: hold the shortcut, speak, release → the text is pasted
  into the focused app via Ctrl+V (the previous clipboard is restored right
  after).
- **Hands-free**: a quick tap on the shortcut, speak freely → ~1.2 s of
  silence (or a second tap) stops and pastes by itself.

Requires `models/ggml-small.bin` (or the `ISPER_MODEL` environment variable
pointing to another ggml model).

In the tray, **"Settings…"** opens the screen with: the global shortcut
(changed on the fly, no restart — pick from the list or click **"Record
shortcut"** and press the combination you want), the **microphone** (used for
dictation and for the "Me" channel in meetings; if it disconnects, it falls
back to the default), the speech language, the **personal dictionary** (terms
Whisper should spell right — they become the `initial_prompt`), the AI
provider with its key and a connection test, and starting with Windows. Every
dictation also goes to the history (`%APPDATA%\ISPer\isper.db`, table
`dictations`).

**Voice commands** (on by default; Settings → Dictation; spoken in
Portuguese): say *nova linha*, *novo parágrafo*, *ponto final*, *vírgula*,
*ponto de interrogação*, *ponto de exclamação*, *dois pontos*, *ponto e
vírgula*, *reticências*, *abre/fecha parênteses*, *abre/fecha aspas*,
*travessão* or *arroba* and ISPer inserts the symbol and fixes the spacing and
the next capital letter. At the end of a dictation, *apagar isso* discards it
all (nothing is pasted) and *tudo em maiúsculas* / *tudo em minúsculas*
changes the case. It is local text processing, matched on whole words and
ignoring accents.

**Transcription quality**: the engine suppresses non-speech tokens and
filters Whisper's classic hallucinations — "Legendas pela comunidade",
one-word loops, symbol-only chunks and segments the model itself flags as
"not speech" (probability > 0.75). Besides steering the model, the personal
dictionary corrects by similarity what it still gets wrong ("ísper" →
"ISPer", "opt solve" → "OptSolv"), in dictation and in meetings.

**AI polishing (optional)**: in Settings → Intelligence, "Polish dictations
with AI before pasting" removes hesitations, repetitions and fixes
punctuation using the configured provider — "clean-up only", formal or
casual. Only the dictation text is sent; it costs about a second; if the API
fails or there is no key, the original is pasted as usual, and the history
keeps both (an "AI" badge in the Library).

### Meeting notetaker (Phase 4)

In the app: tray → **"Start recording a meeting"**, the Home screen button or
the **global meeting shortcut** (Ctrl+Alt+M by default; configurable). The
tray icon gets a **red dot** while recording. The same paths stop it ("Stop
and transcribe the meeting") — the transcript opens by itself and is saved in
`Documents\ISPer\Reunioes\*.md` + SQLite in `%APPDATA%\ISPer\isper.db`.
Microphone = "Me"; system audio (loopback) = "Participants". Tell the
participants that the meeting is being transcribed (privacy laws such as
LGPD apply).

**On save**: by default ISPer shows a **Windows notification** ("Meeting
saved — title · duration · summary ready"); clicking it opens the Library on
that meeting. Settings → Meetings can switch it to "open the .md file" or
"nothing". When speaker identification finishes, a second, silent
notification arrives. ISPer registers its own name and icon for notifications
in the user registry (`HKCU\Software\Classes\AppUserModelId\com.isper.desktop`)
— so it works even when running the `.exe` without the installer. "Test
notification" in Settings shows a sample.

**Quitting during a meeting** (tray → Quit) stops and saves the meeting
before closing — nothing is lost.

**Live**: the meeting is transcribed in ~6 s blocks while it happens, and
captions show up within a few seconds — a provisional line first, replaced by
the definitive text when each block closes. Speech appears in the meeting
card on the Home screen and in the floating indicator. All local; no audio or
text leaves the machine at this stage. When the meeting stops, a **final
pass** transcribes the whole audio again, more carefully, and assigns each
word to its speaker; the live transcript is available right away and is
replaced when the final pass finishes.

**Automatic title**: with an AI provider configured, the post-meeting summary
comes with a short title about the topic ("Weekly PCP planning" instead of
"Meeting — date"); the `.md` is rewritten with the title and the summary.

**Readable paragraphs**: consecutive lines from the same speaker are grouped
only while the pause between them is under 4 s and the paragraph stays under
60 s — each paragraph keeps its timestamp. This applies to the `.md`, the
DOCX and the Library.

**Library**: tray → "Meeting library…" (or the Home screen button) lists
every meeting with search over titles, summaries and transcripts; each one
opens with the summary, the transcript by speaker, rename, open the `.md` and
delete from history (the file stays). The Dictations tab shows the history of
what you dictated. With semantic search configured (below), the **Semantic**
button finds meetings and dictations by meaning and opens the meeting right
at the passage.

**Deleting with Undo**: deleting a meeting, a dictation or a model takes
effect immediately on screen and shows a toast with **Undo** (or Ctrl+Z) for
7 seconds; only after that does ISPer actually delete. Quitting the app within
that window completes the pending deletions.

**Floating indicator**: drag it wherever you want (the position is
remembered); hover to see `–` (mini mode: just the dot + the meeting timer)
and `×` (hide). The **Indicator** button on the Home screen and the tray item
toggle show/hide: at rest it stays **pinned on screen** ("ready · shortcut")
until you hide it, and it comes back by itself on the next dictation or
meeting. While visible it stays **above every window**, including other
"always on top" ones (Teams in a call, video players): ISPer reasserts that
priority when showing it and every 1.5 s, without stealing focus.

**Live captions**: the indicator's **CC** button (or the "Live captions"
switch on the Home screen, during a meeting) turns the indicator into a wide
bar showing the last two transcribed lines, with the speaker — handy to
follow a meeting without keeping the Home screen open. The same button turns
it back; the preference is remembered.

**Marked moments**: during a meeting, **Ctrl+Alt+K** (configurable), the
indicator's **★** button or "★ Mark moment" on the Home screen mark the
current instant — for "this matters, I want to come back here". Moments
become the "Marked moments" section of the Markdown and the DOCX (with the
ongoing line), clickable chips in the Library that scroll to the highlighted
line, and the AI summary gives those passages priority. Two taps less than
1.5 s apart count as one.

**Searching inside a meeting**: with a meeting open, the "search this
meeting" bar (or Ctrl+F) highlights every match in the summary and the
transcript — ignoring case and accents —, with a counter, Enter/Shift+Enter
to navigate and "Matches only" to see just the lines that contain the term.
If the meeting showed up because of the global search, the term comes
highlighted already.

**Naming participants**: click a speaker's name in the transcript
("Participant 1") and type the real name — it applies to the whole meeting,
the `.md` is rewritten and the speaker's color stays. (Recognizing the same
voice in future meetings comes later.)

**Copy and export**: "Copy summary" and "Copy transcript" (plain text with
timestamps), and **Export SRT** (captions) or **DOCX** (Word) — the file is
written next to the `.md` and shown in Explorer.

**Teams only**: in Settings → Meetings, choose "Microsoft Teams only" — ISPer
uses Windows' *process loopback* and ignores notifications, music and other
apps (if Teams isn't open, it falls back to the whole system and says so).

**Call detected → "Record a transcript?"**: ISPer notices when Teams joins a
call through **Windows audio sessions** (in a call, Teams keeps the
microphone open — no bot, no Teams API, no window scraping) and tells you
with a notification (a click records), a banner on the Home screen and the
indicator; when the call ends while recording, it asks whether to stop. In
Settings → Meetings → "Teams calls" you choose to notify (default), **record
automatically** (and stop by itself when the call ends) or not detect. The
probe runs every 4 s with hysteresis (~8 s to start, ~24 s to end), so a
microphone test or notification sounds don't trigger anything.

**Who said what**: Settings → Meetings → "Download models (~45 MB)" installs
pyannote + 3D-Speaker (via sherpa-onnx, 100% local). The meeting is saved and
opened **right away** with "Participants"; identification runs **in the
background** (on the CPU it takes about 40% of the meeting's length —
sherpa-onnx uses a single thread) and, when it finishes, "Participants"
becomes "Participant 1", "Participant 2"… in the database, the Library and
the `.md`; meanwhile the Home screen shows that the transcript is being
redone. The number of speakers is found by clustering; if it merges or splits
too much, tune the threshold without rebuilding:
`ISPER_DIARIZE_THRESHOLD=0.2` (lower = more distinct speakers; default 0.3).
To tune offline without recording again:
`isper-cli diarize fixtures/duas-vozes-16k.wav`. Quitting ISPer midway
cancels identification for that meeting (the generic labels stay).

In the CLI (lab):

```bash
cargo run --release -p isper-cli -- meeting 30 --source teams
```

```bash
cargo run --release -p isper-cli -- models download-diarize
```

Loopback debugging (delivery rate per second + WAV):

```bash
cargo run --release -p isper-cli --bin loopdump -- 15
```

### Cloud intelligence (Phase 5)

At the end of each meeting, ISPer can generate a **summary, key points,
action items and decisions** through an LLM API — attached to the Markdown
and the database. Configure it once:

```bash
cargo run --release -p isper-cli -- llm use groq
```

```bash
cargo run --release -p isper-cli -- llm set-key groq
```

```bash
cargo run --release -p isper-cli -- llm test
```

Providers: `groq` (free key at console.groq.com/keys), `gemini`
(aistudio.google.com/apikey) and `claude` (console.anthropic.com; uses
`claude-opus-5` by default). Model catalogs change fast and vary by account —
list the ones YOUR key can see with `llm models` (or the "List models" button
in Settings) and pick one with `llm use <provider> --model <id>`. The key is
stored in the **Windows Credential Manager** — never in a file. Privacy: only
the TEXT of the transcript is sent; audio never leaves the machine. With no
provider configured, everything works as usual — just without a summary.

**Live insights** (Settings → Intelligence, opt-in): during a meeting, every
3, 5 or 10 minutes the last ~15 min of transcript go to the provider with four
questions — what is pending, what "Me" promised, what was decided, what
nobody answered — and the previous answer is consolidated instead of starting
over. The "Live insights" panel in the Home screen's meeting card shows the
result and has "Refresh now" (which also works as a one-off round with the
feature off). Rounds with no new speech are skipped so the API isn't wasted.

**Semantic search** (Settings → Intelligence): meetings and dictations become
vectors (embeddings) stored in SQLite next to the text, and the Library gets
the **Semantic** button — "when did we talk about the budget?" finds the
passage even without the exact word. The provider is your choice: **Gemini**
(`gemini-embedding-001`, free tier; reuses the Gemini key) or any
**OpenAI-compatible** endpoint — including a **local Ollama**
(`ollama pull nomic-embed-text` or `bge-m3`; base `http://localhost:11434/v1`,
no key, nothing leaves the machine). Each saved meeting and each pasted
dictation is indexed in the background; "Index everything" covers the older
history and rebuilds the index when the model changes. Vectors from one model
are never compared with another's.

### CLI (Phase 1)

```bash
cargo run --release -p isper-cli -- rec 5
```

```bash
cargo run --release -p isper-cli -- file fixtures/fala-16k.wav
```

Options: `--model <path>` (default `models/ggml-small.bin`),
`--lang <pt|en|auto>`.

If the Library index is ever lost, `isper-cli import --apply` rebuilds it
from the `.md` files in the meetings folder. Without `--apply` it only lists
what it would import; running it again adds nothing.

## Installer and updates

ISPer ships as two per-user NSIS installer variants (no UAC). Since 0.15.0
they are built on GitHub Actions when a tag is pushed
([`release.yml`](.github/workflows/release.yml) — the GPU variant installs
the CUDA Toolkit on the runner), with a CycloneDX SBOM and `SHA256SUMS.txt`
published alongside; [`scripts/release.ps1`](scripts/release.ps1) does the
same on the maintainer's machine, as a fallback. The full process, the
secrets involved and the Authenticode signing path (SignPath Foundation) are
in [`docs/RELEASE.md`](docs/RELEASE.md).

| Variant | File | For | Updates through |
|---|---|---|---|
| GPU (CUDA) | `ISPer_<v>_x64-setup.exe` (~400 MB, CUDA DLLs inside) | NVIDIA GPUs | `latest.json` |
| CPU | `ISPer_<v>_x64-cpu-setup.exe` (~50 MB) | any x64 PC with AVX2 | `latest-cpu.json` |

Both carry the sherpa-onnx DLLs (speaker identification) and the Visual C++
runtime next to the exe, so a clean machine installs and runs it. On the CPU,
prefer the Small or Medium model; Large is slow without a GPU.

```powershell
.\scripts\release.ps1
```

```powershell
.\scripts\release.ps1 -Publish
```

The script reads the version from the app's `Cargo.toml` (single source —
`tauri.conf.json` doesn't repeat it; Tauri reads it from there), requires the
`## [version]` section in [`CHANGELOG.md`](CHANGELOG.md) (which becomes the
release notes), a clean git tree and, with `-Publish`, green CI on the
commit. Then it stops the app, copies the sherpa-onnx DLLs (from
`target/release`) and the Visual C++ ones (from VS Build Tools) into
`resources/`, runs `tauri build` per variant (`--config tauri.gpu.conf.json`
with the CUDA DLLs; `--config tauri.cpu.conf.json --no-default-features` in
`target-cpu/`), signs and leaves everything in `dist\v<version>\`. With
`-Publish`, `gh release create v<version>` uploads the six files; the tag
triggers [`release.yml`](.github/workflows/release.yml), which checks tag ×
manifests × CHANGELOG and runs CI again. The CUDA DLLs (`cudart64_13`,
`cublas64_13`, `cublasLt64_13`, ~500 MB) come from `<CUDA>\bin\x64` and live
in `apps/isper-app/src-tauri/resources/cuda/` (a gitignored folder).

**Automatic updates**: the app checks its variant's manifest at
`https://github.com/Marcus-Boni/ISPer/releases/latest/download/` 45 s after
opening and once a day (Settings → System turns it off). When there is a new
version, the Home screen shows a banner with what's new and a silent toast
lets you know; "Update now" downloads the installer, **verifies the minisign
signature** with the embedded public key (`plugins.updater.pubkey` in
`tauri.conf.json`) and runs it in passive mode — ISPer closes and comes back
on the new version. Nothing is downloaded without a click; updating during a
meeting is refused; a download that doesn't match the signature is
discarded. Validated end to end going from 0.11.0 to 0.11.1 on an installed
app.

**Where things live**: the program in `%LOCALAPPDATA%\Programs\ISPer` (or the
folder you choose in the installer); local data — models and logs — in
`%LOCALAPPDATA%\com.isper.desktop`; settings and the database in
`%APPDATA%\ISPer`; transcripts in `Documents\ISPer\Reunioes`. Up to 0.12.1
local data lived in `%LOCALAPPDATA%\ISPer`, which is exactly Tauri's default
per-user install folder — an installer run by hand mixed program and data;
the app moves the old folder by itself on first launch.

**Signing key**: generated once with
`npx @tauri-apps/cli@^2 signer generate -w %USERPROFILE%\.tauri\isper.key`.
The private key (no password; for one with a password, set
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD` before running the script) stays only in
that folder (outside the repository — `*.key` is in `.gitignore`) and the
public one goes in `tauri.conf.json`. Whoever holds the private key can
publish updates that installed ISPers accept: back it up and don't share it.
If it's lost, generate another and publish a version with the new public key
— people who already have the app reinstall once.

**Code signing policy**: the installer doesn't have an Authenticode
certificate yet, so Windows warns on first run ("More info → Run anyway").
The minisign signature protects the integrity of *updates* and each release's
`SHA256SUMS.txt` lets you verify the download; neither replaces the
certificate. The chosen path is the **SignPath Foundation** program for open
source projects (application in progress; the `sign` job in
[`release.yml`](.github/workflows/release.yml) is ready and skipped until
approval), starting with the CPU variant — the GPU one embeds CUDA's
redistributable DLLs. Once approved, every release is signed in SignPath's
pipeline, from the build made on GitHub's runners and after the maintainer's
manual approval. In the Foundation's terms:

- *Free code signing provided by [SignPath.io](https://signpath.io),
  certificate by [SignPath Foundation](https://signpath.org).*
- **Team** — committers and reviewers: Marcus Boni
  ([@Marcus-Boni](https://github.com/Marcus-Boni)); approvers: Marcus Boni.
  External contributions only come in through a reviewed pull request, with
  the three CI checks green ([CONTRIBUTING.md](CONTRIBUTING.md)).
- **Privacy** — *This program will not transfer any information to other
  networked systems unless specifically requested by the user.* In practice:
  audio and transcripts never leave the machine; the update check queries
  GitHub releases (it can be turned off in Settings → System); only the text
  you ask to summarize goes to the AI provider you configure yourself, and
  only if you configure one. Details in [SECURITY.md](SECURITY.md).

SmartScreen reputation builds up over time, from the certificate; signing
doesn't remove the warning right away.

## Logs and diagnostics

Panics go to the log too, with the message, file:line, thread and backtrace:
the exe has no stderr, so without that a crash would vanish without a trace.

The app writes logs to `%LOCALAPPDATA%\com.isper.desktop\logs\isper.log.<date>`
(one file per day, 14 days kept) as well as stdout. The file is **JSON
Lines**: one object per line with `timestamp`, `level`, `message` and the
event's fields (`audio_secs`, `infer_secs`…), easy to filter:

```powershell
Get-Content "$env:LOCALAPPDATA\com.isper.desktop\logs\isper.log.$(Get-Date -Format yyyy-MM-dd)" | ConvertFrom-Json | Where-Object level -eq WARN
```

Settings → System → **Diagnostics** lists the version, engine, model, CUDA
DLLs, microphones, paths, the database schema version and the **local
metrics** for the last 30 days (dictations and meeting blocks: count,
failures, p50/p95 inference time and real-time factor — stored in the
database, never sent). "**Export diagnostics**" creates a `.zip` in
`Documents\ISPer` with the diagnostics, the versions (ISPer, Tauri, WebView2,
Windows), `config.toml` and `llm.toml` (without keys), the metrics and the
last three logs — lines containing dictated text are removed first. That's
what to send when asking for help; nothing is sent by itself.

**Data** (Settings → System): "Keep meetings and dictations for" sets the
retention — 30, 90, 180 days or 1 year. **The default is forever**: nothing
is deleted unless you choose a period and confirm it on screen. With a
period, ISPer deletes from the database and the Meetings folder whatever is
older, on launch, once a day and when you shorten the period (privacy laws:
keep only what's needed), and before deleting it writes an automatic backup
to `Documents\ISPer\Backups` (the last three are kept) — what was removed is
still recoverable. "Back up the database" writes a consistent copy to
`Documents\ISPer\Backups`, even with the app open; to restore, quit ISPer and
copy the file over `%APPDATA%\ISPer\isper.db`. The database has a versioned
schema (`PRAGMA user_version`): a new version migrates the old database on
open, and a database from a newer version is refused with a warning instead
of being altered.

## Tests and CI

```bash
cargo test --release -p isper-core -p isper-llm -p isper-models -p isper-cli -p isper-app
```

(`--release` reuses the already-built whisper.cpp; in the debug profile
`cargo test` rebuilds whisper.cpp + CUDA from scratch, which takes minutes.)
They are unit and property tests (`proptest`) in the core — silence cutting
of blocks, energy VAD with an injected clock, literal search checked against
SQLite itself —, *golden* tests for the exports (Markdown, SRT and DOCX
compared byte by byte with
[`crates/isper-core/tests/golden/`](crates/isper-core/tests/golden/);
`ISPER_UPDATE_GOLDEN=1 cargo test --release -p isper-core --test golden`
regenerates them), the AI layer with a fake provider (no network), and the
pure logic of the app and the CLI (settings, folder migration, indicator
position, shortcuts, the translation dictionaries, the Undo queue, the CLI
definition). `clippy::unwrap_used` applies to the whole workspace: `unwrap()`
only in tests. The full strategy, with the manual validation script for audio
cases (headset unplugged, sleep, exclusive mode, monitors), is in
[`docs/TESTES.md`](docs/TESTES.md). The transcription pipeline architecture —
live and final modes, VAD, decoding, diarization, speaker attribution and how
to measure all of it — is in
[`docs/transcription-pipeline.md`](docs/transcription-pipeline.md). The
reasons behind the big choices (Rust + Tauri, cloud LLM with text only, the
WASAPI loopback defenses, post-hoc diarization, the two transcription modes,
the no-build UI, CI releases, "nothing disappears unless you ask") are
recorded as architecture decision records in [`docs/adr/`](docs/adr/README.md)
(in Portuguese). The engine's API reference is `cargo doc -p isper-core --open`:
every public item in `isper-core` is documented, and CI fails on a missing doc
or a broken intra-doc link.

GitHub Actions ([`.github/workflows/ci.yml`](.github/workflows/ci.yml)) runs,
on every push and PR, formatting (`cargo fmt --check`), the tests of every
crate (app and CLI without CUDA), `cargo clippy --workspace --all-targets --
-D warnings`, `cargo deny` (vulnerabilities, licenses, sources) and gitleaks.
The Rust version is the one in [`rust-toolchain.toml`](rust-toolchain.toml),
read by CI — a new stable with new lints doesn't break the build by surprise.

**Coverage** ([`coverage.yml`](.github/workflows/coverage.yml)): on every push
to `main`, `cargo llvm-cov` publishes the summary in the job summary and sends
the lcov to [Coveralls](https://coveralls.io) — a trend, not a gate: no PR is
blocked by coverage.

**Network tests** (the Hugging Face API and the checksummed download) are
`#[ignore]` so CI runs offline; after touching the HTTP code, run them by
hand:

```bash
cargo test --release -p isper-models -- --ignored
```

**Change flow**: the `main` branch is protected by a GitHub ruleset — no
direct push, no force-push or deletion, linear history and a mandatory PR
with the three CI checks green. Every change (from the maintainer, a
contributor or Dependabot) comes in through a branch + PR:

```bash
git switch -c my-change
```

```bash
gh pr create --fill
```

```bash
gh pr merge --rebase --delete-branch
```

**End to end**: [`tools/e2e`](tools/e2e/README.md) starts the real app with
the WebView2 debugging port and checks, via CDP, the windows, a meeting with
the two-voice fixture (live, captions, moments, exports, Undo), the updater
against a fake release on localhost, the light and dark themes, the interface
language and memory in a long meeting (`soak.ps1`). The smoke, Undo, theme
and language tests run every night on GitHub Actions
([`e2e-nightly.yml`](.github/workflows/e2e-nightly.yml), a Windows runner
without audio or GPU); the others need a GPU and audio: run them before
shipping a version.

```powershell
.\tools\e2e\smoke.ps1 -Exe .\target\release\isper-app.exe
```

```powershell
.\tools\e2e\soak.ps1 -Minutes 120 -Exe .\target\release\isper-app.exe
```

## Security and contributing

Found a vulnerability? Report it privately through GitHub
(*Security → Report a vulnerability*); scope, timelines and how the app
protects itself are in [SECURITY.md](SECURITY.md). To contribute —
environment, what CI requires, style and the PR flow — see
[CONTRIBUTING.md](CONTRIBUTING.md); the rules of conduct are in the
[Code of Conduct](CODE_OF_CONDUCT.md).

To add a language to the interface, copy `apps/isper-app/ui/locales/en.json`
to `<tag>.json` (for example `es.json`) and translate the values, keeping the
keys and the `{placeholders}`. Then wire it up in
`apps/isper-app/src-tauri/src/i18n.rs` (the `UI_LANGS` list, an
`include_str!` for the file, and the `dict`/`resolve` matches) and add the
option to the *Interface language* selector in `settings.html`. The tests
point out any missing key or placeholder.

## License

[MIT](LICENSE). Whisper (MIT) · whisper.cpp (MIT) · Tauri (MIT/Apache-2.0) —
every model used has open weights.
