#!/usr/bin/env node
import { access, readFile, readdir } from "node:fs/promises";
import path from "node:path";

const outDir = path.join(process.cwd(), "out");

async function walk(directory) {
  const entries = await readdir(directory, { withFileTypes: true });
  return (await Promise.all(entries.map(async (entry) => {
    const file = path.join(directory, entry.name);
    return entry.isDirectory() ? walk(file) : [file];
  }))).flat();
}

async function exists(file) {
  try { await access(file); return true; } catch { return false; }
}

function targetFor(pathname) {
  const relative = decodeURIComponent(pathname).replace(/^\//, "");
  if (!relative) return path.join(outDir, "index.html");
  if (path.extname(relative)) return path.join(outDir, relative);
  return path.join(outDir, relative, "index.html");
}

const htmlFiles = (await walk(outDir)).filter((file) => file.endsWith(".html"));
const failures = [];

for (const source of htmlFiles) {
  const html = await readFile(source, "utf8");
  const hrefs = [...html.matchAll(/href="([^"]+)"/g)].map((match) => match[1]);
  for (const href of hrefs) {
    if (!(href.startsWith("/") && !href.startsWith("//"))) continue;
    const url = new URL(href, "https://isper.pages.dev");
    const target = targetFor(url.pathname);
    if (!(await exists(target))) {
      failures.push(`${path.relative(outDir, source)} -> ${href}`);
      continue;
    }
    if (url.hash && target.endsWith(".html")) {
      const targetHtml = await readFile(target, "utf8");
      const id = decodeURIComponent(url.hash.slice(1));
      if (!(targetHtml.includes(`id="${id}"`) || targetHtml.includes(`name="${id}"`))) failures.push(`${path.relative(outDir, source)} -> ${href} (anchor missing)`);
    }
  }
}

if (failures.length) {
  console.error("Internal link check failed:");
  failures.forEach((failure) => console.error(`- ${failure}`));
  process.exit(1);
}
console.log(`Internal link check passed (${htmlFiles.length} HTML files).`);
