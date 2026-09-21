"use client";

import { useEffect, useRef } from "react";
import { usePathname } from "next/navigation";
import type Lenis from "lenis";
import { registerLenis } from "./scroll-engine";

/**
 * How far below the top of the viewport a scroll target comes to rest.
 *
 * Read from the scrollport's own `scroll-padding-top` so CSS stays the single
 * source of truth. It used to be read from the target's `scroll-margin-top`,
 * which said the same thing a second time — and the browser and Lenis both
 * subtract *both* (`lenis.mjs:783`), so every anchor rested a header too low.
 */
function anchorOffset() {
  const root = document.scrollingElement ?? document.documentElement;
  const padding = Number.parseFloat(getComputedStyle(root).scrollPaddingTop);
  return Number.isNaN(padding) ? 0 : padding;
}

export function SmoothScrollProvider({ children }: { children: React.ReactNode }) {
  const lenisRef = useRef<Lenis | null>(null);
  const pathname = usePathname();

  useEffect(() => {
    // Touch devices keep native momentum; reduced motion keeps native scrolling entirely.
    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    if (window.matchMedia("(pointer: coarse)").matches) return;

    let lenis: Lenis | null = null;
    let frame = 0;
    let cancelled = false;

    /* Importado só depois dos dois testes acima, e não no topo do arquivo.
       Estático, a biblioteca entrava no pacote de todo mundo — inclusive de
       quem nunca a executa, que é todo celular (ponteiro grosso) e todo leitor
       com movimento reduzido. Baixar o que não vai rodar é caro justamente
       para quem tende a estar na conexão pior. */
    void import("lenis").then(({ default: Lenis }) => {
      if (cancelled) return;
      lenis = new Lenis({
        duration: 1.05,
        easing: (t) => 1 - Math.pow(1 - t, 3.2),
        smoothWheel: true,
        syncTouch: false,
        anchors: true,
        // Nested scrollers — the docs sidebar, code blocks, wide tables — take the
        // wheel natively while they still have room, then hand it back to the page.
        allowNestedScroll: true,
      });

      lenisRef.current = lenis;
      registerLenis(lenis);

      frame = requestAnimationFrame(function raf(time: number) {
        lenis?.raf(time);
        frame = requestAnimationFrame(raf);
      });
    });

    return () => {
      cancelled = true;
      cancelAnimationFrame(frame);
      lenis?.destroy();
      lenisRef.current = null;
      registerLenis(null);
    };
  }, []);

  /**
   * A new route starts at its top, and Lenis holds its own scroll value, so the
   * router's reset has to be mirrored here or the next wheel event snaps back.
   *
   * Unless the navigation asked for an anchor. Lenis's own `anchors` option only
   * sees real anchor clicks, not client navigations, so a cross-route link like
   * `/#benchmarks` arrives here with the hash in the URL.
   *
   * This must not depend on Lenis existing. Lenis is skipped under reduced
   * motion *and* on coarse pointers — which is every phone — so guarding the
   * whole effect on it meant the anchor silently failed for most visitors, the
   * exact failure this exists to prevent.
   */
  useEffect(() => {
    const hash = window.location.hash.slice(1);

    if (!hash) {
      lenisRef.current?.scrollTo(0, { immediate: true, force: true });
      return;
    }

    const id = decodeURIComponent(hash);
    let frame = 0;
    let agreed = 0;
    let stopped = false;
    // Generous, because it is a ceiling and not a schedule: agreement ends the
    // loop after three frames in the ordinary case.
    const deadline = performance.now() + 3000;

    // The moment the reader scrolls, the scroll is theirs.
    const release = () => { stopped = true; };
    const yields = ["wheel", "touchstart", "keydown", "pointerdown"] as const;
    yields.forEach((type) => window.addEventListener(type, release, { passive: true }));

    const seek = (now: number) => {
      if (stopped) return;
      const target = document.getElementById(id);
      if (target) {
        /* The incoming route keeps growing after it commits — a dynamic import
           resolves, a font swaps, the reveal effects release their from-states.
           Lenis clamps every scroll to the document height it measured last,
           which at this point is still the *outgoing* page's, so measuring once
           parked the reader at the old page's maximum scroll — up to 3,000px
           short of the heading they asked for, silently. Re-measure instead:
           refresh Lenis's dimensions, correct by whatever is still missing, and
           stop once two consecutive frames agree on the position.

           Lido a cada quadro, e não uma vez: o Lenis agora é carregado sob
           demanda e pode chegar no meio desta busca. Enquanto não chegou, a
           rolagem nativa serve — e é o único caminho no celular, onde ele
           nunca chega. */
        const lenis = lenisRef.current;
        lenis?.resize();
        const delta = target.getBoundingClientRect().top - anchorOffset();
        if (Math.abs(delta) <= 1) {
          if (++agreed >= 2) return;
        } else {
          agreed = 0;
          const top = window.scrollY + delta;
          if (lenis) lenis.scrollTo(top, { immediate: true, force: true });
          else window.scrollTo({ top, behavior: "auto" });
        }
      }
      if (now < deadline) frame = requestAnimationFrame(seek);
    };

    frame = requestAnimationFrame(seek);
    return () => {
      stopped = true;
      cancelAnimationFrame(frame);
      yields.forEach((type) => window.removeEventListener(type, release));
    };
  }, [pathname]);

  return <>{children}</>;
}
