# Visual review evidence

The portal was inspected in the local static export on 2026-09-16 using the in-app Chromium browser.

- Desktop: 1280 × 720 — landing, download and installation guide.
- Mobile: 390 × 844 — landing and installation guide.
- Verified: primary CTA visibility, app-stage hierarchy, CPU/CUDA selector, MDX callouts, Pagefind results, mobile documentation disclosure and zero page-level horizontal overflow (`viewport: 375`, `scrollWidth: 375`).
- Browser console: no warnings or errors during the tested interactions.

The first Chrome headless attempt exceeded the command wait, but it completed in the background and produced `desktop.png` and `mobile.png`. They record the landing before the final accessibility fixes; regenerate them whenever the visual surface changes materially.
