"use client";

import { useCallback, useRef } from "react";

/**
 * The keyboard contract `role="tablist"` promises.
 *
 * Declaring the role tells a screen-reader user "tab 1 of 2, use the arrow
 * keys". Without roving tabindex and arrow handling they press Right, nothing
 * happens, and every tab also sits in the tab sequence — the announcement and
 * the behaviour disagree.
 *
 * Returns a ref for the tablist container and a factory for each tab's props.
 */
export function useTablist<T extends string>(ids: readonly T[], active: T, select: (id: T) => void) {
  const list = useRef<HTMLDivElement>(null);

  const focusTab = useCallback((id: T) => {
    list.current?.querySelector<HTMLElement>(`[data-tab="${id}"]`)?.focus();
  }, []);

  const tabProps = useCallback((id: T) => ({
    "data-tab": id,
    role: "tab" as const,
    "aria-selected": id === active,
    // Only the selected tab is in the tab sequence; arrows move within the set.
    tabIndex: id === active ? 0 : -1,
    onClick: () => select(id),
    onKeyDown: (event: React.KeyboardEvent) => {
      const index = ids.indexOf(id);
      let next: T | undefined;
      if (event.key === "ArrowRight" || event.key === "ArrowDown") next = ids[(index + 1) % ids.length];
      else if (event.key === "ArrowLeft" || event.key === "ArrowUp") next = ids[(index - 1 + ids.length) % ids.length];
      else if (event.key === "Home") next = ids[0];
      else if (event.key === "End") next = ids[ids.length - 1];
      if (!next) return;
      event.preventDefault();
      select(next);
      focusTab(next);
    },
  }), [ids, active, select, focusTab]);

  return { list, tabProps };
}
