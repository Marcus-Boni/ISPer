import type { Metadata } from "next";
import { fontFraunces, fontHanken } from "./fonts";
import { SmoothScrollProvider } from "@/components/motion/smooth-scroll";
import "./globals.css";

export const metadata: Metadata = {
  title: {
    default: "ISPer — Transcrição com IA local e resumo de reuniões",
    template: "%s | ISPer",
  },
  description:
    "Transcrição de áudio por IA direto no seu hardware. 100% privado, offline e sem custos de API.",
  metadataBase: new URL("https://isper.pages.dev"),
};

export default function RootLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <html
      lang="pt-BR"
      className={`${fontFraunces.variable} ${fontHanken.variable} dark`}
    >
      <body className="min-h-screen bg-[var(--bg)] text-[var(--ink)] font-sans antialiased atmos-bg relative selection:bg-[rgba(240,126,114,0.35)] selection:text-[var(--ink)]">
        <div className="noise-overlay" aria-hidden="true" />
        <SmoothScrollProvider>{children}</SmoothScrollProvider>
      </body>
    </html>
  );
}
