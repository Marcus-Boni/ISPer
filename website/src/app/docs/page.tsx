import type { Metadata } from "next";
import Link from "next/link";
import { ArrowRight, BookOpen } from "lucide-react";
import { DocsLayout } from "@/components/docs/docs-layout";
import { getAllDocs, getDocsNav, getDocsSearchIndex } from "@/lib/docs";

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
    <DocsLayout nav={nav} searchIndex={searchIndex}>
      <div className="max-w-3xl">
        <BookOpen aria-hidden="true" className="h-8 w-8 text-[var(--accent)]" />
        <h1 className="mt-5 font-display text-4xl font-semibold leading-tight text-[var(--ink)] sm:text-5xl">
          Documentação oficial do ISPer.
        </h1>
        <p className="mt-5 text-lg leading-8 text-[var(--ink-2)]">
          Guias versionados para instalar, ditar, gravar reuniões, configurar IA opcional e resolver problemas comuns.
        </p>
      </div>

      <div className="mt-10 grid gap-5 lg:grid-cols-2">
        {sections.map(({ section, docs: sectionDocs }) => (
          <section key={section} className="rounded-2xl border border-[var(--line)] bg-[var(--panel)] p-5">
            <h2 className="font-display text-2xl font-semibold text-[var(--ink)]">{section}</h2>
            <div className="mt-4 space-y-3">
              {sectionDocs.map((doc) =>
                doc ? (
                  <Link
                    key={doc.slug}
                    href={doc.href}
                    className="group block rounded-xl border border-[var(--line)] bg-[var(--panel-2)] p-4 transition hover:border-[var(--accent)]"
                  >
                    <span className="flex items-center justify-between gap-4 font-semibold text-[var(--ink)]">
                      {doc.frontmatter.title}
                      <ArrowRight aria-hidden="true" className="h-4 w-4 text-[var(--muted)] group-hover:text-[var(--accent)]" />
                    </span>
                    <span className="mt-2 block text-sm leading-6 text-[var(--muted)]">
                      {doc.frontmatter.description}
                    </span>
                  </Link>
                ) : null,
              )}
            </div>
          </section>
        ))}
      </div>
    </DocsLayout>
  );
}
