import Link from "next/link";
import { siteConfig } from "@/lib/site";

export default function Home() {
  return (
    <main className="min-h-screen flex flex-col items-center justify-center px-6 py-20">
      <div className="w-full max-w-3xl border border-[var(--line)] bg-[var(--panel)]/80 backdrop-blur-md rounded-2xl p-8 sm:p-12 shadow-2xl relative overflow-hidden">
        {/* Glow ambient highlight */}
        <div
          className="absolute -top-24 -right-24 w-60 h-60 rounded-full bg-[var(--accent)]/15 blur-3xl pointer-events-none"
          aria-hidden="true"
        />

        {/* Brand & status badge */}
        <div className="flex items-center justify-between gap-4 mb-8">
          <div className="flex items-center gap-3">
            <span className="font-display text-2xl font-bold tracking-tight text-[var(--ink)]">
              ISPer<span className="text-[var(--accent)]">.</span>
            </span>
            <span className="text-xs font-mono px-2.5 py-0.5 rounded-full border border-[var(--line)] text-[var(--muted)]">
              {siteConfig.currentVersion}
            </span>
          </div>

          <div className="inline-flex items-center gap-2 px-3 py-1 rounded-full border border-[var(--line)] bg-[var(--bg-2)] text-xs text-[var(--muted)]">
            <span className="w-2 h-2 rounded-full bg-[var(--good)] animate-pulse" />
            <span>Boilerplate Pronto</span>
          </div>
        </div>

        {/* Heading */}
        <h1 className="font-display text-3xl sm:text-4xl lg:text-5xl font-semibold tracking-tight text-[var(--ink)] leading-[1.15] mb-6">
          Transcrição com IA. <br />
          <span className="text-[var(--accent)]">Local, privada e sem custos.</span>
        </h1>

        <p className="text-[var(--ink-2)] text-base sm:text-lg leading-relaxed mb-8 max-w-2xl">
          Ambiente base preparado com as melhores práticas de engenharia: Next.js 16 (App Router),
          TypeScript, Tailwind CSS v4, shadcn/ui, Lenis, GSAP, anime.js e export estático para Cloudflare Pages.
        </p>

        {/* Stack badges */}
        <div className="grid grid-cols-2 sm:grid-cols-4 gap-3 mb-10 text-xs font-mono">
          <div className="p-3 rounded-lg border border-[var(--line)] bg-[var(--bg-2)]">
            <span className="text-[var(--muted)] block mb-1">Framework</span>
            <span className="text-[var(--ink)] font-semibold">Next.js 16</span>
          </div>
          <div className="p-3 rounded-lg border border-[var(--line)] bg-[var(--bg-2)]">
            <span className="text-[var(--muted)] block mb-1">Estilização</span>
            <span className="text-[var(--ink)] font-semibold">Tailwind v4</span>
          </div>
          <div className="p-3 rounded-lg border border-[var(--line)] bg-[var(--bg-2)]">
            <span className="text-[var(--muted)] block mb-1">Smooth Scroll</span>
            <span className="text-[var(--ink)] font-semibold">Lenis 1.3</span>
          </div>
          <div className="p-3 rounded-lg border border-[var(--line)] bg-[var(--bg-2)]">
            <span className="text-[var(--muted)] block mb-1">Hospedagem</span>
            <span className="text-[var(--ink)] font-semibold">Cloudflare Pages</span>
          </div>
        </div>

        {/* Action buttons */}
        <div className="flex flex-wrap items-center gap-4">
          <a
            href={siteConfig.links.github}
            target="_blank"
            rel="noopener noreferrer"
            className="inline-flex items-center justify-center px-5 py-2.5 rounded-lg bg-[var(--accent)] hover:bg-[var(--accent-2)] text-[var(--accent-ink)] font-semibold text-sm transition-all shadow-lg hover:shadow-[0_10px_24px_-10px_rgba(240,126,114,0.7)]"
          >
            Ver no GitHub
          </a>
          <div className="text-xs text-[var(--muted)] font-mono">
            Pronto para receber as fases da Landing Page e Documentação.
          </div>
        </div>
      </div>
    </main>
  );
}
