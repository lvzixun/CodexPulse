import type { Action } from 'svelte/action';

interface Options {
  open: boolean;
  reduced: boolean;
  contentReady: boolean;
  settled: (open: boolean) => void;
}

// Observe natural content height, rather than capturing a loading placeholder once.
// Only an open/closing disclosure owns an observer; no permanent animation loop.
export const disclosure: Action<HTMLDivElement, Options> = (node, initial) => {
  const content = node.firstElementChild as HTMLElement;
  let options = initial;
  let animation: Animation | null = null;
  let target = 0;
  let destroyed = false;
  node.style.height = '0px';

  function resize(height: number) {
    const next = options.open ? height : 0;
    if (animation && next === target && !options.reduced) return;
    const current = node.getBoundingClientRect().height;
    // Keep the rendered height when interrupting an intro, outro or retarget.
    node.style.height = `${current}px`;
    if (animation) {
      animation.onfinish = null;
      animation.cancel();
      animation = null;
    }
    target = next;
    if (options.reduced || Math.abs(next - current) < 0.5) {
      node.style.height = `${next}px`;
      options.settled(options.open);
      return;
    }
    const active = node.animate([{ height: `${current}px` }, { height: `${next}px` }], {
      duration: 180,
      easing: 'ease-in-out',
      fill: 'both',
    });
    animation = active;
    active.onfinish = () => {
      if (destroyed || animation !== active) return;
      node.style.height = `${next}px`;
      active.cancel();
      animation = null;
      options.settled(options.open);
    };
  }

  const observer = new ResizeObserver(([entry]) => {
    if (destroyed || !options.open) return;
    const height = entry.borderBoxSize?.[0]?.blockSize ?? content.getBoundingClientRect().height;
    resize(height);
  });
  observer.observe(content);
  resize(content.getBoundingClientRect().height);

  return {
    update(next) {
      const changed =
        options.open !== next.open ||
        options.reduced !== next.reduced ||
        options.contentReady !== next.contentReady;
      options = next;
      if (changed) resize(content.getBoundingClientRect().height);
    },
    destroy() {
      destroyed = true;
      observer.disconnect();
      if (animation) {
        animation.onfinish = null;
        animation.cancel();
      }
    },
  };
};
