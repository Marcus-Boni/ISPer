"use client";

import Link from "next/link";
import { useRouter } from "next/navigation";
import { Search, X } from "lucide-react";
import { useCallback, useEffect, useId, useMemo, useRef, useState } from "react";

type SearchItem = { title: string; description: string; section: string; href: string; text: string };
type SearchResult = Pick<SearchItem, "title" | "description" | "section" | "href">;
type PagefindData = { url: string; excerpt: string; meta: { title?: string } };
type PagefindModule = { search: (query: string) => Promise<{ results: Array<{ data: () => Promise<PagefindData> }> }> };

function normalize(value: string) {
  return value.normalize("NFD").replace(/[̀-ͯ]/g, "").toLowerCase();
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

/**
 * Combobox, not a div that happens to contain links.
 *
 * The previous version announced the whole result list on every keystroke — the
 * panel itself was the live region and it held the links — had no way out but
 * deleting the query, and ignored the arrow keys it appeared to offer.
 */
export function DocSearch({ items, inputId = "docs-search" }: { items: SearchItem[]; inputId?: string }) {
  const router = useRouter();
  const listId = useId();
  const [query, setQuery] = useState("");
  const [dismissed, setDismissed] = useState(false);
  const [active, setActive] = useState(-1);
  const [indexed, setIndexed] = useState<{ query: string; results: SearchResult[] } | null>(null);
  const root = useRef<HTMLDivElement>(null);
  const input = useRef<HTMLInputElement>(null);

  const fallback = useMemo(() => localResults(items, query), [items, query]);
  const results = indexed?.query === query.trim() ? indexed.results : fallback;
  const open = !dismissed && query.trim().length >= 2;

  useEffect(() => {
    const value = query.trim();
    if (value.length < 2) return;
    let alive = true;
    const timer = window.setTimeout(() => {
      void searchPagefind(value)
        .then((found) => { if (alive) setIndexed({ query: value, results: found }); })
        .catch(() => {
          // The synchronous local index remains available when Pagefind is not built in dev.
        });
    }, 120);
    return () => { alive = false; window.clearTimeout(timer); };
  }, [query]);

  const close = useCallback(() => { setDismissed(true); setActive(-1); }, []);

  const clear = useCallback(() => {
    setQuery("");
    setActive(-1);
    setDismissed(false);
    input.current?.focus();
  }, []);

  // Pointer outside the combobox closes it, the way every other one behaves.
  useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: PointerEvent) => {
      if (!root.current?.contains(event.target as Node)) close();
    };
    document.addEventListener("pointerdown", onPointerDown);
    return () => document.removeEventListener("pointerdown", onPointerDown);
  }, [open, close]);

  // Ctrl/Cmd+K and "/" reach the search. Two instances are rendered — desktop and
  // mobile — so only the one actually on screen answers.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const shortcut = ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k")
        || (event.key === "/" && !event.ctrlKey && !event.metaKey && !event.altKey);
      if (!shortcut) return;
      const target = event.target as HTMLElement | null;
      if (target && (target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.isContentEditable)) return;
      if (!input.current || input.current.offsetParent === null) return;
      event.preventDefault();
      input.current.focus();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  function onKeyDown(event: React.KeyboardEvent<HTMLInputElement>) {
    if (event.key === "Escape") {
      event.preventDefault();
      if (query) clear();
      else input.current?.blur();
      return;
    }
    if (!open || results.length === 0) return;
    if (event.key === "ArrowDown") {
      event.preventDefault();
      setActive((index) => (index + 1) % results.length);
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      setActive((index) => (index <= 0 ? results.length - 1 : index - 1));
    } else if (event.key === "Home") {
      event.preventDefault();
      setActive(0);
    } else if (event.key === "End") {
      event.preventDefault();
      setActive(results.length - 1);
    } else if (event.key === "Enter" && active >= 0) {
      event.preventDefault();
      const target = results[active];
      if (target) { close(); router.push(target.href); }
    }
  }

  return (
    <div className="doc-search" ref={root}>
      <label className="visually-hidden" htmlFor={inputId}>Buscar na documentação</label>
      <Search aria-hidden="true" className="doc-search-icon" />
      <input
        ref={input}
        id={inputId}
        value={query}
        onChange={(event) => { setQuery(event.target.value); setDismissed(false); setActive(-1); }}
        onKeyDown={onKeyDown}
        placeholder="Buscar guias, CUDA, Teams..."
        autoComplete="off"
        role="combobox"
        aria-expanded={open}
        aria-controls={listId}
        aria-autocomplete="list"
        aria-activedescendant={open && active >= 0 ? `${listId}-${active}` : undefined}
        className="doc-search-input"
      />
      <kbd className="doc-search-hint" aria-hidden="true">Ctrl K</kbd>
      {query ? (
        <button type="button" className="doc-search-clear" onClick={clear} aria-label="Limpar busca">
          <X aria-hidden="true" />
        </button>
      ) : null}

      {/* The count is announced; the results themselves are read by navigating them. */}
      <span className="visually-hidden" role="status">
        {open ? `${results.length} resultado${results.length === 1 ? "" : "s"}` : ""}
      </span>

      {open ? (
        <ul className="doc-search-results" id={listId} role="listbox" aria-label="Resultados da busca">
          {results.length > 0 ? results.map((item, index) => (
            <li key={item.href} id={`${listId}-${index}`} role="option" aria-selected={index === active}>
              <Link href={item.href} className={index === active ? "is-active" : undefined} onClick={close}>
                <span className="result-section">{item.section}</span>
                <span className="result-title">{item.title}</span>
                <span className="result-text">{item.description}</span>
              </Link>
            </li>
          )) : (
            <li className="doc-search-empty">Nenhum guia encontrado para esta busca.</li>
          )}
        </ul>
      ) : null}
    </div>
  );
}
