"use client";

import { Check, Copy } from "lucide-react";
import { useState } from "react";

const command = "cargo run --release -p isper-app --no-default-features";

export function CopyCommand() {
  const [state, setState] = useState<"idle" | "copied" | "failed">("idle");

  async function copy() {
    try {
      await navigator.clipboard.writeText(command);
      setState("copied");
    } catch {
      // The command is on screen either way; say so rather than doing nothing.
      setState("failed");
    }
    window.setTimeout(() => setState("idle"), 2400);
  }

  return (
    <div className="command-card">
      <span className="command-label">Rodar pelo código · CPU</span>
      <code>{command}</code>
      <button type="button" className="copy-button" data-copied={state === "copied"} onClick={copy} aria-label="Copiar comando">
        {state === "copied" ? <Check aria-hidden="true" /> : <Copy aria-hidden="true" />}
        {state === "copied" ? "Copiado" : state === "failed" ? "Selecione e copie" : "Copiar"}
      </button>
      <span className="visually-hidden" role="status">{state === "copied" ? "Comando copiado para a área de transferência." : state === "failed" ? "O navegador bloqueou a área de transferência; selecione o comando e copie manualmente." : ""}</span>
    </div>
  );
}
