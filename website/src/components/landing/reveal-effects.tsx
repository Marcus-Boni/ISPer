"use client";

import { useEffect } from "react";
import { onLenisReady } from "@/components/motion/scroll-engine";

/**
 * Landing motion: the stage settle and the section entrances.
 *
 * The hero arrival is not here any more. The headline is dictated in pure CSS
 * (hero-headline.tsx), which starts on the first frame the stylesheet arrives
 * instead of waiting for hydration and this module's dynamic import — the
 * delay that used to leave the hero sitting in its from-state. The stage
 * plays its own demonstration once the headline is done, so it no longer
 * needs to be driven by scroll beats from here.
 *
 * Nothing here is allowed to hide content it cannot bring back: elements already
 * on screen animate from a visible state, and a failsafe clears every from-state
 * if the scroll layer never reports back.
 */
export function RevealEffects() {
  useEffect(() => {
    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;

    let cleanup = () => {};
    let cancelled = false;

    void Promise.all([import("gsap"), import("gsap/ScrollTrigger")]).then(([gsapModule, triggerModule]) => {
      if (cancelled) return;
      const gsap = gsapModule.default;
      const { ScrollTrigger } = triggerModule;
      gsap.registerPlugin(ScrollTrigger);
      ScrollTrigger.config({ ignoreMobileResize: true });

      const unsubscribe = onLenisReady((lenis) => lenis.on("scroll", ScrollTrigger.update));

      const context = gsap.context(() => {
        const inView = (element: Element) => element.getBoundingClientRect().top < window.innerHeight * 0.92;

        /* The one scrubbed moment: the app window settles square as the hero leaves. */
        const media = gsap.matchMedia();
        media.add("(min-width: 981px)", () => {
          gsap.to(".hero-stage .app-window", {
            rotateY: 0,
            rotateX: 0,
            y: -10,
            scale: 1.015,
            ease: "none",
            scrollTrigger: {
              trigger: ".hero",
              start: "top top",
              end: "bottom 42%",
              scrub: 0.6,
              invalidateOnRefresh: true,
            },
          });
        });

        /* Section entrances, one register per section role. */
        gsap.utils.toArray<HTMLElement>("[data-reveal]").forEach((section) => {
          if (inView(section)) return;
          const scrollTrigger = { trigger: section, start: "top 86%", once: true, invalidateOnRefresh: true };
          const timeline = gsap.timeline({ scrollTrigger, defaults: { ease: "expo.out" } });

          const heading = section.querySelector<HTMLElement>("h2");
          if (heading) timeline.from(heading, { y: 26, opacity: 0, duration: 0.9 });
          const lead = section.querySelector<HTMLElement>(".section-heading p, .section-lead");
          if (lead) timeline.from(lead, { y: 16, opacity: 0, duration: 0.7 }, 0.12);

          const items = section.querySelectorAll<HTMLElement>("[data-reveal-item]");
          if (items.length) {
            timeline.from(items, { y: 22, opacity: 0, scale: 0.985, duration: 0.8, stagger: 0.07 }, 0.16);
          }

          const keys = section.querySelectorAll<HTMLElement>(".key-combo kbd");
          if (keys.length) {
            timeline.from(keys, { y: -10, opacity: 0, duration: 0.5, stagger: 0.09, ease: "back.out(2.2)" }, 0.2);
          }
        });
      });

      /* Layout settles late: webfonts swap and the chart mounts after hydration. */
      const refresh = () => ScrollTrigger.refresh();
      void document.fonts?.ready.then(refresh);
      const settle = window.setTimeout(refresh, 1200);

      /* Failsafe: never leave content that GSAP hid but never revealed. */
      const failsafe = window.setTimeout(() => {
        const animated = "[data-reveal], [data-reveal-item]";
        document.querySelectorAll<HTMLElement>(animated).forEach((element) => {
          if (Number(getComputedStyle(element).opacity) < 0.99) gsap.set(element, { clearProps: "all" });
        });
      }, 6000);

      cleanup = () => {
        window.clearTimeout(settle);
        window.clearTimeout(failsafe);
        unsubscribe();
        context.revert();
      };
    });

    return () => {
      cancelled = true;
      cleanup();
    };
  }, []);

  return null;
}
