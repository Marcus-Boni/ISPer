"use client";

import { Check, Copy } from "lucide-react";
import { useRef, useState } from "react";

/**
 * A copy control on every code block. Commands are the one thing on a docs page
 * a reader always wants in their clipboard, and selecting a multi-line block by
 * hand is exactly the friction that sends people to the repository instead.
 */
export function DocPre({ children, ...props }: React.ComponentProps<"pre">) {
  const pre = useRef<HTMLPreElement>(null);
  const [state, setState] = useState<"idle" | "copied" | "failed">("idle");

  async function copy() {
    const text = pre.current?.innerText ?? "";
    if (!text.trim()) return;
    try {
      await navigator.clipboard.writeText(text);
      setState("copied");
    } catch {
      setState("failed");
    }
    window.setTimeout(() => setState("idle"), 2200);
  }

  return (
    <div className="doc-code">
      <pre ref={pre} {...props}>{children}</pre>
      <button type="button" className="doc-code-copy" data-state={state} onClick={copy} aria-label="Copiar código">
        {state === "copied" ? <Check aria-hidden="true" /> : <Copy aria-hidden="true" />}
        <span className="visually-hidden" role="status">
          {state === "copied" ? "Código copiado." : state === "failed" ? "Não foi possível copiar." : ""}
        </span>
      </button>
    </div>
  );
}
