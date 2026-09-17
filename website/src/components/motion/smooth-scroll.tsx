"use client";

import { useEffect, useRef } from "react";
import { usePathname } from "next/navigation";
import Lenis from "lenis";
import { registerLenis } from "./scroll-engine";

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
      anchors: { offset: -96 },
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

  // A new route starts at its top. Lenis holds its own scroll value, so the
  // router's reset has to be mirrored here or the next wheel event snaps back.
  useEffect(() => {
    lenisRef.current?.scrollTo(0, { immediate: true, force: true });
  }, [pathname]);

  return <>{children}</>;
}
