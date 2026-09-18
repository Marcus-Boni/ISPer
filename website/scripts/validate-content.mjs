#!/usr/bin/env node
import { readFile } from "node:fs/promises";
import path from "node:path";

const root = process.cwd();

async function read(relativePath) {
  return readFile(path.join(root, relativePath), "utf8");
}

const checks = [
  {
    file: "src/app/layout.tsx",
    label: "html lang is pt-BR",
    pattern: /lang="pt-BR"/,
  },
  {
    file: "src/lib/site.ts",
    label: "default public URL uses Cloudflare Pages",
    pattern: /https:\/\/isper\.pages\.dev/,
  },
  {
    file: "src/app/page.tsx",
    label: "landing identifies ISPer",
    pattern: /ISPer/,
  },
  {
    file: "src/app/page.tsx",
    label: "landing communicates local transcription",
    pattern: /Local|local/,
  },
];

const failures = [];

for (const check of checks) {
  const source = await read(check.file);
  if (!check.pattern.test(source)) {
    failures.push(`${check.file}: ${check.label}`);
  }
}

const publicFiles = ["public/_headers", "public/_redirects"];
for (const file of publicFiles) {
  const source = await read(file);
  if (source.includes("YOUR_") || source.includes("TODO")) {
    failures.push(`${file}: contains placeholder text`);
  }
}

if (failures.length > 0) {
  console.error("Website content validation failed:");
  for (const failure of failures) {
    console.error(`- ${failure}`);
  }
  process.exit(1);
}

console.log("Website content validation passed.");
