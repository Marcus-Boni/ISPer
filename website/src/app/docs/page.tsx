import type { Metadata } from "next";
import Link from "next/link";
import { ArrowRight, BookOpen } from "lucide-react";
import { DocsLayout } from "@/components/docs/docs-layout";
import { getAllDocs, getDocsNav, getDocsSearchIndex } from "@/lib/docs";
import { PageTransition } from "@/components/motion/page-transition";

export const metadata: Metadata = {
  title: "Documentação",
  description:
    "Guias oficiais do ISPer para instalação, ditado, reuniões, falantes, resumos, busca semântica e solução de problemas.",
  alternates: { canonical: "/docs/" },
};

export default function DocsIndexPage() {
  const docs = getAllDocs();
  const nav = getDocsNav();
  const searchIndex = getDocsSearchIndex();
  const sections = nav.map((group) => ({
    section: group.section,
    docs: group.items.map((item) => docs.find((doc) => doc.slug === item.slug)).filter(Boolean),
  }));

  return (
    <PageTransition>
    <DocsLayout nav={nav} searchIndex={searchIndex}>
      <div className="docs-index-head">
        <BookOpen aria-hidden="true" />
        <h1>Documentação oficial do ISPer.</h1>
        <p>Guias versionados para instalar, ditar, gravar reuniões, configurar IA opcional e resolver problemas comuns.</p>
      </div>

      <div className="docs-index-grid">
        {sections.map(({ section, docs: sectionDocs }) => (
          <section key={section} className="docs-index-group">
            <h2>{section}</h2>
            <div className="docs-index-items">
              {sectionDocs.map((doc) =>
                doc ? (
                  <Link key={doc.slug} href={doc.href} className="docs-index-card">
                    <span className="docs-index-title">
                      {doc.frontmatter.title}
                      <ArrowRight aria-hidden="true" />
                    </span>
                    <span className="docs-index-text">{doc.frontmatter.description}</span>
                  </Link>
                ) : null,
              )}
            </div>
          </section>
        ))}
      </div>
    </DocsLayout>
    </PageTransition>
  );
}
