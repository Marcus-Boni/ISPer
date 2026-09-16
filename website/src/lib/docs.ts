import fs from "node:fs";
import path from "node:path";

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

function slugify(value: string) {
  return value
    .normalize("NFD")
    .replace(/[\u0300-\u036f]/g, "")
    .toLowerCase()
    .replace(/[^a-z0-9\s-]/g, "")
    .trim()
    .replace(/\s+/g, "-")
    .replace(/-+/g, "-");
}

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
  const usedIds = new Map<string, number>();

  const uniqueId = (text: string) => {
    const base = slugify(text);
    const count = usedIds.get(base) ?? 0;
    usedIds.set(base, count + 1);
    return count === 0 ? base : `${base}-${count + 1}`;
  };

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

export function getDocsSearchIndex() {
  return getAllDocs().map((doc) => ({
    title: doc.frontmatter.title,
    description: doc.frontmatter.description,
    section: doc.frontmatter.section,
    href: doc.href,
    text: doc.raw
      .replace(/^---[\s\S]*?---/, "")
      .replace(/[#>*`|_-]/g, " ")
      .replace(/\s+/g, " ")
      .trim(),
  }));
}
