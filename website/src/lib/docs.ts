import fs from "node:fs";
import path from "node:path";
import GithubSlugger from "github-slugger";

const DOCS_ROOT = path.join(process.cwd(), "content", "docs");

export type DocFrontmatter = {
  title: string;
  description: string;
  section: string;
  order: number;
};

export type DocHeading = {
  depth: 2 | 3;
  id: string;
  text: string;
};

export type DocBlock =
  | { type: "heading"; depth: 1 | 2 | 3; id: string; text: string }
  | { type: "paragraph"; text: string }
  | { type: "list"; items: string[] }
  | { type: "code"; language: string; code: string }
  | { type: "table"; rows: string[][] }
  | { type: "callout"; tone: "note" | "tip" | "warn"; text: string };

export type DocPage = {
  slug: string;
  segments: string[];
  href: string;
  frontmatter: DocFrontmatter;
  headings: DocHeading[];
  blocks: DocBlock[];
  raw: string;
};

export type DocNavItem = {
  section: string;
  items: Array<Pick<DocPage, "slug" | "href" | "frontmatter">>;
};

function readFiles(dir: string): string[] {
  if (!fs.existsSync(dir)) {
    return [];
  }

  return fs.readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const fullPath = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      return readFiles(fullPath);
    }
    return entry.isFile() && entry.name.endsWith(".md") ? [fullPath] : [];
  });
}

function parseFrontmatter(file: string) {
  const raw = fs.readFileSync(file, "utf8");
  const match = raw.match(/^---\r?\n([\s\S]*?)\r?\n---\r?\n([\s\S]*)$/);

  if (!match) {
    throw new Error(`Documento sem frontmatter: ${file}`);
  }

  const entries = match[1].split(/\r?\n/).filter(Boolean);
  const data = Object.fromEntries(
    entries.map((line) => {
      const index = line.indexOf(":");
      const key = line.slice(0, index).trim();
      const rawValue = line.slice(index + 1).trim();
      return [key, rawValue.replace(/^"|"$/g, "")];
    }),
  ) as Record<string, string>;

  const frontmatter: DocFrontmatter = {
    title: data.title,
    description: data.description,
    section: data.section,
    order: Number(data.order),
  };

  if (!frontmatter.title || !frontmatter.description || !frontmatter.section || Number.isNaN(frontmatter.order)) {
    throw new Error(`Frontmatter incompleto: ${file}`);
  }

  return { frontmatter, body: match[2], raw };
}

function parseMarkdown(body: string) {
  const lines = body.replace(/\r\n/g, "\n").split("\n");
  const blocks: DocBlock[] = [];
  const headings: DocHeading[] = [];
  // One slugger per document, walked in document order, exactly as rehype-slug
  // does — otherwise repeated headings would disagree on their suffix.
  const slugger = new GithubSlugger();
  const uniqueId = (text: string) => slugger.slug(text);

  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index];
    if (!line.trim()) {
      continue;
    }

    const heading = line.match(/^(#{1,3})\s+(.+)$/);
    if (heading) {
      const depth = heading[1].length as 1 | 2 | 3;
      const text = heading[2].trim();
      const id = uniqueId(text);
      blocks.push({ type: "heading", depth, id, text });
      if (depth === 2 || depth === 3) {
        headings.push({ depth, id, text });
      }
      continue;
    }

    const fence = line.match(/^```(\w+)?$/);
    if (fence) {
      const code: string[] = [];
      index += 1;
      while (index < lines.length && !lines[index].startsWith("```")) {
        code.push(lines[index]);
        index += 1;
      }
      blocks.push({ type: "code", language: fence[1] ?? "text", code: code.join("\n") });
      continue;
    }

    if (line.startsWith("> [!")) {
      const toneMatch = line.match(/^> \[!(NOTE|TIP|WARN)\]\s*$/);
      const text: string[] = [];
      index += 1;
      while (index < lines.length && lines[index].startsWith(">")) {
        text.push(lines[index].replace(/^>\s?/, ""));
        index += 1;
      }
      index -= 1;
      const toneMap = { NOTE: "note", TIP: "tip", WARN: "warn" } as const;
      blocks.push({ type: "callout", tone: toneMap[toneMatch?.[1] as keyof typeof toneMap] ?? "note", text: text.join(" ") });
      continue;
    }

    if (line.startsWith("- ")) {
      const items = [line.replace(/^- /, "")];
      while (index + 1 < lines.length && lines[index + 1].startsWith("- ")) {
        index += 1;
        items.push(lines[index].replace(/^- /, ""));
      }
      blocks.push({ type: "list", items });
      continue;
    }

    if (line.startsWith("|")) {
      const tableLines = [line];
      while (index + 1 < lines.length && lines[index + 1].startsWith("|")) {
        index += 1;
        tableLines.push(lines[index]);
      }
      const rows = tableLines
        .filter((row) => !/^\|\s*-+/.test(row))
        .map((row) =>
          row
            .split("|")
            .slice(1, -1)
            .map((cell) => cell.trim()),
        );
      blocks.push({ type: "table", rows });
      continue;
    }

    const paragraph = [line.trim()];
    while (
      index + 1 < lines.length &&
      lines[index + 1].trim() &&
      !/^(#{1,3})\s+/.test(lines[index + 1]) &&
      !lines[index + 1].startsWith("```") &&
      !lines[index + 1].startsWith("- ") &&
      !lines[index + 1].startsWith("|") &&
      !lines[index + 1].startsWith("> [!")
    ) {
      index += 1;
      paragraph.push(lines[index].trim());
    }
    blocks.push({ type: "paragraph", text: paragraph.join(" ") });
  }

  return { blocks, headings };
}

export function getAllDocs(): DocPage[] {
  return readFiles(DOCS_ROOT)
    .map((file) => {
      const { frontmatter, body, raw } = parseFrontmatter(file);
      const relative = path.relative(DOCS_ROOT, file);
      const segments = relative.replace(/\.md$/, "").split(path.sep);
      const slug = segments.join("/");
      const { blocks, headings } = parseMarkdown(body);

      return {
        slug,
        segments,
        href: `/docs/${segments.join("/")}`,
        frontmatter,
        headings,
        blocks,
        raw,
      };
    })
    .sort((a, b) => a.frontmatter.order - b.frontmatter.order);
}

export function getDocBySegments(segments: string[]) {
  const slug = segments.join("/");
  return getAllDocs().find((doc) => doc.slug === slug);
}

export function getDocsNav(): DocNavItem[] {
  const groups = new Map<string, DocNavItem>();

  for (const doc of getAllDocs()) {
    const group = groups.get(doc.frontmatter.section) ?? {
      section: doc.frontmatter.section,
      items: [],
    };
    group.items.push({
      slug: doc.slug,
      href: doc.href,
      frontmatter: doc.frontmatter,
    });
    groups.set(doc.frontmatter.section, group);
  }

  return Array.from(groups.values());
}

export function getAdjacentDocs(slug: string) {
  const docs = getAllDocs();
  const index = docs.findIndex((doc) => doc.slug === slug);
  return {
    previous: index > 0 ? docs[index - 1] : null,
    next: index >= 0 && index < docs.length - 1 ? docs[index + 1] : null,
  };
}

/**
 * The synchronous fallback behind the docs search, sent to every docs page.
 *
 * In production Pagefind searches the full text, so the fallback carries only
 * titles, descriptions and sections. The body goes along only under `next dev`,
 * where Pagefind is not built. Shipping it everywhere put the text of every
 * doc into every docs page — each new doc added ~7 KiB of raw HTML to all of
 * them, until the two Copilot guides pushed the docs routes past their budget.
 */
export function getDocsSearchIndex() {
  const withBody = process.env.NODE_ENV === "development";
  return getAllDocs().map((doc) => ({
    title: doc.frontmatter.title,
    description: doc.frontmatter.description,
    section: doc.frontmatter.section,
    href: doc.href,
    text: withBody
      ? doc.raw
          .replace(/^---[\s\S]*?---/, "")
          .replace(/[#>*`|_-]/g, " ")
          .replace(/\s+/g, " ")
          .trim()
      : "",
  }));
}
