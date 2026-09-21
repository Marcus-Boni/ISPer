#!/usr/bin/env node
/**
 * Deriva content/data/releases.snapshot.json da release publicada no GitHub.
 *
 * O snapshot foi mantido à mão até 21/09/2026 e ficou duas versões para trás
 * sem ninguém notar: a página de download anunciava um instalador e publicava
 * a soma SHA-256 de outro, o que torna o fluxo de integridade — a razão de a
 * página existir — impossível de seguir. Nada aqui é digitado: tamanho e URL
 * vêm da API, as somas vêm do SHA256SUMS.txt da própria release, e as notas
 * saem do corpo dela (que o release.ps1 já monta a partir do CHANGELOG).
 *
 *   node scripts/sync-release.mjs            # grava a partir da release mais recente
 *   node scripts/sync-release.mjs --tag v1.2.3
 *   node scripts/sync-release.mjs --check    # não grava; sai 1 se estiver defasado
 *
 * Campos editoriais (requirements, authenticodeStatus) são preservados por
 * variante: descrevem o produto, não o arquivo, e mudam por decisão humana.
 */

import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import process from "node:process";
import { notesFromBody } from "./lib/release-notes.mjs";

const SNAPSHOT = path.join(process.cwd(), "content", "data", "releases.snapshot.json");
const CHECKSUMS = "SHA256SUMS.txt";

/** Campos que descrevem o produto, usados quando a variante ainda não existe no snapshot. */
const DEFAULTS = {
  cpu: {
    authenticodeStatus: "not-signed",
    requirements: ["Windows 10/11 x64", "CPU moderna com AVX2 recomendado", "8 GB de RAM ou mais"],
  },
  cuda: {
    authenticodeStatus: "not-signed",
    requirements: ["Windows 10/11 x64", "GPU NVIDIA compatível com CUDA", "VRAM conforme modelo Whisper escolhido"],
  },
};

function parseArgs(argv) {
  const args = { check: false, tag: null };
  for (let i = 0; i < argv.length; i += 1) {
    if (argv[i] === "--check") args.check = true;
    else if (argv[i] === "--tag") args.tag = argv[i + 1] ?? null;
    else if (argv[i].startsWith("--tag=")) args.tag = argv[i].slice("--tag=".length);
    else throw new Error(`argumento desconhecido: ${argv[i]}`);
  }
  if (args.tag !== null && !/^v\d+\.\d+\.\d+/.test(args.tag)) {
    throw new Error(`--tag precisa parecer uma tag de versão, recebi: ${args.tag}`);
  }
  return args;
}

async function api(url) {
  const headers = { accept: "application/vnd.github+json", "user-agent": "isper-portal-sync" };
  // Sem token funciona (repositório público), mas o limite anônimo é baixo.
  if (process.env.GITHUB_TOKEN) headers.authorization = `Bearer ${process.env.GITHUB_TOKEN}`;
  const response = await fetch(url, { headers });
  if (!response.ok) throw new Error(`GET ${url} devolveu ${response.status} ${response.statusText}`);
  return response.json();
}

function variantOf(name) {
  if (/-cpu-setup\.exe$/i.test(name)) return "cpu";
  if (/-setup\.exe$/i.test(name)) return "cuda";
  return null;
}

async function checksums(asset) {
  const response = await fetch(asset.browser_download_url, {
    headers: { "user-agent": "isper-portal-sync" },
    redirect: "follow",
  });
  if (!response.ok) throw new Error(`não consegui baixar ${CHECKSUMS}: ${response.status}`);
  const map = new Map();
  for (const line of (await response.text()).split(/\r?\n/)) {
    const match = line.match(/^([0-9a-f]{64})\s+\*?(.+)$/i);
    if (match) map.set(match[2].trim(), match[1].toLowerCase());
  }
  if (map.size === 0) throw new Error(`${CHECKSUMS} veio sem nenhuma linha reconhecível`);
  return map;
}

async function build(previous, tag) {
  const repository = previous.repository;
  if (!repository) throw new Error("o snapshot atual não diz a qual repositório pertence");
  const base = `https://api.github.com/repos/${repository}/releases`;
  const release = await api(tag ? `${base}/tags/${tag}` : `${base}/latest`);

  if (release.draft) throw new Error(`${release.tag_name} é rascunho; não publico rascunho no site`);

  const sums = release.assets.find((asset) => asset.name === CHECKSUMS);
  if (!sums) throw new Error(`a release ${release.tag_name} não tem ${CHECKSUMS}; sem ele não publico soma nenhuma`);
  const sha256 = await checksums(sums);

  const keep = new Map(previous.assets?.map((asset) => [asset.variant, asset]) ?? []);
  const assets = [];
  for (const asset of release.assets) {
    const variant = variantOf(asset.name);
    if (!variant) continue;
    const sum = sha256.get(asset.name);
    if (!sum) throw new Error(`${asset.name} não aparece no ${CHECKSUMS} da release`);
    const editorial = keep.get(variant) ?? DEFAULTS[variant];
    assets.push({
      name: asset.name,
      platform: "windows",
      arch: "x64",
      variant,
      kind: "installer",
      sizeBytes: asset.size,
      downloadUrl: asset.browser_download_url,
      sha256: sum,
      checksumSource: sums.browser_download_url,
      authenticodeStatus: editorial.authenticodeStatus,
      requirements: editorial.requirements,
    });
  }

  // A página existe para o leitor escolher entre as duas; uma sozinha é um
  // erro de empacotamento, não um snapshot válido.
  const found = assets.map((asset) => asset.variant).sort();
  if (found.join(",") !== "cpu,cuda") {
    throw new Error(`esperava instaladores cpu e cuda, encontrei: ${found.join(", ") || "nenhum"}`);
  }
  assets.sort((a, b) => a.variant.localeCompare(b.variant));

  const notes = notesFromBody(release.body);
  if (notes.length === 0) throw new Error(`a release ${release.tag_name} não tem notas legíveis no corpo`);

  return {
    repository,
    tag: release.tag_name,
    version: release.tag_name.replace(/^v/, ""),
    channel: release.prerelease ? "preview" : "stable",
    publishedAt: release.published_at,
    fetchedAt: new Date().toISOString(),
    releaseUrl: release.html_url,
    notes,
    assets,
  };
}

/** `fetchedAt` muda a cada execução: compará-lo faria todo check acusar defasagem. */
function meaningful(snapshot) {
  return JSON.stringify({ ...snapshot, fetchedAt: null }, null, 2);
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const previous = JSON.parse(readFileSync(SNAPSHOT, "utf8"));
  const next = await build(previous, args.tag);

  if (meaningful(previous) === meaningful(next)) {
    console.log(`Snapshot já está em ${next.tag}; nada a fazer.`);
    return;
  }

  const summary = `${previous.tag ?? "?"} -> ${next.tag}`;
  if (args.check) {
    console.error(`Snapshot do portal defasado (${summary}).`);
    console.error("Rode: node scripts/sync-release.mjs");
    process.exitCode = 1;
    return;
  }

  writeFileSync(SNAPSHOT, `${JSON.stringify(next, null, 2)}\n`, "utf8");
  console.log(`Snapshot atualizado (${summary}).`);
  for (const asset of next.assets) {
    console.log(`  ${asset.variant.padEnd(4)} ${asset.name}  ${asset.sizeBytes} B  ${asset.sha256.slice(0, 12)}…`);
  }
  for (const note of next.notes) console.log(`  nota: ${note}`);
}

main().catch((error) => {
  console.error(`sync-release: ${error.message}`);
  process.exitCode = 1;
});
