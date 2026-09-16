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
  currentVersion: "v0.15.0",
  license: "MIT",
} as const;

export const navItems = [
  { label: "Recursos", href: "/#recursos" },
  { label: "Benchmarks", href: "/#benchmarks" },
  { label: "Documentação", href: "/docs/" },
  { label: "Download", href: "/download/" },
] as const;

export const releaseAssets = {
  cpu: "https://github.com/Marcus-Boni/ISPer/releases/download/v0.15.0/ISPer_0.15.0_x64-cpu-setup.exe",
  cuda: "https://github.com/Marcus-Boni/ISPer/releases/download/v0.15.0/ISPer_0.15.0_x64-setup.exe",
} as const;
