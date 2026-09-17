"use client";

import { ViewTransition } from "react";

/**
 * Wraps a route's content so navigations animate.
 *
 * This belongs in each `page.tsx`, never in a layout: layouts persist across
 * navigations, so their enter and exit animations would never fire. The shared
 * chrome — header and footer — is anchored in CSS instead, which keeps a fixed
 * reference point while the content moves.
 *
 * Typed navigations slide in the direction of travel. Everything else — sibling
 * routes, the browser's own back button, a refresh — takes the neutral lift.
 */
export function PageTransition({ children }: { children: React.ReactNode }) {
  return (
    <ViewTransition
      enter={{ "nav-forward": "nav-forward", "nav-back": "nav-back", default: "page-shift" }}
      exit={{ "nav-forward": "nav-forward", "nav-back": "nav-back", default: "page-shift" }}
      default="none"
    >
      {children}
    </ViewTransition>
  );
}
