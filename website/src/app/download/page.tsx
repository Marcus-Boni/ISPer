import type { Metadata } from "next";
import Link from "next/link";
import { ArrowRight, Code2, ShieldAlert, ShieldCheck } from "lucide-react";
import { DownloadSelector } from "@/components/download/download-selector";
import {
  currentRelease,
  downloadVariants,
  formatReleaseDate,
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

/** The dialog Windows shows for an unsigned installer, in the order it appears. */
const smartScreenSteps = [
  "O Windows mostra “O Windows protegeu o computador”.",
  "Clique em “Mais informações”.",
  "Clique em “Executar assim mesmo”.",
];

export default function DownloadPage() {
  // Every distributed installer has to be signed before the page stops warning.
  const signed = downloadVariants.length > 0 && downloadVariants.every((asset) => asset.authenticodeStatus === "verified");
  const published = currentRelease.publishedAt
    ? formatReleaseDate(currentRelease.publishedAt)
    : null;

  return (
    <PageTransition>
      <main id="conteudo" className="download-page shell">
        {/* Two columns so the fold carries the choice, not just the title. */}
        <header className="download-head">
          <div>
            <h1>Download do ISPer para Windows.</h1>
            <p className="section-lead">
              Escolha a variante que combina com seu hardware. CPU e CUDA são os caminhos verificados no repositório; DirectML/AMD ainda não é anunciado como backend distribuído.
            </p>
          </div>
          <dl className="release-meta">
            <div><dt>Versão</dt><dd>{currentRelease.tag}</dd></div>
            <div><dt>Canal</dt><dd>{currentRelease.channel === "stable" ? "estável" : "prévia"}</dd></div>
            {published ? <div><dt>Publicada</dt><dd>{published}</dd></div> : null}
            <a className="text-link" href={releaseLinks.current}>Ver release no GitHub <ArrowRight aria-hidden="true" /></a>
          </dl>
        </header>

        <DownloadSelector
          assets={downloadVariants}
          notice={
            /* What you are about to see, before "click here" — this used to sit
               below the button that triggers the dialog. Driven by the release
               data, so a signed build stops showing an unsigned build's warning. */
            signed ? (
              <section className="notice" aria-labelledby="smartscreen">
                <h2 id="smartscreen"><ShieldCheck aria-hidden="true" />Instalador assinado</h2>
                <p>Esta release tem assinatura Authenticode, então o Windows não deve exibir o SmartScreen. Conferir o SHA-256 abaixo continua valendo.</p>
                <Link className="text-link" href="/docs/referencia/releases/">Como a release é assinada e publicada <ArrowRight aria-hidden="true" /></Link>
              </section>
            ) : (
              <section className="notice notice-warn" aria-labelledby="smartscreen">
                <h2 id="smartscreen"><ShieldAlert aria-hidden="true" />O Windows vai avisar na primeira execução</h2>
                <p>
                  Esta release ainda não tem assinatura Authenticode, então o SmartScreen aparece ao abrir o instalador. Isso é esperado.
                </p>
                <ol className="notice-steps">
                  {smartScreenSteps.map((step) => <li key={step}>{step}</li>)}
                </ol>
                <Link className="text-link" href="/docs/referencia/releases/">Como a release é assinada e publicada <ArrowRight aria-hidden="true" /></Link>
              </section>
            )
          }
        />

        <div className="download-grid">
          <section className="panel">
            <h2><Code2 aria-hidden="true" />Rodar a partir do código</h2>
            <ol className="ordered-steps">
              {sourceInstallSteps.map((step) => <li key={step}>{step}</li>)}
            </ol>
            <p className="panel-note">
              Não há pacote portátil executável no contrato desta release; o ZIP automático de código fonte do GitHub não substitui um.
            </p>
          </section>

          <section className="panel">
            <h2><ShieldCheck aria-hidden="true" />Notas desta release</h2>
            <ul className="marked-list">
              {currentRelease.notes.map((note) => <li key={note}>{note}</li>)}
            </ul>
            <p className="panel-note">{releaseIntegrityNotice}</p>
            <Link className="text-link" href="/docs/primeiros-passos/instalacao/">Abrir guia de instalação <ArrowRight aria-hidden="true" /></Link>
          </section>
        </div>
      </main>
    </PageTransition>
  );
}
