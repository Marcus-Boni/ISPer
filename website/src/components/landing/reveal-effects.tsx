"use client";

import { useEffect } from "react";

export function RevealEffects() {
  useEffect(() => {
    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    let cleanup = () => {};
    void Promise.all([import("gsap"), import("gsap/ScrollTrigger")]).then(([gsapModule, triggerModule]) => {
      const gsap = gsapModule.default;
      const ScrollTrigger = triggerModule.ScrollTrigger;
      gsap.registerPlugin(ScrollTrigger);
      const context = gsap.context(() => {
        gsap.utils.toArray<HTMLElement>("[data-reveal]").forEach((element) => {
          gsap.fromTo(element, { y: 24, opacity: 0.01 }, { y: 0, opacity: 1, duration: 0.72, ease: "power3.out", scrollTrigger: { trigger: element, start: "top 88%", once: true } });
        });
        const media = gsap.matchMedia();
        media.add("(min-width: 1081px) and (prefers-reduced-motion: no-preference)", () => {
          gsap.timeline({
            scrollTrigger: {
              trigger: ".hero",
              start: "top 76px",
              end: "+=420",
              scrub: .55,
              pin: ".hero-stage",
              pinSpacing: true,
              invalidateOnRefresh: true,
            },
          }).to(".app-window", { rotateY: 2, rotateX: 0, y: -12, ease: "none" });
        });
      });
      cleanup = () => context.revert();
    });
    return () => cleanup();
  }, []);
  return null;
}
