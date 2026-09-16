"use client";

import Link from "next/link";
import { useEffect, useState } from "react";
import type { DocHeading } from "@/lib/docs";

export function OnThisPage({ headings }: { headings: DocHeading[] }) {
  const [active, setActive] = useState(headings[0]?.id ?? "");

  useEffect(() => {
    const observers = headings
      .map((heading) => document.getElementById(heading.id))
      .filter((node): node is HTMLElement => Boolean(node));

    const observer = new IntersectionObserver(
      (entries) => {
        const visible = entries.find((entry) => entry.isIntersecting);
        if (visible?.target.id) {
          setActive(visible.target.id);
        }
      },
      { rootMargin: "-18% 0px -70% 0px" },
    );

    for (const node of observers) {
      observer.observe(node);
    }

    return () => observer.disconnect();
  }, [headings]);

  if (headings.length === 0) {
    return null;
  }

  return (
    <nav aria-label="Nesta página" className="hidden xl:block">
      <div className="sticky top-8 max-h-[calc(100vh-4rem)] overflow-auto pl-6">
        <p className="mb-3 text-xs font-semibold uppercase tracking-[0.18em] text-[var(--muted-2)]">Nesta página</p>
        <ol className="space-y-2 text-sm">
          {headings.map((heading) => (
            <li key={heading.id} className={heading.depth === 3 ? "pl-4" : undefined}>
              <Link
                href={`#${heading.id}`}
                className={`block border-l pl-3 leading-relaxed transition ${
                  active === heading.id
                    ? "border-[var(--accent)] text-[var(--ink)]"
                    : "border-[var(--line)] text-[var(--muted)] hover:border-[var(--line-2)] hover:text-[var(--ink-2)]"
                }`}
              >
                {heading.text}
              </Link>
            </li>
          ))}
        </ol>
      </div>
    </nav>
  );
}
