"use client";

import { useMemo, useState } from "react";
import { Check, Copy, Cpu, Download, FileCheck2, MonitorCog, TriangleAlert } from "lucide-react";
import type { ReleaseAsset, ReleaseVariant } from "@/lib/releases";
import { formatBytes } from "@/lib/releases";

const variantCopy: Record<ReleaseVariant, { title: string; pitch: string }> = {
  cpu: {
    title: "Windows x64 · CPU",
    pitch: "Funciona em qualquer PC com Windows 10 ou 11. Comece por aqui se não tiver certeza.",
  },
  cuda: {
    title: "Windows x64 · CUDA",
    pitch: "Acelera os modelos Whisper maiores usando uma GPU NVIDIA compatível.",
  },
};

type CopyState = "idle" | "copied" | "failed";

export function DownloadSelector({ assets, notice }: { assets: ReleaseAsset[]; notice?: React.ReactNode }) {
  const [variant, setVariant] = useState<ReleaseVariant>("cpu");
  const [copyState, setCopyState] = useState<CopyState>("idle");

  const selected = useMemo(
    () => assets.find((asset) => asset.variant === variant) ?? assets[0],
    [assets, variant],
  );

  async function copyChecksum() {
    if (!selected?.sha256) return;
    try {
      await navigator.clipboard.writeText(selected.sha256);
      setCopyState("copied");
    } catch {
      // Clipboard access is refused in plenty of ordinary situations. The hash is
      // on screen either way, so say what happened instead of failing silently.
      setCopyState("failed");
    }
    window.setTimeout(() => setCopyState("idle"), 2600);
  }

  if (!selected) return null;

  return (
    <div className="download-panel">
      {/* Both variants stay on screen: choosing between 9,8 MB and 422,8 MB should
          not require remembering the card that disappeared. */}
      <div className="variant-grid" role="group" aria-label="Variante do instalador">
        {assets.map((asset) => {
          const isSelected = selected.variant === asset.variant;
          return (
            <button
              key={asset.variant}
              type="button"
              aria-pressed={isSelected}
              onClick={() => setVariant(asset.variant)}
              className={`variant-card ${isSelected ? "is-selected" : ""}`}
            >
              <span className="variant-head">
                {asset.variant === "cuda" ? <MonitorCog aria-hidden="true" /> : <Cpu aria-hidden="true" />}
                <span className="variant-title">{variantCopy[asset.variant].title}</span>
                <span className="variant-check" aria-hidden="true">
                  <Check />
                </span>
              </span>
              <span className="variant-size">{formatBytes(asset.sizeBytes)}</span>
              <span className="variant-pitch">{variantCopy[asset.variant].pitch}</span>
              <span className="variant-reqs">
                {asset.requirements.map((item) => (
                  <span key={item}>{item}</span>
                ))}
              </span>
            </button>
          );
        })}
      </div>

      {/* The warning sits beside the button, not after it: the reader sees what
          Windows is about to say while deciding to click, without the notice
          pushing the primary action off the fold. */}
      <div className="download-commit">
        <div className="download-row">
          <div>
            <p className="download-kicker">Instalador selecionado</p>
            <p className="download-file">{selected.name}</p>
            <p className="download-size">{formatBytes(selected.sizeBytes)} · Windows {selected.arch}</p>
          </div>
          <a className="button button-primary button-large" href={selected.downloadUrl}>
            <Download aria-hidden="true" />
            Baixar instalador
          </a>
        </div>
        {notice}
      </div>

      {/* The page asks the reader to verify the download, so it has to show the
          thing they are verifying against, not just a button that writes it away. */}
      <div className="integrity">
        <p className="integrity-head">
          <FileCheck2 aria-hidden="true" />
          SHA-256 deste arquivo
        </p>
        {selected.sha256 ? (
          <>
            <code className="integrity-hash">{selected.sha256}</code>
            <div className="integrity-actions">
              <button type="button" className="copy-button" data-copied={copyState === "copied"} onClick={copyChecksum}>
                {copyState === "copied" ? <Check aria-hidden="true" /> : copyState === "failed" ? <TriangleAlert aria-hidden="true" /> : <Copy aria-hidden="true" />}
                {copyState === "copied" ? "Copiado" : copyState === "failed" ? "Não foi possível copiar" : "Copiar"}
              </button>
              {selected.checksumSource ? (
                <a href={selected.checksumSource} className="text-link">
                  Abrir SHA256SUMS.txt
                </a>
              ) : null}
            </div>
            <p className="integrity-note" role="status">
              {copyState === "failed"
                ? "O navegador bloqueou a área de transferência. Selecione o texto acima e copie manualmente."
                : "Compare com o valor do arquivo baixado antes de instalar em ambiente controlado."}
            </p>
          </>
        ) : (
          <p className="integrity-note">Soma ainda pendente no snapshot local desta release.</p>
        )}
      </div>
    </div>
  );
}
