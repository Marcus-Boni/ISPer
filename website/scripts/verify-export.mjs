#!/usr/bin/env node
import { access, readdir, readFile, stat } from "node:fs/promises";
import path from "node:path";

const root = process.cwd();
const outDir = path.join(root, "out");

async function exists(relativePath) {
  try {
    await access(path.join(outDir, relativePath));
    return true;
  } catch {
    return false;
  }
}

async function walk(dir) {
  const entries = await readdir(dir, { withFileTypes: true });
  const files = [];

  for (const entry of entries) {
    const fullPath = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      files.push(...(await walk(fullPath)));
    } else {
      files.push(fullPath);
    }
  }

  return files;
}

const requiredFiles = ["index.html", "404.html", "_headers", "_redirects"];
const failures = [];

for (const file of requiredFiles) {
  if (!(await exists(file))) {
    failures.push(`out/${file} is missing`);
  }
}

if (await exists("index.html")) {
  const index = await readFile(path.join(outDir, "index.html"), "utf8");
  for (const text of ["ISPer", "Transcrição", "sem enviar seu áudio"]) {
    if (!index.includes(text)) {
      failures.push(`out/index.html does not include expected text: ${text}`);
    }
  }
}

const files = await walk(outDir);
const serverArtifacts = files.filter((file) =>
  /(?:^|[\\/])(?:server|cache|standalone|trace)(?:[\\/]|$)/.test(file),
);

for (const file of serverArtifacts) {
  failures.push(`server-only artifact found in export: ${path.relative(root, file)}`);
}

const stats = await Promise.all(files.map((file) => stat(file)));
const emptyFiles = files.filter((_, index) => stats[index].size === 0);
const allowedEmpty = new Set(["_redirects"]);

for (const file of emptyFiles) {
  const relative = path.relative(outDir, file).replaceAll("\\", "/");
  if (!allowedEmpty.has(relative)) {
    failures.push(`empty exported file: out/${relative}`);
  }
}

if (failures.length > 0) {
  console.error("Static export verification failed:");
  for (const failure of failures) {
    console.error(`- ${failure}`);
  }
  process.exit(1);
}

console.log(`Static export verification passed (${files.length} files).`);
