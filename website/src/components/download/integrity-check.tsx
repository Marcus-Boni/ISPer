"use client";

import { useState } from "react";
import { Check, Copy, TriangleAlert } from "lucide-react";
import type { ReleaseAsset } from "@/lib/releases";

type CopyState = "idle" | "copied" | "failed";
type CopyTarget = "hash" | "command";

/**
 * Conferir o download, para os quatro arquivos da release.
 *
 * O comando é entre parênteses, com `.Hash` e em minúsculas: o `Get-FileHash`
 * sozinho imprime uma tabela, com o caminho cortado e a soma em maiúsculas, e
 * "se as duas linhas forem iguais" descrevia uma comparação que nunca parecia
 * igual. Assim ele imprime uma linha só, idêntica à soma mostrada abaixo.
 */
export function IntegrityCheck({ assets }: { assets: ReleaseAsset[] }) {
  const withSums = assets.filter((a) => a.sha256);
  const [name, setName] = useState(withSums[0]?.name ?? "");
  const [copyState, setCopyState] = useState<Record<CopyTarget, CopyState>>({ hash: "idle", command: "idle" });
  const selected = withSums.find((a) => a.name === name) ?? withSums[0];

  if (!selected) return <p className="integrity-note">Somas ainda pendentes no snapshot desta release.</p>;

  const command = `(Get-FileHash .\\${selected.name} -Algorithm SHA256).Hash.ToLower()`;

  async function copy(target: CopyTarget, value: string) {
    let result: CopyState;
    try {
      await navigator.clipboard.writeText(value);
      result = "copied";
    } catch {
      // A área de transferência é recusada em situações comuns; os dois valores
      // estão na tela de qualquer jeito, então dizemos o que aconteceu.
      result = "failed";
    }
    setCopyState((state) => ({ ...state, [target]: result }));
    window.setTimeout(() => setCopyState((state) => ({ ...state, [target]: "idle" })), 2600);
  }

  const label = (target: CopyTarget, idle: string) =>
    copyState[target] === "copied" ? "Copiado" : copyState[target] === "failed" ? "Não foi possível copiar" : idle;
  const icon = (target: CopyTarget) =>
    copyState[target] === "copied" ? <Check aria-hidden="true" /> : copyState[target] === "failed" ? <TriangleAlert aria-hidden="true" /> : <Copy aria-hidden="true" />;

  return (
    <div className="integrity">
      <label className="integrity-label" htmlFor="integrity-file">Arquivo que você baixou</label>
      <select id="integrity-file" className="integrity-select" value={selected.name} onChange={(event) => setName(event.target.value)}>
        {withSums.map((asset) => <option key={asset.name} value={asset.name}>{asset.name}</option>)}
      </select>
      <ol className="integrity-steps">
        <li>
          <span className="integrity-label">1 · No PowerShell, na pasta do download, rode</span>
          <code className="integrity-value">{command}</code>
          <button type="button" className="copy-button" data-copied={copyState.command === "copied"} onClick={() => copy("command", command)}>
            {icon("command")}{label("command", "Copiar comando")}
          </button>
        </li>
        <li>
          <span className="integrity-label">2 · Compare com a soma publicada</span>
          <code className="integrity-value">{selected.sha256}</code>
          <button type="button" className="copy-button" data-copied={copyState.hash === "copied"} onClick={() => copy("hash", selected.sha256 ?? "")}>
            {icon("hash")}{label("hash", "Copiar soma")}
          </button>
        </li>
      </ol>
      <p className="integrity-note" role="status">
        {copyState.hash === "failed" || copyState.command === "failed"
          ? "O navegador bloqueou a área de transferência. Selecione o texto e copie manualmente."
          : "Se as duas linhas forem iguais, o arquivo é exatamente o publicado na release."}
      </p>
      {selected.checksumSource ? (
        <a href={selected.checksumSource} className="text-link" target="_blank" rel="noreferrer noopener">
          Abrir o SHA256SUMS.txt da release<span className="visually-hidden"> (abre em nova aba)</span>
        </a>
      ) : null}
    </div>
  );
}
