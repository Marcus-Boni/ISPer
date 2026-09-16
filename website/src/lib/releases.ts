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

export function formatBytes(sizeBytes: number | null) {
  if (!sizeBytes) return "Tamanho pendente de verificação";
  const units = ["B", "KB", "MB", "GB"];
  let value = sizeBytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(unit === 0 ? 0 : 1)} ${units[unit]}`;
}

export const releaseIntegrityNotice =
  "Dados conferidos na release pública v0.15.0 em 16/09/2026. Compare o arquivo baixado com SHA256SUMS.txt antes de instalar em ambientes controlados.";

export const sourceInstallSteps = [
  "Instale Rust stable, Visual Studio Build Tools com C++ e Git.",
  "Clone o repositório oficial Marcus-Boni/ISPer.",
  "Para CPU, rode cargo build --release --no-default-features.",
  "Para CUDA, use o caminho de release documentado no repositório e valide o driver NVIDIA.",
];

export const releaseLinks = {
  all: siteConfig.releases,
  current: currentRelease.releaseUrl,
  checksums: `${currentRelease.releaseUrl}`,
};
