import type { Metadata } from "next";
import Link from "next/link";
import { AlertTriangle, Code2, FileArchive, ShieldCheck } from "lucide-react";
import { DownloadSelector } from "@/components/download/download-selector";
import {
  currentRelease,
  downloadVariants,
  releaseIntegrityNotice,
  releaseLinks,
  sourceInstallSteps,
} from "@/lib/releases";
import { PageTransition } from "@/components/motion/page-transition";

export const metadata: Metadata = {
  title: "Download",
  description:
    "Baixe o instalador Windows x64 do ISPer, escolha CPU ou CUDA e confira requisitos, checksums e notas de release.",
  alternates: { canonical: "/download/" },
};

export default function DownloadPage() {
  return (
    <PageTransition>
    <main id="conteudo" className="mx-auto w-full max-w-7xl px-4 py-12 sm:px-6 lg:px-8">
      <div className="max-w-3xl">
        <p className="font-mono text-sm text-[var(--accent-2)]">{currentRelease.tag}</p>
        <h1 className="mt-3 font-display text-5xl font-semibold leading-tight sm:text-6xl">
          Download do ISPer para Windows.
        </h1>
        <p className="mt-5 text-lg leading-8 text-[var(--ink-2)]">
          Escolha a variante que combina com seu hardware. CPU e CUDA são os caminhos verificados no repositório; DirectML/AMD ainda não é anunciado como backend distribuído.
        </p>
      </div>

      <div className="mt-10 grid gap-8 lg:grid-cols-[1.25fr_0.75fr]">
        <DownloadSelector assets={downloadVariants} />

        <aside className="space-y-4">
          <div className="rounded-2xl border border-[var(--line)] bg-[var(--panel)] p-5">
            <ShieldCheck aria-hidden="true" className="h-6 w-6 text-[var(--good)]" />
            <h2 className="mt-4 text-xl font-semibold">Release estável</h2>
            <p className="mt-3 text-sm leading-6 text-[var(--muted)]">
              Canal {currentRelease.channel}, dados conferidos em{" "}
              {new Date(currentRelease.fetchedAt).toLocaleDateString("pt-BR")}.
            </p>
            <a href={releaseLinks.current} className="mt-4 inline-block text-sm font-semibold text-[var(--accent-2)]">
              Ver release no GitHub
            </a>
          </div>
          <div className="rounded-2xl border border-[var(--line)] bg-[var(--panel)] p-5">
            <AlertTriangle aria-hidden="true" className="h-6 w-6 text-[var(--warn)]" />
            <h2 className="mt-4 text-xl font-semibold">Assinatura e SmartScreen</h2>
            <p className="mt-3 text-sm leading-6 text-[var(--muted)]">
              Esta release ainda não tem assinatura Authenticode. Confira o SHA-256 publicado; o Windows pode exibir o SmartScreen na primeira execução.
            </p>
          </div>
        </aside>
      </div>

      <section className="mt-12 grid gap-5 md:grid-cols-3">
        <article className="rounded-2xl border border-[var(--line)] bg-[var(--panel)] p-6">
          <FileArchive aria-hidden="true" className="h-7 w-7 text-[var(--accent-2)]" />
          <h2 className="mt-4 text-xl font-semibold">Versão portátil</h2>
          <p className="mt-3 leading-7 text-[var(--muted)]">
            O contrato da release ainda não confirma um ZIP portátil executável. O ZIP automático de código fonte do GitHub não substitui esse pacote.
          </p>
        </article>
        <article className="rounded-2xl border border-[var(--line)] bg-[var(--panel)] p-6">
          <Code2 aria-hidden="true" className="h-7 w-7 text-[var(--info)]" />
          <h2 className="mt-4 text-xl font-semibold">Rodar do código fonte</h2>
          <ol className="mt-3 space-y-2 text-sm leading-6 text-[var(--muted)]">
            {sourceInstallSteps.map((step) => (
              <li key={step}>{step}</li>
            ))}
          </ol>
        </article>
        <article className="rounded-2xl border border-[var(--line)] bg-[var(--panel)] p-6">
          <ShieldCheck aria-hidden="true" className="h-7 w-7 text-[var(--good)]" />
          <h2 className="mt-4 text-xl font-semibold">Checklist antes de instalar</h2>
          <p className="mt-3 leading-7 text-[var(--muted)]">{releaseIntegrityNotice}</p>
        </article>
      </section>

      <section className="mt-12 rounded-3xl border border-[var(--line)] bg-[var(--bg-2)] p-6 sm:p-8">
        <h2 className="font-display text-3xl font-semibold">Notas desta release</h2>
        <ul className="mt-5 space-y-3 text-[var(--ink-2)]">
          {currentRelease.notes.map((note) => (
            <li key={note}>{note}</li>
          ))}
        </ul>
        <div className="mt-6">
          <Link href="/docs/primeiros-passos/instalacao/" className="text-sm font-semibold text-[var(--accent-2)]">
            Abrir guia de instalação
          </Link>
        </div>
      </section>
    </main>
    </PageTransition>
  );
}
