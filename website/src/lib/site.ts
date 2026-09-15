export const siteConfig = {
  name: "ISPer",
  title: "ISPer — Transcrição com IA local e resumo de reuniões",
  description:
    "Transcrição de áudio por IA direto no seu hardware. 100% privado, offline e sem custos de API.",
  url: process.env.NEXT_PUBLIC_SITE_URL || "https://isper.pages.dev",
  ogImage: "https://isper.pages.dev/og/isper-card.png",
  links: {
    github: "https://github.com/Marcus-Boni/ISPer",
    docs: "/docs",
    download: "/download",
  },
  currentVersion: "v0.15.0",
};
