---
name: ISPer Portal
description: Warm editorial software surfaces for private local transcription.
colors:
  warm-void: "#161311"
  warm-void-raised: "#1b1714"
  panel: "#1e1a18"
  panel-raised: "#26211e"
  panel-active: "#2f2824"
  line: "#372f2b"
  line-strong: "#4a403a"
  ivory: "#ece7e1"
  ivory-soft: "#d3cbc3"
  muted: "#a79e96"
  terracotta: "#f07e72"
  terracotta-light: "#f79f94"
  success: "#84c297"
  warning: "#e8c15a"
  information: "#7cc4f0"
typography:
  display:
    fontFamily: "Fraunces, Georgia, serif"
    fontSize: "clamp(3.4rem, 6vw, 5.2rem)"
    fontWeight: 520
    lineHeight: 0.92
    letterSpacing: "-0.03em"
  headline:
    fontFamily: "Fraunces, Georgia, serif"
    fontSize: "clamp(2.4rem, 6vw, 4.6rem)"
    fontWeight: 700
    lineHeight: 0.98
  body:
    fontFamily: "Hanken Grotesk, system-ui, sans-serif"
    fontSize: "1rem"
    fontWeight: 400
    lineHeight: 1.65
  label:
    fontFamily: "Hanken Grotesk, system-ui, sans-serif"
    fontSize: "0.85rem"
    fontWeight: 650
    lineHeight: 1.2
rounded:
  sm: "8px"
  md: "12px"
  lg: "16px"
  pill: "999px"
spacing:
  xs: "4px"
  sm: "8px"
  md: "16px"
  lg: "32px"
  section: "clamp(4rem, 9vw, 7rem)"
shadows:
  raised: "0 1px 2px rgba(0,0,0,0.32), 0 12px 28px -18px rgba(0,0,0,0.8)"
  floating: "0 2px 6px rgba(0,0,0,0.3), 0 34px 70px -42px rgba(0,0,0,0.92)"
  accent: "0 2px 5px rgba(0,0,0,0.28), 0 18px 34px -22px rgba(240,126,114,0.7)"
motion:
  easeOut: "cubic-bezier(0.16, 1, 0.3, 1)"
  easeSoft: "cubic-bezier(0.32, 0.72, 0, 1)"
  fast: "140ms"
  mid: "260ms"
components:
  button-primary:
    backgroundColor: "{colors.terracotta}"
    textColor: "#1b0f0d"
    rounded: "{rounded.md}"
    padding: "14px 20px"
    height: "52px"
  button-secondary:
    backgroundColor: "{colors.panel}"
    textColor: "{colors.ivory}"
    rounded: "{rounded.md}"
    padding: "14px 20px"
    height: "52px"
  card:
    backgroundColor: "{colors.panel}"
    textColor: "{colors.ivory}"
    rounded: "{rounded.lg}"
    padding: "32px"
---

# Design System: ISPer Portal

## Overview

**Creative North Star: "The Warm Transcript Desk"**

The portal expands the desktop application's dark, warm and editorial identity into a public product surface. It should feel like a private working desk after a meeting: quiet enough to read, exact enough to trust and alive only when an interaction explains the product.

The landing uses Persuade mode with large Fraunces typography and a working ISPer stage. Download uses Operate mode with explicit CPU/CUDA choice and verifiable release data. Documentation uses Read mode with constrained measure, persistent wayfinding and local search.

**Key Characteristics:**

- Warm near-black field with ivory text and rare terracotta emphasis.
- Editorial serif headlines paired with legible sans-serif interfaces.
- Product proof through working interface states, not decorative illustration.
- One orchestrated scroll moment plus restrained state micro-interactions.

## Colors

The palette is dark and warm rather than blue-black. Terracotta marks action and active state; status colors keep their literal meaning.

**The Scarce Coral Rule.** Terracotta is reserved for primary actions, active controls, recording state and the key phrase. Large surfaces remain neutral.

**One name per thing.** The provedor is the only part of the system that can carry text off the machine, so it has one spelling everywhere — "provedor", never "provider". Two names for the load-bearing noun in a privacy claim reads as a draft, and that is the sentence an evaluator screenshots. English identifiers survive only where they name a literal CLI value or app control, inside code formatting.

The Scarce Coral Rule is easiest to break in documentation, where a marker on every bullet and a colour on every inline code token spend it on ordinary prose — so by the time a real control appears the colour no longer signals anything. In the docs, terracotta belongs to the active navigation item and the active contents entry, and nothing else.

**The Named Speaker Rule.** Speaker color always appears with a textual speaker name. Color never carries identity alone.

## Typography

The scale runs display → headline → **section** → title → body → label. The
section step exists because without it every headline arrived at one volume:
six H2s at the same size meant the FAQ shouted as loudly as the closing call,
and nothing on the page could be second-loudest. Supporting sections — the
shortcut detail, the FAQ — take the section step; the thesis, the library, the
benchmarks and the close keep the headline.

**Display Font:** Fraunces with Georgia fallback.

**Body Font:** Hanken Grotesk with system-ui fallback.
**Code Font:** Cascadia Mono with JetBrains Mono and Consolas fallbacks.

Only the sections that carry the argument take the headline step — the thesis, the audited numbers, and the close. Everything else takes the section step. When five of seven headlines shared one size the page had a single volume, and a reader could not tell which section mattered.

Fraunces supplies the product's human, editorial voice. Hanken Grotesk keeps controls and long-form documentation calm. Monospace is reserved for commands, paths, timestamps and checksums.

### Hierarchy

- **Display:** variable 520 weight, up to 5.2rem, compact line-height; landing thesis only.
- **Headline:** 700 weight with balanced wrapping; section transitions and final CTA.
- **Title:** 1.25–2rem; cards and documentation subsections.
- **Body:** 1rem with 1.55–1.8 line-height and a 65–75ch reading measure.
- **Label:** 0.7–0.9rem, semibold; metadata and compact interface states.

## Layout

The page uses a centered shell around 1180px. Landing sections alternate dense demonstrations with quiet explanatory space. Bento items use a six-column desktop grid; documentation uses navigation, article and on-page contents at wide breakpoints.

Below 980px the landing becomes one column. Below 640px primary actions span the available width, the app stage loses perspective, and documentation navigation becomes a collapsed disclosure before the article. No page-level horizontal scrolling is permitted.

## Elevation & Depth

Depth is mainly tonal. Panels move from warm-void to panel and panel-raised; 1px warm borders define structure. Broad soft shadows belong to floating product windows, download focus and the final CTA. The sticky header is transparent at the top of the page and only takes its border, tint and 14px backdrop blur after scrolling.

Every shadow carries an offset as well as a blur; a zero-offset coloured halo is decoration, not elevation. Demonstration surfaces nested inside a card — the typed note, the search result, the command field — are recessed rather than raised, so they read as the application's own chrome instead of a second card.

## Shapes

Radii come from the `rounded` scale only: 16px for cards and major panels, 12px for buttons, fields and inner surfaces, 8px for the smallest controls and focus rings. Buttons and fields use 8–12px. Full pills are limited to compact badges, chart switches and speaker labels. Borders and shadows are not stacked unless the shadow communicates a genuinely floating layer.

## Motion

Lenis owns wheel smoothing on pointer devices and is the single scroll source GSAP's ScrollTrigger listens to; touch and reduced-motion users keep native scrolling. Three registers, and no more:

1. **Arrival** — the landing headline wipes up line by line behind its own mask, and the rest of the hero follows it once.
2. **The settle** — the hero's product window is sticky while the copy scrolls past it, and a single scrubbed timeline squares its perspective as the hero leaves. This is the page's one orchestrated scroll moment. It needs runway: the hero runs past one viewport so the stage has somewhere to hold, and the stage's column stretches to the full row so the sticky child has slack. At exactly one viewport the column was 84px taller than the stage, which is a twitch, not a moment.

   While it holds, the stage plays the two things the page is about to explain — dictation, then a meeting — driven by scroll progress rather than a timer. The motion layer dispatches the state and the component owns its own UI; any deliberate click or keypress inside the stage ends the sequence for good, because a reader operating the demo should not have the page argue with them.
3. **Entrances** — sections differ by role: headings and their supporting line lead, grids stagger their own items, and the shortcut keys press in.
4. **Navigation** — React's `<ViewTransition>` animates route changes. Content travels and the header does not: it is the reader's spatial anchor, so it holds its `view-transition-name` and its animation is suppressed. Going deeper slides left, coming back slides right, and moves between siblings lift in place instead, because a slide would claim a journey that did not happen. Direction is derived from route depth rather than hand-tagged per link, so the same header link reads correctly from every page.

Old content leaves in 150ms so it stops competing for attention; new content arrives over 210ms, delayed until the exit has cleared, while its movement runs the full 420ms.

Motion never hides content it cannot restore. Anything already on screen animates from a visible state, and a failsafe clears every from-state if the scroll layer stops reporting.

## Components

### Download selection

Both installer variants stay on screen with their size and requirements. A 9,8 MB choice and a 422,8 MB choice cannot be compared from memory, and the difference between them is the most decision-relevant fact on the page. Selecting raises a card rather than erasing the other, and carries a check so the state is never held by colour alone.

The page asks the reader to verify what they downloaded, so it shows both halves of that check as numbered steps: the PowerShell command that produces a hash for the file they just saved, and the published hash to compare it against. An instruction to compare a checksum without the command that produces one is an instruction nobody can follow, and this is a Windows-only product, so the command is the Windows one. The signed list opens in its own tab, because it should not replace the value being compared against. The unsigned-binary warning names the SmartScreen dialog and gives the literal steps through it; a warning without a recovery path just leaves the reader stuck at the scariest moment.

### Buttons

- Primary buttons use terracotta, dark text and a soft downward shadow.
- Secondary buttons use a raised neutral surface and strong warm border.
- Hover moves at most one pixel; active state returns toward the surface.
- Focus uses a visible terracotta outline with offset.

### Cards / Containers

- Warm neutral surfaces with one border and 12–16px radii.
- Bento cards may receive a localized glow tied to their subject.
- Nested cards are reserved for real application UI, charts and search results.

### Inputs / Fields

- Near-black field, warm line and 8px radius.
- Focus shifts the border to terracotta and adds a low-opacity ring.
- Search results remain keyboard reachable and use local Pagefind data.

### Navigation

- Desktop navigation remains quiet until hover.
- Mobile navigation uses an explicit menu control.
- Documentation uses a sticky desktop tree and a collapsed mobile disclosure.

Anchors come to rest one header-and-a-half below the top of the viewport, and that distance is declared exactly once, as `scroll-padding-top` on the scrollport. Repeating it per target as `scroll-margin-top` does not reinforce it: the browser and Lenis each subtract *both*, which rested every anchor a full header too low. The scrollport is also the only one of the two that reaches focus and find-in-page.

A cross-route anchor cannot trust a single measurement. The incoming page is still growing when it commits — a dynamic import resolves, a font swaps, the reveal effects release their from-states — and a smooth-scroll library clamps to the document height it measured last, which is still the outgoing page's. So measure, correct, and keep correcting until two consecutive frames agree. Then stop — and stop at once if the reader scrolls, because from that moment the scroll is theirs.

### The audio boundary

The product's irreducible claim is that audio never leaves the machine, and a claim of that weight cannot live only in prose inside a collapsed disclosure. The landing draws it: capture, transcription and storage sit inside a bounded region labelled as the reader's own computer, and the single dashed line that crosses the boundary carries text, only to a provider the reader configured, and is labelled as such.

It is geometry, not illustration — boxes, arrows and a boundary, authored twice so that a horizontal flow is never squeezed into a phone. Both variants are decorative to assistive technology; the figure's caption states the same flow in a sentence.

### Documentation rails

Both rails are sticky columns offset by `--header-h`, so nothing they hold can end up beneath the header. Within a rail only the list scrolls: the search field and the section title stay put, because a reader mid-scroll should not lose the control they were reaching for. A list that genuinely overflows is masked at both edges to say there is more; one that fits is left alone rather than dimmed for symmetry.

Smooth scrolling releases the wheel to any nested scroller that still has room, and those scrollers contain their own overscroll so reaching the end of a list does not carry on into the page.

Heading anchors are slugged with the same implementation that renders the ids. Two slug functions for one document is a broken link waiting to happen, and in Portuguese it is most of them.

### Interactive App Stage

The waveform is centred on its own midline and scales about that axis, so it opens symmetrically the way an audio meter does. At rest it collapses toward the line — an honest "not listening" state — and the resting height is a CSS transition the animation library hands back to when recording stops.

The stage reproduces real ISPer states with synthetic content, and the states have to be true to themselves: the meeting transcript fills and then holds rather than wrapping, because a recording clock that counts back to zero while the chip still reads "gravando reunião" tells the reader the whole thing is theatre — on a page whose thesis is that its numbers can be audited. It exposes dictation and meeting modes, speaker colors, waveform, keyboard shortcut and optional summary boundary without requesting microphone access.

## Do's and Don'ts

### Do:

- **Do** use product data and actual interface states as the primary visual material.
- **Do** label synthetic transcripts, illustrative costs and unsupported features.
- **Do** preserve native scrolling and static content when motion is reduced.
- **Do** keep download integrity, version and hardware choice visible together.

### Don't:

- **Don't** describe optional cloud summaries as fully local.
- **Don't** turn every section into equal icon cards or add decorative glass layers.
- **Don't** use gradients on text or monospace as a generic technical costume.
- **Don't** hide article content behind the documentation navigation on mobile.
