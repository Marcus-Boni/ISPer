#!/usr/bin/env node
import { readFile, readdir, stat } from "node:fs/promises";
import { readFileSync } from "node:fs";
import path from "node:path";
import { gzipSync } from "node:zlib";

const outDir = path.join(process.cwd(), "out");
const routes = [
  // Next.js 16 + React 19 establish a shared client baseline. These limits are
  // measured per route and keep deliberate headroom without counting unrelated
  // docs pages or the on-demand Pagefind index against every navigation.
  { name: "landing", html: "index.html", js: 285 * 1024, total: 600 * 1024, htmlRaw: 110 * 1024 },
  { name: "download", html: "download/index.html", js: 190 * 1024, total: 520 * 1024, htmlRaw: 110 * 1024 },
  // The docs index intentionally renders both accessible mobile disclosure and
  // desktop tree navigation. Compression keeps transfer small; raw HTML gets
  // a narrow allowance for those duplicate landmarks.
  { name: "docs", html: "docs/index.html", js: 190 * 1024, total: 520 * 1024, htmlRaw: 120 * 1024 },
  { name: "docs-guide", html: "docs/primeiros-passos/instalacao/index.html", js: 190 * 1024, total: 520 * 1024, htmlRaw: 110 * 1024 },
];

async function walk(directory) {
  const entries = await readdir(directory, { withFileTypes: true });
  return (await Promise.all(entries.map(async (entry) => {
    const file = path.join(directory, entry.name);
    return entry.isDirectory() ? walk(file) : [file];
  }))).flat();
}

function transferSize(file) {
  if (/\.(?:html|css|js|json|xml|txt|svg)$/i.test(file)) return gzipSync(readFileSync(file)).byteLength;
  return readFileSync(file).byteLength;
}

function localAsset(url) {
  if (!(url.startsWith("/") && !url.startsWith("//"))) return null;
  const clean = decodeURIComponent(url.split(/[?#]/)[0]).replace(/^\//, "");
  return clean ? path.join(outDir, clean) : null;
}

const failures = [];
const measurements = [];

for (const route of routes) {
  const htmlPath = path.join(outDir, route.html);
  const html = await readFile(htmlPath, "utf8");
  const urls = [...html.matchAll(/(?:src|href)="([^"]+)"/g)].map((match) => match[1]);
  const assets = [...new Set(urls.map(localAsset).filter(Boolean))];
  const existing = [];
  for (const asset of assets) {
    try { if ((await stat(asset)).isFile()) existing.push(asset); } catch { /* route links are not static assets */ }
  }
  const jsAssets = existing.filter((file) => file.endsWith(".js"));
  const cssAssets = existing.filter((file) => file.endsWith(".css"));
  const jsBytes = jsAssets.reduce((sum, file) => sum + transferSize(file), 0);
  const cssBytes = cssAssets.reduce((sum, file) => sum + transferSize(file), 0);
  const totalBytes = transferSize(htmlPath) + existing.reduce((sum, file) => sum + transferSize(file), 0);
  measurements.push({ route: route.name, jsKiB: (jsBytes / 1024).toFixed(1), cssKiB: (cssBytes / 1024).toFixed(1), initialKiB: (totalBytes / 1024).toFixed(1), htmlRawKiB: (Buffer.byteLength(html) / 1024).toFixed(1) });
  if (jsBytes > route.js) failures.push(`${route.name}: JS inicial ${(jsBytes / 1024).toFixed(1)} KiB > ${route.js / 1024} KiB`);
  if (cssBytes > 35 * 1024) failures.push(`${route.name}: CSS inicial ${(cssBytes / 1024).toFixed(1)} KiB > 35 KiB`);
  if (totalBytes > route.total) failures.push(`${route.name}: transferência inicial ${(totalBytes / 1024).toFixed(1)} KiB > ${route.total / 1024} KiB`);
  if (Buffer.byteLength(html) > route.htmlRaw) failures.push(`${route.name}: HTML bruto ${(Buffer.byteLength(html) / 1024).toFixed(1)} KiB > ${route.htmlRaw / 1024} KiB`);
}

const allFiles = await walk(outDir);
for (const file of allFiles) {
  const size = (await stat(file)).size;
  if (size > 25 * 1024 * 1024) failures.push(`${path.relative(outDir, file)} excede o limite de 25 MiB do Pages`);
}
if (allFiles.length > 20_000) failures.push(`export possui ${allFiles.length} arquivos; limite do Pages Free é 20.000`);

console.table(measurements);
console.log(`Export completo: ${allFiles.length} arquivos. O tamanho total do acervo e do índice Pagefind é informativo; o gate mede a transferência inicial por rota.`);

if (failures.length) {
  console.error("Static budget check failed:");
  failures.forEach((failure) => console.error(`- ${failure}`));
  process.exit(1);
}
console.log("Static budget check passed.");
