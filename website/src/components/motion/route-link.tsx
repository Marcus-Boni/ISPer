"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import type { ComponentProps } from "react";
import { transitionTypesFor } from "@/lib/navigation";

type RouteLinkProps = Omit<ComponentProps<typeof Link>, "transitionTypes">;

/**
 * A `Link` that tags its own navigation direction.
 *
 * Shared chrome appears at every depth, so the same "Documentação" link goes
 * deeper from `/` and comes back from `/docs/referencia/hardware`. Reading the
 * current path at click time is the only way to get that right.
 */
export function RouteLink({ href, ...props }: RouteLinkProps) {
  const pathname = usePathname();
  const target = typeof href === "string" ? href : (href.pathname ?? "");

  // Next's own scroll restoration would race the anchor seek and win, landing
  // the reader at the top with the section in the address bar.
  const hasHash = target.includes("#");

  return (
    <Link
      href={href}
      scroll={hasHash ? false : undefined}
      transitionTypes={transitionTypesFor(pathname, target)}
      {...props}
    />
  );
}
