"use client";

import { useMemo, useState } from "react";
import { Check, Copy, Cpu, Download, MonitorCog } from "lucide-react";
import type { ReleaseAsset, ReleaseVariant } from "@/lib/releases";
import { formatBytes } from "@/lib/releases";

export function DownloadSelector({ assets }: { assets: ReleaseAsset[] }) {
  const [variant, setVariant] = useState<ReleaseVariant>("cpu");
  const [copied, setCopied] = useState(false);

  const selected = useMemo(
    () => assets.find((asset) => asset.variant === variant) ?? assets[0],
    [assets, variant],
  );

  async function copyChecksum() {
    if (!selected?.sha256) return;
    await navigator.clipboard.writeText(selected.sha256);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1800);
  }

  if (!selected) return null;

  return (
    <div className="rounded-3xl border border-[var(--line)] bg-[var(--panel)] p-6 shadow-[0_30px_90px_-55px_rgba(0,0,0,0.9)]">
      <div className="flex flex-col gap-3 sm:flex-row" role="group" aria-label="Variante do instalador">
        {assets.map((asset) => (
          <button
            key={asset.variant}
            type="button"
            aria-pressed={selected.variant === asset.variant}
            onClick={() => setVariant(asset.variant)}
            className={`flex-1 rounded-2xl border p-4 text-left transition-colors ${
              selected.variant === asset.variant
                ? "border-[var(--accent)] bg-[var(--accent-soft)]"
                : "border-[var(--line)] bg-[var(--panel-2)] hover:border-[var(--line-2)]"
            }`}
          >
            <span className="flex items-center gap-2 font-semibold">
              {asset.variant === "cuda" ? (
                <MonitorCog aria-hidden="true" className="h-5 w-5 text-[var(--info)]" />
              ) : (
                <Cpu aria-hidden="true" className="h-5 w-5 text-[var(--accent-2)]" />
              )}
              {asset.variant === "cuda" ? "Windows x64 CUDA" : "Windows x64 CPU"}
            </span>
            <span className="mt-2 block text-sm text-[var(--muted)]">
              {asset.variant === "cuda"
                ? "Para GPU NVIDIA compatível e driver atualizado."
                : "Opção mais compatível para começar."}
            </span>
          </button>
        ))}
      </div>

      <div className="mt-6 rounded-2xl border border-[var(--line)] bg-[var(--bg-2)] p-5">
        <div className="flex flex-col justify-between gap-5 lg:flex-row lg:items-center">
          <div>
            <p className="text-sm text-[var(--muted)]">Instalador oficial</p>
            <h2 className="mt-1 break-words font-mono text-lg font-semibold text-[var(--ink)]">
              {selected.name}
            </h2>
            <p className="mt-2 text-sm text-[var(--ink-2)]">{formatBytes(selected.sizeBytes)}</p>
          </div>
          <a
            href={selected.downloadUrl}
            className="inline-flex items-center justify-center gap-2 rounded-xl bg-[var(--accent)] px-5 py-3 font-semibold text-[var(--accent-ink)] hover:bg-[var(--accent-2)]"
          >
            <Download aria-hidden="true" className="h-5 w-5" />
            Baixar .exe
          </a>
        </div>

        <div className="mt-6 grid gap-4 lg:grid-cols-2">
          <div>
            <h3 className="font-semibold">Requisitos</h3>
            <ul className="mt-3 space-y-2 text-sm text-[var(--ink-2)]">
              {selected.requirements.map((item) => (
                <li key={item} className="flex gap-2">
                  <Check aria-hidden="true" className="mt-0.5 h-4 w-4 shrink-0 text-[var(--good)]" />
                  {item}
                </li>
              ))}
            </ul>
          </div>
          <div>
            <h3 className="font-semibold">Integridade</h3>
            <p className="mt-3 text-sm leading-6 text-[var(--ink-2)]">
              SHA-256: {selected.sha256 ? "verificado" : "pendente no snapshot local"}
            </p>
            <button
              type="button"
              onClick={copyChecksum}
              disabled={!selected.sha256}
              className="mt-3 inline-flex items-center gap-2 rounded-lg border border-[var(--line)] px-3 py-2 text-sm text-[var(--ink-2)] disabled:cursor-not-allowed disabled:opacity-55"
            >
              <Copy aria-hidden="true" className="h-4 w-4" />
              {copied ? "Copiado" : "Copiar checksum"}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
