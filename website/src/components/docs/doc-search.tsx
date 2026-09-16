"use client";

import Link from "next/link";
import { Search } from "lucide-react";
import { useEffect, useMemo, useState } from "react";

type SearchItem = { title: string; description: string; section: string; href: string; text: string };
type SearchResult = Pick<SearchItem, "title" | "description" | "section" | "href">;
type PagefindData = { url: string; excerpt: string; meta: { title?: string } };
type PagefindModule = { search: (query: string) => Promise<{ results: Array<{ data: () => Promise<PagefindData> }> }> };

function normalize(value: string) {
  return value.normalize("NFD").replace(/[\u0300-\u036f]/g, "").toLowerCase();
}

function localResults(items: SearchItem[], query: string): SearchResult[] {
  const normalized = normalize(query.trim());
  if (normalized.length < 2) return [];
  return items.map((item) => {
    const haystack = normalize(`${item.title} ${item.description} ${item.section} ${item.text}`);
    return { item, score: Number(normalize(item.title).includes(normalized)) * 3 + Number(haystack.includes(normalized)) };
  }).filter((entry) => entry.score > 0).sort((a, b) => b.score - a.score).slice(0, 6).map(({ item }) => item);
}

async function searchPagefind(query: string): Promise<SearchResult[]> {
  const modulePath = "/pagefind/pagefind.js";
  const pagefind = await import(/* webpackIgnore: true */ modulePath) as PagefindModule;
  const response = await pagefind.search(query);
  const data = await Promise.all(response.results.slice(0, 6).map((result) => result.data()));
  return data.map((item) => ({
    title: item.meta.title ?? "Documentação do ISPer",
    description: item.excerpt.replace(/<[^>]+>/g, "").replace(/\s+/g, " ").trim(),
    section: "Documentação",
    href: item.url,
  }));
}

export function DocSearch({ items, inputId = "docs-search" }: { items: SearchItem[]; inputId?: string }) {
  const [query, setQuery] = useState("");
  const fallback = useMemo(() => localResults(items, query), [items, query]);
  const [indexed, setIndexed] = useState<{ query: string; results: SearchResult[] } | null>(null);

  useEffect(() => {
    const value = query.trim();
    if (value.length < 2) return;
    let active = true;
    const timer = window.setTimeout(() => {
      void searchPagefind(value).then((results) => {
        if (active) setIndexed({ query: value, results });
      }).catch(() => {
        // The synchronous local index remains available when Pagefind is not built in dev.
      });
    }, 120);
    return () => { active = false; window.clearTimeout(timer); };
  }, [query]);

  const results = indexed?.query === query.trim() ? indexed.results : fallback;
  return (
    <div className="relative">
      <label className="sr-only" htmlFor={inputId}>Buscar na documentação</label>
      <Search aria-hidden="true" className="pointer-events-none absolute left-3 top-3 h-4 w-4 text-[var(--muted)]" />
      <input id={inputId} value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Buscar guias, CUDA, Teams..." autoComplete="off" className="h-10 w-full rounded-lg border border-[var(--line)] bg-[var(--bg-2)] pl-9 pr-3 text-sm text-[var(--ink)] outline-none transition focus:border-[var(--accent)] focus:ring-2 focus:ring-[rgba(240,126,114,0.22)]" />
      {query.trim().length >= 2 ? (
        <div className="absolute left-0 right-0 top-12 z-30 max-h-96 overflow-y-auto rounded-lg border border-[var(--line)] bg-[var(--panel)] shadow-2xl shadow-black/30" role="status" aria-live="polite">
          {results.length > 0 ? results.map((item) => (
            <Link key={item.href} href={item.href} className="block border-b border-[var(--line)] px-4 py-3 transition hover:bg-[var(--panel-2)] last:border-b-0">
              <span className="block text-xs text-[var(--accent-2)]">{item.section}</span><span className="block text-sm font-semibold text-[var(--ink)]">{item.title}</span><span className="mt-1 block text-xs leading-relaxed text-[var(--muted)]">{item.description}</span>
            </Link>
          )) : <div className="px-4 py-4 text-sm text-[var(--muted)]">Nenhum guia encontrado para esta busca.</div>}
        </div>
      ) : null}
    </div>
  );
}
