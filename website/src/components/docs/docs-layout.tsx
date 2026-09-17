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
        <div key={group.section} className="docs-nav-block">
          <p className="docs-nav-group">{group.section}</p>
          <ol className="docs-nav-items">
            {group.items.map((item) => {
              const isActive = current?.slug === item.slug;
              return <li key={item.slug}><Link href={item.href} aria-current={isActive ? "page" : undefined}>{item.frontmatter.title}</Link></li>;
            })}
          </ol>
        </div>
      ))}
    </nav>
  );
}

export function DocsLayout({ nav, searchIndex, current, previous, next, children }: DocsLayoutProps) {
  return (
    <main id="conteudo" className="docs-shell">
      <details className="docs-mobile-nav">
        <summary><Menu aria-hidden="true" />Documentação<i aria-hidden="true" /></summary>
        <div className="docs-mobile-search"><DocSearch items={searchIndex} inputId="docs-search-mobile" /></div>
        <div className="docs-nav-scroll"><DocumentationNav nav={nav} current={current} /></div>
      </details>
      <div className="docs-grid">
        <aside className="docs-sidebar">
          <DocSearch items={searchIndex} inputId="docs-search-desktop" />
          <div className="docs-sidebar-card">
            <div className="docs-sidebar-title">
              <Menu aria-hidden="true" />
              Documentação
            </div>
            <div className="docs-nav-scroll">
              <DocumentationNav nav={nav} current={current} />
            </div>
          </div>
        </aside>

        <section className="docs-body">
          <nav className="docs-crumbs" aria-label="Trilha">
            <Link href="/docs/">
              <BookOpen aria-hidden="true" />
              Documentação
            </Link>
            {current ? (
              <>
                <ChevronRight aria-hidden="true" />
                <span>{current.frontmatter.section}</span>
                <ChevronRight aria-hidden="true" />
                <span aria-current="page">{current.frontmatter.title}</span>
              </>
            ) : null}
          </nav>

          {current ? <OnThisPage headings={current.headings} variant="inline" /> : null}

          {children}

          {current ? (
            <nav className="docs-pager" aria-label="Navegação entre guias">
              {previous ? (
                <Link href={previous.href} className="docs-pager-link">
                  <span className="docs-pager-kicker"><ArrowLeft aria-hidden="true" />Anterior</span>
                  <span className="docs-pager-title">{previous.frontmatter.title}</span>
                </Link>
              ) : (
                <div />
              )}
              {next ? (
                <Link href={next.href} className="docs-pager-link is-next">
                  <span className="docs-pager-kicker">Próximo<ArrowRight aria-hidden="true" /></span>
                  <span className="docs-pager-title">{next.frontmatter.title}</span>
                </Link>
              ) : null}
            </nav>
          ) : null}
        </section>

        {current ? <OnThisPage headings={current.headings} /> : null}
      </div>
    </main>
  );
}
