import Link from "next/link";
import type { ReactNode } from "react";
import type { DocBlock } from "@/lib/docs";

function Inline({ text }: { text: string }) {
  const parts: ReactNode[] = [];
  const pattern = /(`[^`]+`|\*\*[^*]+\*\*|\[[^\]]+\]\([^)]+\))/g;
  let lastIndex = 0;

  for (const match of text.matchAll(pattern)) {
    if (match.index > lastIndex) {
      parts.push(text.slice(lastIndex, match.index));
    }

    const value = match[0];
    if (value.startsWith("`")) {
      parts.push(
        <code key={`${value}-${match.index}`} className="rounded-md border border-[var(--line)] bg-[var(--bg-2)] px-1.5 py-0.5 font-mono text-[0.92em] text-[var(--accent-2)]">
          {value.slice(1, -1)}
        </code>,
      );
    } else if (value.startsWith("**")) {
      parts.push(
        <strong key={`${value}-${match.index}`} className="font-semibold text-[var(--ink)]">
          {value.slice(2, -2)}
        </strong>,
      );
    } else {
      const link = value.match(/^\[([^\]]+)\]\(([^)]+)\)$/);
      if (link) {
        const href = link[2];
        const external = /^https?:/.test(href);
        parts.push(
          <Link key={`${value}-${match.index}`} href={href} className="font-medium text-[var(--accent-2)] underline decoration-[var(--accent)]/50 underline-offset-4 hover:text-[var(--accent)]" target={external ? "_blank" : undefined} rel={external ? "noreferrer" : undefined}>
            {link[1]}
          </Link>,
        );
      }
    }

    lastIndex = match.index + value.length;
  }

  if (lastIndex < text.length) {
    parts.push(text.slice(lastIndex));
  }

  return <>{parts}</>;
}

export function DocRenderer({ blocks }: { blocks: DocBlock[] }) {
  return (
    <article className="max-w-3xl">
      {blocks.map((block, index) => {
        if (block.type === "heading") {
          const Tag = `h${block.depth}` as "h1" | "h2" | "h3";
          const className =
            block.depth === 1
              ? "font-display text-4xl font-semibold leading-tight text-[var(--ink)] sm:text-5xl"
              : block.depth === 2
                ? "mt-12 scroll-mt-24 font-display text-3xl font-semibold text-[var(--ink)]"
                : "mt-8 scroll-mt-24 text-xl font-semibold text-[var(--ink)]";
          return (
            <Tag key={`${block.id}-${index}`} id={block.id} className={className}>
              {block.text}
            </Tag>
          );
        }

        if (block.type === "paragraph") {
          return (
            <p key={index} className="mt-5 text-[1.02rem] leading-8 text-[var(--ink-2)]">
              <Inline text={block.text} />
            </p>
          );
        }

        if (block.type === "list") {
          return (
            <ul key={index} className="mt-5 space-y-3 pl-5 text-[1.02rem] leading-7 text-[var(--ink-2)]">
              {block.items.map((item) => (
                <li key={item} className="list-disc marker:text-[var(--accent)]">
                  <Inline text={item} />
                </li>
              ))}
            </ul>
          );
        }

        if (block.type === "code") {
          return (
            <pre key={index} className="mt-6 overflow-x-auto rounded-xl border border-[var(--line)] bg-[#120f0e] p-4 text-sm leading-7 text-[var(--ink-2)] shadow-inner">
              <code className="font-mono">{block.code}</code>
            </pre>
          );
        }

        if (block.type === "table") {
          const [head, ...rows] = block.rows;
          return (
            <div key={index} className="mt-6 overflow-hidden rounded-xl border border-[var(--line)]">
              <table className="w-full border-collapse text-left text-sm">
                <thead className="bg-[var(--panel-2)] text-[var(--ink)]">
                  <tr>
                    {head.map((cell) => (
                      <th key={cell} className="border-b border-[var(--line)] px-4 py-3 font-semibold">
                        <Inline text={cell} />
                      </th>
                    ))}
                  </tr>
                </thead>
                <tbody>
                  {rows.map((row) => (
                    <tr key={row.join("|")} className="border-b border-[var(--line)] last:border-b-0">
                      {row.map((cell) => (
                        <td key={cell} className="px-4 py-3 text-[var(--ink-2)]">
                          <Inline text={cell} />
                        </td>
                      ))}
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          );
        }

        const toneClass = {
          note: "border-[var(--info)]/40 bg-[rgba(124,196,240,0.10)]",
          tip: "border-[var(--good)]/40 bg-[rgba(132,194,151,0.10)]",
          warn: "border-[var(--warn)]/40 bg-[rgba(232,193,90,0.10)]",
        }[block.tone];

        return (
          <aside key={index} className={`mt-6 rounded-xl border p-4 text-[var(--ink-2)] ${toneClass}`}>
            <p className="leading-7">
              <Inline text={block.text} />
            </p>
          </aside>
        );
      })}
    </article>
  );
}
