import { test } from 'node:test';
import assert from 'node:assert/strict';
import { subscriptionGroup } from '../src/lib/subscriptions.ts';

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

test('a failed registration releases successful and subsequently resolved listeners', async () => {
  const failed = deferred<() => void>();
  const late = deferred<() => void>();
  const released: string[] = [];
  const scope = subscriptionGroup([
    async () => () => released.push('early'),
    () => failed.promise,
    () => late.promise,
  ]);
  const rejection = assert.rejects(scope.ready, /registration failed/);
  await Promise.resolve();
  await Promise.resolve();
  failed.reject(new Error('registration failed'));
  await rejection;
  assert.deepEqual(released, ['early']);
  late.resolve(() => released.push('late'));
  await Promise.resolve();
  assert.deepEqual(released, ['early', 'late']);
  scope.dispose();
  assert.deepEqual(released, ['early', 'late']);
});

test('closing before native registration finishes releases it exactly once', async () => {
  const late = deferred<() => void>();
  let released = 0;
  const scope = subscriptionGroup([() => late.promise]);
  await Promise.resolve();
  scope.dispose();
  late.resolve(() => released++);
  await scope.ready;
  scope.dispose();
  assert.equal(released, 1);
});

test('closing before registration starts does not allocate a listener', async () => {
  let started = 0;
  const scope = subscriptionGroup([
    async () => {
      started++;
      return () => {};
    },
  ]);
  scope.dispose();
  await scope.ready;
  assert.equal(started, 0);
});

test('synchronous registration errors and throwing cleanup cannot strand siblings', async () => {
  let released = 0;
  const good = subscriptionGroup([
    async () => () => {
      throw new Error('native listener already gone');
    },
    async () => () => released++,
  ]);
  await good.ready;
  good.dispose();
  good.dispose();
  assert.equal(released, 1);

  const failed = subscriptionGroup([
    async () => () => released++,
    () => {
      throw new Error('synchronous registration failure');
    },
  ]);
  await assert.rejects(failed.ready, /synchronous registration failure/);
  assert.equal(released, 2);
});
