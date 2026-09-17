"use client";

import { useEffect, useRef, useState } from "react";

/**
 * A horizontal scroller is only a keyboard target when it actually scrolls.
 * At desktop widths this table fits, so a permanent tabIndex put an empty stop
 * in the tab order; at 390px the rows restack and it never scrolls either.
 */
export function ComparisonWrap({ children }: { children: React.ReactNode }) {
  const ref = useRef<HTMLDivElement>(null);
  const [scrollable, setScrollable] = useState(false);

  useEffect(() => {
    const element = ref.current;
    if (!element) return;
    const measure = () => setScrollable(element.scrollWidth > element.clientWidth + 1);
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  return (
    <div
      ref={ref}
      className="comparison-wrap"
      data-reveal-item
      tabIndex={scrollable ? 0 : undefined}
      role={scrollable ? "region" : undefined}
      aria-label={scrollable ? "Comparação de arquitetura e privacidade" : undefined}
    >
      {children}
    </div>
  );
}
