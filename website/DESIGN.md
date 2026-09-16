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

**The Named Speaker Rule.** Speaker color always appears with a textual speaker name. Color never carries identity alone.

## Typography

**Display Font:** Fraunces with Georgia fallback.

**Body Font:** Hanken Grotesk with system-ui fallback.
**Code Font:** Cascadia Mono with JetBrains Mono and Consolas fallbacks.

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

Depth is mainly tonal. Panels move from warm-void to panel and panel-raised; 1px warm borders define structure. Broad soft shadows belong to floating product windows, download focus and the final CTA. The sticky header uses 14px backdrop blur only after scrolling.

## Shapes

Cards and major panels use 12–16px radii. Buttons and fields use 8–12px. Full pills are limited to compact badges, chart switches and speaker labels. Borders and shadows are not stacked unless the shadow communicates a genuinely floating layer.

## Components

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

### Interactive App Stage

The stage reproduces real ISPer states with synthetic content. It exposes dictation and meeting modes, speaker colors, waveform, keyboard shortcut and optional summary boundary without requesting microphone access.

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
