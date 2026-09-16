import type { Metadata } from "next";
import { notFound } from "next/navigation";
import { DocsLayout } from "@/components/docs/docs-layout";
import {
  getAdjacentDocs,
  getAllDocs,
  getDocBySegments,
  getDocsNav,
  getDocsSearchIndex,
} from "@/lib/docs";
import { docComponents } from "@/lib/docs-content";

export function generateStaticParams() {
  return getAllDocs().map((doc) => ({ slug: doc.segments }));
}

export async function generateMetadata({
  params,
}: {
  params: Promise<{ slug: string[] }>;
}): Promise<Metadata> {
  const { slug } = await params;
  const doc = getDocBySegments(slug);
  if (!doc) return {};
  return {
    title: doc.frontmatter.title,
    description: doc.frontmatter.description,
    alternates: { canonical: `${doc.href}/` },
  };
}

export default async function DocPage({
  params,
}: {
  params: Promise<{ slug: string[] }>;
}) {
  const { slug } = await params;
  const doc = getDocBySegments(slug);
  if (!doc) notFound();

  const adjacent = getAdjacentDocs(doc.slug);
  const Content = docComponents[doc.slug];
  if (!Content) notFound();

  return (
    <DocsLayout
      nav={getDocsNav()}
      searchIndex={getDocsSearchIndex()}
      current={doc}
      previous={adjacent.previous}
      next={adjacent.next}
    >
      <article className="docs-prose" data-pagefind-body>
        <Content />
      </article>
    </DocsLayout>
  );
}
