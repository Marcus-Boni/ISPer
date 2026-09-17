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
type CopyTarget = "hash" | "command";

export function DownloadSelector({ assets, notice }: { assets: ReleaseAsset[]; notice?: React.ReactNode }) {
  const [variant, setVariant] = useState<ReleaseVariant>("cpu");
  const [copyState, setCopyState] = useState<Record<CopyTarget, CopyState>>({ hash: "idle", command: "idle" });

  const selected = useMemo(
    () => assets.find((asset) => asset.variant === variant) ?? assets[0],
    [assets, variant],
  );

  async function copy(target: CopyTarget, value: string) {
    let result: CopyState;
    try {
      await navigator.clipboard.writeText(value);
      result = "copied";
    } catch {
      // Clipboard access is refused in plenty of ordinary situations. Both values
      // are on screen either way, so say what happened instead of failing silently.
      result = "failed";
    }
    setCopyState((state) => ({ ...state, [target]: result }));
    window.setTimeout(() => setCopyState((state) => ({ ...state, [target]: "idle" })), 2600);
  }

  if (!selected) return null;

  const hashCommand = `Get-FileHash .\\${selected.name} -Algorithm SHA256`;

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

      {/* Telling someone to compare a checksum without giving them the command
          that produces one is an instruction they cannot follow. This is a
          Windows-only product, so the command is the PowerShell one. */}
      <div className="integrity">
        <p className="integrity-head">
          <FileCheck2 aria-hidden="true" />
          Conferir o que você baixou
        </p>
        {selected.sha256 ? (
          <>
            <ol className="integrity-steps">
              <li>
                <span className="integrity-label">1 · Rode no PowerShell, na pasta do download</span>
                <code className="integrity-value">{hashCommand}</code>
                <button type="button" className="copy-button" data-copied={copyState.command === "copied"} onClick={() => copy("command", hashCommand)}>
                  {copyState.command === "copied" ? <Check aria-hidden="true" /> : copyState.command === "failed" ? <TriangleAlert aria-hidden="true" /> : <Copy aria-hidden="true" />}
                  {copyState.command === "copied" ? "Copiado" : copyState.command === "failed" ? "Não foi possível copiar" : "Copiar comando"}
                </button>
              </li>
              <li>
                <span className="integrity-label">2 · Compare com este valor</span>
                <code className="integrity-value integrity-hash">{selected.sha256}</code>
                <button type="button" className="copy-button" data-copied={copyState.hash === "copied"} onClick={() => copy("hash", selected.sha256 ?? "")}>
                  {copyState.hash === "copied" ? <Check aria-hidden="true" /> : copyState.hash === "failed" ? <TriangleAlert aria-hidden="true" /> : <Copy aria-hidden="true" />}
                  {copyState.hash === "copied" ? "Copiado" : copyState.hash === "failed" ? "Não foi possível copiar" : "Copiar soma"}
                </button>
              </li>
            </ol>
            {selected.checksumSource ? (
              <a href={selected.checksumSource} className="text-link" target="_blank" rel="noreferrer noopener">
                Abrir SHA256SUMS.txt
                <span className="visually-hidden">(abre em nova aba)</span>
              </a>
            ) : null}
            <p className="integrity-note" role="status">
              {copyState.hash === "failed" || copyState.command === "failed"
                ? "O navegador bloqueou a área de transferência. Selecione o texto e copie manualmente."
                : "Se as duas linhas forem iguais, o arquivo é o publicado na release."}
            </p>
          </>
        ) : (
          <p className="integrity-note">Soma ainda pendente no snapshot local desta release.</p>
        )}
      </div>
    </div>
  );
}
