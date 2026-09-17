"use client";

import Link from "next/link";
import { useEffect, useState } from "react";
import type { DocHeading } from "@/lib/docs";

export function OnThisPage({ headings }: { headings: DocHeading[] }) {
  const [active, setActive] = useState(headings[0]?.id ?? "");

  useEffect(() => {
    const nodes = headings
      .map((heading) => document.getElementById(heading.id))
      .filter((node): node is HTMLElement => Boolean(node));

    if (nodes.length === 0) return;

    let frame = 0;

    /**
     * The section being read is the last one whose heading has passed the line
     * just under the header — not whichever heading happens to sit inside a
     * narrow observer band, which leaves the marker stranded whenever a section
     * is taller than that band.
     */
    const measure = () => {
      frame = 0;
      const line = 0.28 * window.innerHeight;
      let current = nodes[0].id;

      for (const node of nodes) {
        if (node.getBoundingClientRect().top > line) break;
        current = node.id;
      }

      // The final section is often too short to ever reach the line.
      const atBottom = window.innerHeight + window.scrollY >= document.documentElement.scrollHeight - 2;
      setActive(atBottom ? nodes[nodes.length - 1].id : current);
    };

    const schedule = () => {
      frame ||= requestAnimationFrame(measure);
    };

    measure();
    window.addEventListener("scroll", schedule, { passive: true });
    window.addEventListener("resize", schedule, { passive: true });

    return () => {
      if (frame) cancelAnimationFrame(frame);
      window.removeEventListener("scroll", schedule);
      window.removeEventListener("resize", schedule);
    };
  }, [headings]);

  if (headings.length === 0) {
    return null;
  }

  return (
    <nav aria-label="Nesta página" className="docs-toc-rail">
      <div className="docs-toc">
        <p className="docs-nav-group">Nesta página</p>
        <ol className="docs-nav-scroll docs-toc-items">
          {headings.map((heading) => (
            <li key={heading.id} data-depth={heading.depth}>
              <Link href={`#${heading.id}`} aria-current={active === heading.id ? "location" : undefined}>
                {heading.text}
              </Link>
            </li>
          ))}
        </ol>
      </div>
    </nav>
  );
}
