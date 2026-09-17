import type { Metadata } from "next";
import Link from "next/link";
import { ArrowRight, Code2, ShieldAlert, ShieldCheck } from "lucide-react";
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

/** The dialog Windows shows for an unsigned installer, in the order it appears. */
const smartScreenSteps = [
  "O Windows mostra “O Windows protegeu o computador”.",
  "Clique em “Mais informações”.",
  "Clique em “Executar assim mesmo”.",
];

export default function DownloadPage() {
  const published = currentRelease.publishedAt
    ? new Date(currentRelease.publishedAt).toLocaleDateString("pt-BR")
    : null;

  return (
    <PageTransition>
      <main id="conteudo" className="download-page shell">
        <header className="download-head">
          <h1>Download do ISPer para Windows.</h1>
          <p className="section-lead">
            Escolha a variante que combina com seu hardware. CPU e CUDA são os caminhos verificados no repositório; DirectML/AMD ainda não é anunciado como backend distribuído.
          </p>
          <ul className="hero-proof">
            <li><ShieldCheck aria-hidden="true" />{currentRelease.tag} · canal {currentRelease.channel}</li>
            {published ? <li>Publicada em {published}</li> : null}
            <li><a className="text-link" href={releaseLinks.current}>Ver release no GitHub <ArrowRight aria-hidden="true" /></a></li>
          </ul>
        </header>

        <DownloadSelector assets={downloadVariants} />

        {/* The scariest second of the funnel. The old page named the risk and left
            the reader alone in front of the dialog. */}
        <section className="notice notice-warn" aria-labelledby="smartscreen">
          <h2 id="smartscreen"><ShieldAlert aria-hidden="true" />O Windows vai avisar na primeira execução</h2>
          <p>
            Esta release ainda não tem assinatura Authenticode, então o SmartScreen aparece ao abrir o instalador. Isso é esperado. Confira o SHA-256 acima e siga:
          </p>
          <ol className="notice-steps">
            {smartScreenSteps.map((step) => <li key={step}>{step}</li>)}
          </ol>
          <Link className="text-link" href="/docs/referencia/releases/">Como a release é assinada e publicada <ArrowRight aria-hidden="true" /></Link>
        </section>

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
