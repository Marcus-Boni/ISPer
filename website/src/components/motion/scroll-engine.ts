import type Lenis from "lenis";

type Listener = (lenis: Lenis) => void;

let instance: Lenis | null = null;
const listeners = new Set<Listener>();

/** Publishes the single Lenis instance so motion scenes can sync to it. */
export function registerLenis(lenis: Lenis | null) {
  instance = lenis;
  if (lenis) listeners.forEach((listener) => listener(lenis));
}

/** Runs immediately when Lenis already exists, otherwise on the next registration. */
export function onLenisReady(listener: Listener) {
  if (instance) {
    listener(instance);
    return () => {};
  }
  listeners.add(listener);
  return () => listeners.delete(listener);
}
