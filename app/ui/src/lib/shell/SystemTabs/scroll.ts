// Svelte action for a System strip that scrolls horizontally when it overflows: the wheel scrolls it sideways,
// and the selected tab ([aria-selected="true"] or [data-sel="true"]) is kept in view when the selection changes.
export function strip(node: HTMLElement, selected?: string | null) {
  void selected;
  const onWheel = (e: WheelEvent) => {
    if (node.scrollWidth <= node.clientWidth + 1) return;
    if (Math.abs(e.deltaX) >= Math.abs(e.deltaY)) return;
    node.scrollLeft += e.deltaY;
    e.preventDefault();
  };
  const reveal = () => {
    const el = node.querySelector<HTMLElement>('[aria-selected="true"], [data-sel="true"]');
    if (!el || node.scrollWidth <= node.clientWidth + 1) return;
    const a = node.getBoundingClientRect();
    const b = el.getBoundingClientRect();
    if (b.left < a.left) node.scrollLeft -= a.left - b.left + 8;
    else if (b.right > a.right) node.scrollLeft += b.right - a.right + 8;
  };
  node.addEventListener('wheel', onWheel, { passive: false });
  queueMicrotask(reveal);
  return {
    update(next?: string | null) {
      void next;
      queueMicrotask(reveal);
    },
    destroy() {
      node.removeEventListener('wheel', onWheel);
    },
  };
}
