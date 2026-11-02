/** Reveal native disclosures around errors, including errors owned by nested forms. */
export function revealFieldErrors(node: HTMLElement) {
  function reveal(element: Element) {
    for (let parent: Element | null = element; parent && node.contains(parent); parent = parent.parentElement) {
      if (parent instanceof HTMLDetailsElement) parent.open = true;
    }
  }
  function inspect() {
    node.querySelectorAll('[role="alert"], [aria-invalid="true"]').forEach(reveal);
  }
  function invalid(event: Event) {
    if (event.target instanceof Element) reveal(event.target);
  }
  const observer = new MutationObserver(inspect);
  observer.observe(node, { childList: true, subtree: true, attributes: true, attributeFilter: ["aria-invalid", "role"] });
  node.addEventListener("invalid", invalid, true);
  inspect();
  return { destroy() { observer.disconnect(); node.removeEventListener("invalid", invalid, true); } };
}
