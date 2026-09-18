#!/usr/bin/env node
import { access } from "node:fs/promises";
import path from "node:path";

const root = process.cwd();
const required = [
  "src/app/layout.tsx",
  "src/app/page.tsx",
  "src/lib/site.ts",
  "public/_headers",
  "public/_redirects",
];

const missing = [];

for (const relativePath of required) {
  try {
    await access(path.join(root, relativePath));
  } catch {
    missing.push(relativePath);
  }
}

if (missing.length > 0) {
  console.error("Missing required website inputs:");
  for (const item of missing) {
    console.error(`- ${item}`);
  }
  process.exit(1);
}

console.log("Website content inputs are present.");
