import releaseSnapshot from "../../content/data/releases.snapshot.json";

export const siteConfig = {
  name: "ISPer",
  title: "ISPer — Transcrição com IA local para Windows",
  description:
    "Dite em qualquer aplicativo e transcreva reuniões no seu próprio computador. Código aberto, privado e sem mensalidade de transcrição.",
  url: process.env.NEXT_PUBLIC_SITE_URL || "https://isper.pages.dev",
  repository: "https://github.com/Marcus-Boni/ISPer",
  releases: "https://github.com/Marcus-Boni/ISPer/releases",
  latestRelease: "https://github.com/Marcus-Boni/ISPer/releases/latest",
  issues: "https://github.com/Marcus-Boni/ISPer/issues",
  /* Derived, not restated. This used to be a literal, and it was still saying
     v0.15.0 two releases later — the download page and the hero disagreeing
     with the installer they hand out. The release snapshot is the one place a
     version is written down. */
  currentVersion: `v${releaseSnapshot.version}`,
  license: "MIT",
} as const;

/**
 * content/docs/solucao-de-problemas/atalhos.md is the source: Ctrl+Alt+Espaço is
 * the default and Ctrl+Shift+Espaço is the fallback the app tries when the
 * default is already taken. Both places that render it read from here.
 */
export const shortcut = {
  default: ["Ctrl", "Alt", "Espaço"],
  fallback: ["Ctrl", "Shift", "Espaço"],
} as const;

export const navItems = [
  { label: "Recursos", href: "/#recursos" },
  { label: "Benchmarks", href: "/#benchmarks" },
  { label: "Documentação", href: "/docs/" },
  { label: "Download", href: "/download/" },
] as const;
