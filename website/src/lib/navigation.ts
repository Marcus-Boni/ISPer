/**
 * Direction for view transitions.
 *
 * The portal is a shallow tree — `/` above `/download` and `/docs`, and `/docs`
 * above each guide — so depth alone tells us whether a navigation goes deeper or
 * comes back up. Moves between siblings carry no direction and cross-fade.
 */

export type TransitionType = "nav-forward" | "nav-back";

function depthOf(path: string) {
  const [route] = path.split(/[?#]/);
  return route.split("/").filter(Boolean).length;
}

/**
 * Returns the transition types for a navigation, or `undefined` when the move
 * should not be read as travel: same-page anchors, reloads and sibling routes.
 */
export function transitionTypesFor(from: string, to: string): TransitionType[] | undefined {
  if (to.startsWith("#") || to.startsWith("http")) return undefined;

  const fromDepth = depthOf(from);
  const toDepth = depthOf(to);
  if (fromDepth === toDepth) return undefined;

  return [toDepth > fromDepth ? "nav-forward" : "nav-back"];
}
