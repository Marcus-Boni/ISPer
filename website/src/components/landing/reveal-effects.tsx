"use client";

import { useEffect } from "react";
import { onLenisReady } from "@/components/motion/scroll-engine";

/**
 * Landing motion. Three registers, deliberately distinct:
 * 1. the hero arrival, which plays once on load;
 * 2. the stage settle, the page's single scrubbed moment;
 * 3. section entrances, which differ per section role.
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

        /* 1. Arrival. The headline wipes up behind its own mask, everything else follows it. */
        const heroTimeline = gsap.timeline({ defaults: { ease: "expo.out" } });
        heroTimeline
          .from(".hero-copy .hero-line", { yPercent: 118, duration: 1.15, stagger: 0.08 })
          .from(".hero-stage", { y: 34, opacity: 0, scale: 0.97, filter: "blur(10px)", duration: 1.25, clearProps: "filter" }, 0.18)
          .from(".hero-lead", { y: 18, opacity: 0, duration: 0.8 }, 0.4)
          .from(".hero-actions > *", { y: 16, opacity: 0, duration: 0.7, stagger: 0.08 }, 0.5)
          .from(".hero-proof li, .command-card > *", { y: 12, opacity: 0, duration: 0.6, stagger: 0.05 }, 0.62)
          .from(".trust-grid > *", { y: 10, opacity: 0, duration: 0.6, stagger: 0.05 }, 0.78);

        /* 2. The one scrubbed moment: the app window settles square as the hero
              leaves, and the stage plays the two things the page is about to
              explain while it holds. The reader can take it over at any point. */
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

          let released = false;
          const onReleased = () => { released = true; };
          window.addEventListener("isper:stage-released", onReleased);

          let current = "";
          const drive = (mode: "dictation" | "meeting", active: boolean) => {
            if (released) return;
            const next = `${mode}:${active}`;
            if (next === current) return;
            current = next;
            window.dispatchEvent(new CustomEvent("isper:stage", { detail: { mode, active } }));
          };

          const beats: Array<[number, "dictation" | "meeting", boolean]> = [
            [0, "dictation", false],
            [0.28, "dictation", true],
            [0.66, "meeting", true],
          ];

          ScrollTrigger.create({
            trigger: ".hero",
            start: "top top",
            end: "bottom 42%",
            invalidateOnRefresh: true,
            onUpdate: (self) => {
              const beat = [...beats].reverse().find(([at]) => self.progress >= at);
              if (beat) drive(beat[1], beat[2]);
            },
            onLeave: () => drive("meeting", false),
            onLeaveBack: () => drive("dictation", false),
          });

          return () => window.removeEventListener("isper:stage-released", onReleased);
        });

        /* 3. Section entrances, one register per section role. */
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
        const animated = "[data-reveal], [data-reveal-item], .hero-line, .hero-stage, .hero-lead, .hero-actions > *, .hero-proof li, .command-card > *, .trust-grid > *";
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
