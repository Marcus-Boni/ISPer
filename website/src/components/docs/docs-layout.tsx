import Link from "next/link";
import { ArrowLeft, ArrowRight, BookOpen, ChevronRight, Menu } from "lucide-react";
import type { ReactNode } from "react";
import type { DocNavItem, DocPage } from "@/lib/docs";
import { DocSearch } from "./doc-search";
import { OnThisPage } from "./on-this-page";

type DocsLayoutProps = {
  nav: DocNavItem[];
  searchIndex: Array<{
    title: string;
    description: string;
    section: string;
    href: string;
    text: string;
  }>;
  current?: DocPage;
  previous?: DocPage | null;
  next?: DocPage | null;
  children: ReactNode;
};

function DocumentationNav({ nav, current }: { nav: DocNavItem[]; current?: DocPage }) {
  return (
    <nav aria-label="Documentação">
      {nav.map((group) => (
        <div key={group.section} className="mb-5 last:mb-0">
          <p className="mb-2 text-xs font-semibold uppercase tracking-[0.18em] text-[var(--muted-2)]">{group.section}</p>
          <ol className="space-y-1">
            {group.items.map((item) => {
              const isActive = current?.slug === item.slug;
              return <li key={item.slug}><Link href={item.href} aria-current={isActive ? "page" : undefined} className={`block rounded-lg px-3 py-2 text-sm leading-relaxed transition ${isActive ? "bg-[rgba(240,126,114,0.14)] text-[var(--ink)]" : "text-[var(--muted)] hover:bg-[var(--panel-2)] hover:text-[var(--ink-2)]"}`}>{item.frontmatter.title}</Link></li>;
            })}
          </ol>
        </div>
      ))}
    </nav>
  );
}

export function DocsLayout({ nav, searchIndex, current, previous, next, children }: DocsLayoutProps) {
  return (
    <main id="conteudo" className="docs-shell min-h-screen px-4 sm:px-6 lg:px-8">
      <details className="docs-mobile-nav mx-auto mb-2 max-w-7xl rounded-xl border border-[var(--line)] bg-[var(--panel)] p-4 lg:hidden">
        <summary className="flex cursor-pointer list-none items-center gap-2 font-semibold text-[var(--ink)]"><Menu aria-hidden="true" className="h-4 w-4 text-[var(--accent)]" />Documentação</summary>
        <div className="mt-4"><DocSearch items={searchIndex} inputId="docs-search-mobile" /></div>
        <div className="docs-nav-scroll mt-5 max-h-[62vh]"><DocumentationNav nav={nav} current={current} /></div>
      </details>
      <div className="mx-auto grid max-w-7xl gap-8 lg:grid-cols-[280px_minmax(0,1fr)] xl:grid-cols-[280px_minmax(0,1fr)_240px]">
        <aside className="docs-sidebar hidden lg:flex">
          <DocSearch items={searchIndex} inputId="docs-search-desktop" />
          <div className="docs-sidebar-card">
            <div className="docs-sidebar-title">
              <Menu aria-hidden="true" className="h-4 w-4 text-[var(--accent)]" />
              Documentação
            </div>
            <div className="docs-nav-scroll">
              <DocumentationNav nav={nav} current={current} />
            </div>
          </div>
        </aside>

        <section className="min-w-0">
          <div className="mb-5 flex flex-wrap items-center gap-2 text-sm text-[var(--muted)]">
            <Link href="/docs" className="inline-flex items-center gap-2 hover:text-[var(--ink)]">
              <BookOpen aria-hidden="true" className="h-4 w-4" />
              Docs
            </Link>
            {current ? (
              <>
                <ChevronRight aria-hidden="true" className="h-4 w-4 text-[var(--muted-2)]" />
                <span className="text-[var(--ink-2)]">{current.frontmatter.title}</span>
              </>
            ) : null}
          </div>

          {children}

          {current ? (
            <div className="mt-12 grid gap-4 border-t border-[var(--line)] pt-6 sm:grid-cols-2">
              {previous ? (
                <Link href={previous.href} className="rounded-xl border border-[var(--line)] bg-[var(--panel)] p-4 transition hover:border-[var(--line-2)] hover:bg-[var(--panel-2)]">
                  <span className="mb-2 flex items-center gap-2 text-xs text-[var(--muted)]">
                    <ArrowLeft aria-hidden="true" className="h-4 w-4" />
                    Anterior
                  </span>
                  <span className="font-semibold text-[var(--ink)]">{previous.frontmatter.title}</span>
                </Link>
              ) : (
                <div />
              )}
              {next ? (
                <Link href={next.href} className="rounded-xl border border-[var(--line)] bg-[var(--panel)] p-4 text-right transition hover:border-[var(--line-2)] hover:bg-[var(--panel-2)]">
                  <span className="mb-2 flex items-center justify-end gap-2 text-xs text-[var(--muted)]">
                    Próximo
                    <ArrowRight aria-hidden="true" className="h-4 w-4" />
                  </span>
                  <span className="font-semibold text-[var(--ink)]">{next.frontmatter.title}</span>
                </Link>
              ) : null}
            </div>
          ) : null}
        </section>

        {current ? <OnThisPage headings={current.headings} /> : null}
      </div>
    </main>
  );
}
