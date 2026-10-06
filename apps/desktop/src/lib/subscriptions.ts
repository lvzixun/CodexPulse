type Unlisten = () => void;

// Own each registration as soon as it resolves. A rejected sibling must not
// orphan successful listeners, including registrations that finish after close.
export function subscriptionGroup(register: (() => Promise<Unlisten>)[]) {
  let disposed = false;
  const active = new Set<Unlisten>();
  const release = (off: Unlisten) => {
    try {
      off();
    } catch {
      // Continue releasing siblings even if one native listener has gone away.
    }
  };
  const dispose = () => {
    if (disposed) return;
    disposed = true;
    const pending = [...active];
    active.clear();
    pending.forEach(release);
  };
  const ready = Promise.all(
    register.map(async (start) => {
      // Put synchronous registration failures through the same cleanup path.
      await Promise.resolve();
      if (disposed) return;
      const off = await start();
      if (disposed) release(off);
      else active.add(off);
    }),
  )
    .then(() => {})
    .catch((error: unknown) => {
      dispose();
      throw error;
    });
  return { ready, dispose };
}
