"use client";

import { Check, Copy } from "lucide-react";
import { useEffect, useRef, useState } from "react";

/**
 * A copy control on every code block. Commands are the one thing on a docs page
 * a reader always wants in their clipboard, and selecting a multi-line block by
 * hand is exactly the friction that sends people to the repository instead.
 */
export function DocPre({ children, ...props }: React.ComponentProps<"pre">) {
  const pre = useRef<HTMLPreElement>(null);
  const [state, setState] = useState<"idle" | "copied" | "failed">("idle");
  const [scrollable, setScrollable] = useState(false);

  /**
   * Foco só onde há o que rolar.
   *
   * O rehype-pretty-code marca todo `<pre>` com `tabindex="0"`, que é a técnica
   * certa para uma região rolável — e errada para uma que cabe na tela. No
   * desktop, os dois blocos da página de instalação não transbordam, então
   * eram duas paradas de tabulação que não faziam nada e não se anunciavam.
   * Quando transborda de verdade, a região ganha nome, porque uma parada sem
   * nome só diz "grupo" para quem usa leitor de tela.
   */
  useEffect(() => {
    const node = pre.current;
    if (!node) return;
    const measure = () => setScrollable(node.scrollWidth > node.clientWidth + 1);
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(node);
    return () => observer.disconnect();
  }, []);

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
      <pre
        ref={pre}
        {...props}
        tabIndex={scrollable ? 0 : undefined}
        role={scrollable ? "region" : undefined}
        aria-label={scrollable ? "Bloco de código, rolável na horizontal" : undefined}
      >
        {children}
      </pre>
      <button type="button" className="doc-code-copy" data-state={state} onClick={copy} aria-label="Copiar código">
        {state === "copied" ? <Check aria-hidden="true" /> : <Copy aria-hidden="true" />}
        <span className="visually-hidden" role="status">
          {state === "copied" ? "Código copiado." : state === "failed" ? "Não foi possível copiar." : ""}
        </span>
      </button>
    </div>
  );
}
