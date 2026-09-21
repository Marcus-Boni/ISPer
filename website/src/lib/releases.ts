import releaseSnapshot from "../../content/data/releases.snapshot.json";
import { siteConfig } from "@/lib/site";

export type ReleaseVariant = "cpu" | "cuda";
export type ReleaseKind = "installer" | "portable" | "source";

export type ReleaseAsset = {
  name: string;
  platform: "windows";
  arch: "x64";
  variant: ReleaseVariant;
  kind: ReleaseKind;
  sizeBytes: number | null;
  downloadUrl: string;
  sha256: string | null;
  checksumSource: string | null;
  authenticodeStatus: "verified" | "unverified" | "not-signed";
  requirements: string[];
};

export type ReleaseSnapshot = {
  repository: string;
  tag: string;
  version: string;
  channel: "stable" | "preview";
  publishedAt: string | null;
  fetchedAt: string;
  releaseUrl: string;
  notes: string[];
  assets: ReleaseAsset[];
};

export const currentRelease = releaseSnapshot as ReleaseSnapshot;

export const downloadVariants = currentRelease.assets.filter(
  (asset) => asset.kind === "installer",
);

export function getAssetByVariant(variant: ReleaseVariant) {
  return downloadVariants.find((asset) => asset.variant === variant);
}

/**
 * Decimal units, because the reader is comparing this against what GitHub and
 * the browser report for the same file. Dividing by 1024 and writing "MB" names
 * the wrong unit, and this is the page whose whole subject is byte-exact checks.
 */
export function formatBytes(sizeBytes: number | null) {
  if (!sizeBytes) return "Tamanho pendente de verificação";
  const units = ["B", "kB", "MB", "GB"];
  let value = sizeBytes;
  let unit = 0;
  while (value >= 1000 && unit < units.length - 1) {
    value /= 1000;
    unit += 1;
  }
  const formatted = value.toLocaleString("pt-BR", {
    minimumFractionDigits: unit === 0 ? 0 : 1,
    maximumFractionDigits: unit === 0 ? 0 : 1,
  });
  return `${formatted} ${units[unit]}`;
}

/**
 * The audience is Brazilian and the build machine is not. GitHub Actions runs
 * in UTC, so a bare `toLocaleDateString("pt-BR")` rendered a release published
 * at 21:39 in São Paulo as the following day, and the same commit produced
 * different HTML here and in CI. Naming the zone fixes both at once.
 */
export function formatReleaseDate(iso: string) {
  return new Date(iso).toLocaleDateString("pt-BR", { timeZone: "America/Sao_Paulo" });
}

/* Derived from the snapshot: a hand-written version here went stale twice over
   while the file beside it was correct. */
export const releaseIntegrityNotice =
  `Dados conferidos na release pública ${currentRelease.tag} em ${formatReleaseDate(currentRelease.fetchedAt)}.`;

export const sourceInstallSteps = [
  "Instale Rust stable, Visual Studio Build Tools com C++ e Git.",
  "Clone o repositório oficial Marcus-Boni/ISPer.",
  "Para CPU, compile com cargo build --release --no-default-features e abra com cargo run --release -p isper-app --no-default-features.",
  "Para CUDA, use o caminho de release documentado no repositório e valide o driver NVIDIA.",
];

export const releaseLinks = {
  all: siteConfig.releases,
  current: currentRelease.releaseUrl,
  checksums: `${currentRelease.releaseUrl}`,
};
