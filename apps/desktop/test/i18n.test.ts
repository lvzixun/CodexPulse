import test from 'node:test';
import assert from 'node:assert/strict';
import english from '../src/lib/locales/en.json' with { type: 'json' };
import {
  locale,
  resolveLanguage,
  translateFor,
  translate,
  localizeError,
} from '../src/lib/i18n.ts';
import { resetAge, untilReset, sessionLabel } from '../src/lib/format.ts';

test('display language variants resolve to Chinese or English; unsupported primary language uses English', () => {
  for (const tag of ['zh', 'zh-CN', 'ZH_hant_TW', 'zh-HK'])
    assert.equal(resolveLanguage(tag), 'zh');
  for (const tag of ['en', 'en-GB', 'fr-FR', 'ja-JP', 'zhongwen', '', null, undefined]) {
    assert.equal(resolveLanguage(tag), 'en');
  }
  assert.equal(translateFor(resolveLanguage('fr-FR'), '总览'), 'Overview');
});

test('all English messages preserve interpolation parameters and contain no Chinese text', () => {
  const parameters = (text: string) => [...text.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort();
  for (const [key, entry] of Object.entries(english)) {
    for (const value of typeof entry === 'string' ? [entry] : Object.values(entry)) {
      assert.ok(value.trim(), key);
      assert.doesNotMatch(value, /\p{Script=Han}/u, key);
      assert.deepEqual(parameters(value), parameters(key), key);
    }
  }
  assert.equal(translateFor('en', '{_0} 天前', { _0: 1 }), '1 day ago');
  assert.equal(translateFor('en', '会话 · {_0}', { _0: 'example' }), 'Session · example');
  assert.equal(translateFor('zh', '会话 · {_0}', { _0: 'example' }), '会话 · example');
});

test('language changes affect formatting and errors without changing user session titles', () => {
  const now = Date.parse('2026-10-06T12:00:00Z');
  try {
    locale.set('en');
    assert.equal(resetAge('2026-10-03T12:00:00Z', now), '3 days ago');
    assert.equal(translate('退出'), 'Quit');
    assert.equal(untilReset({ resets_at: now / 1000 + 60 } as never, now), 'Resets in 1m');
    assert.equal(localizeError('采集器繁忙，请稍后重试'), 'Collector is busy. Try again later.');
    assert.doesNotMatch(localizeError('未识别的异常'), /\p{Script=Han}/u);
    locale.set('zh');
    assert.equal(resetAge('2026-10-03T12:00:00Z', now), '3 天前');
    assert.equal(translate('退出'), '退出');
    assert.equal(untilReset({ resets_at: now / 1000 + 60 } as never, now), '1 分钟后刷新');
    assert.equal(
      sessionLabel({ id: 'example-id', title: 'My 中文 project' } as never),
      'My 中文 project',
    );
  } finally {
    locale.set('en');
  }
});
