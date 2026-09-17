"use client";

import { useEffect, useRef } from "react";
import { usePathname } from "next/navigation";
import Lenis from "lenis";
import { registerLenis } from "./scroll-engine";

/** Clears the sticky header when an anchor is the destination. */
const HEADER_OFFSET = 96;

export function SmoothScrollProvider({ children }: { children: React.ReactNode }) {
  const lenisRef = useRef<Lenis | null>(null);
  const pathname = usePathname();

  useEffect(() => {
    // Touch devices keep native momentum; reduced motion keeps native scrolling entirely.
    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    if (window.matchMedia("(pointer: coarse)").matches) return;

    const lenis = new Lenis({
      duration: 1.05,
      easing: (t) => 1 - Math.pow(1 - t, 3.2),
      smoothWheel: true,
      syncTouch: false,
      anchors: { offset: -HEADER_OFFSET },
      // Nested scrollers — the docs sidebar, code blocks, wide tables — take the
      // wheel natively while they still have room, then hand it back to the page.
      allowNestedScroll: true,
    });

    lenisRef.current = lenis;
    registerLenis(lenis);

    let frame = requestAnimationFrame(function raf(time: number) {
      lenis.raf(time);
      frame = requestAnimationFrame(raf);
    });

    return () => {
      cancelAnimationFrame(frame);
      lenis.destroy();
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
   * `/#recursos` arrives here with the hash in the URL and would otherwise be
   * reset to the top — the address bar claiming a section the reader never saw.
   */
  useEffect(() => {
    const lenis = lenisRef.current;
    if (!lenis) return;

    const hash = window.location.hash.slice(1);
    if (!hash) {
      lenis.scrollTo(0, { immediate: true, force: true });
      return;
    }

    let frame = 0;
    let attempts = 0;
    const seek = () => {
      const target = document.getElementById(decodeURIComponent(hash));
      if (target) {
        lenis.scrollTo(target, { offset: -HEADER_OFFSET, immediate: true, force: true });
        return;
      }
      // The incoming route may not have painted yet; give it a few frames.
      if (attempts++ < 20) frame = requestAnimationFrame(seek);
      else lenis.scrollTo(0, { immediate: true, force: true });
    };
    frame = requestAnimationFrame(seek);
    return () => cancelAnimationFrame(frame);
  }, [pathname]);

  return <>{children}</>;
}
