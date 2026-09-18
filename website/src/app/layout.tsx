import type { Metadata, Viewport } from "next";
import { SiteFooter } from "@/components/layout/site-footer";
import { SiteHeader } from "@/components/layout/site-header";
import { SmoothScrollProvider } from "@/components/motion/smooth-scroll";
import { siteConfig } from "@/lib/site";
import { fontFraunces, fontHanken } from "./fonts";
import "./globals.css";

export const metadata: Metadata = {
  metadataBase: new URL(siteConfig.url),
  title: { default: siteConfig.title, template: "%s | ISPer" },
  description: siteConfig.description,
  applicationName: siteConfig.name,
  alternates: { canonical: "/" },
  keywords: ["transcrição local", "Whisper Windows", "ditado por voz", "transcrição de reuniões", "software open source"],
  authors: [{ name: "Marcus Boni", url: siteConfig.repository }],
  creator: "Marcus Boni",
  openGraph: {
    type: "website",
    locale: "pt_BR",
    url: "/",
    title: siteConfig.title,
    description: siteConfig.description,
    siteName: siteConfig.name,
    images: [{ url: "/opengraph-image", width: 1200, height: 630, alt: "ISPer — transcrição local com IA" }],
  },
  twitter: { card: "summary_large_image", title: siteConfig.title, description: siteConfig.description, images: ["/opengraph-image"] },
};

export const viewport: Viewport = { themeColor: "#161311", colorScheme: "dark" };

export default function RootLayout({ children }: Readonly<{ children: React.ReactNode }>) {
  return (
    <html lang="pt-BR" className={`${fontFraunces.variable} ${fontHanken.variable} dark`}>
      <body className="atmos-bg">
        <a className="skip-link" href="#conteudo">Pular para o conteúdo</a>
        <div className="noise-overlay" aria-hidden="true" />
        <SmoothScrollProvider><SiteHeader />{children}<SiteFooter /></SmoothScrollProvider>
      </body>
    </html>
  );
}
