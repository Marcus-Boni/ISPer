"use client";

import { Check, Copy } from "lucide-react";
import { useState } from "react";

const command = "cargo run --release -p isper-app --no-default-features";

export function CopyCommand() {
  const [copied, setCopied] = useState(false);

  async function copy() {
    try {
      await navigator.clipboard.writeText(command);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1800);
    } catch {
      setCopied(false);
    }
  }

  return (
    <div className="command-card">
      <span className="command-label">Rodar pelo código · CPU</span>
      <code>{command}</code>
      <button type="button" className="copy-button" onClick={copy} aria-label="Copiar comando">
        {copied ? <Check aria-hidden="true" /> : <Copy aria-hidden="true" />}
        {copied ? "Copiado" : "Copiar"}
      </button>
    </div>
  );
}
