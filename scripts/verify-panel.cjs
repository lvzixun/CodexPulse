const { chromium } = require(process.env.CODEXPULSE_PLAYWRIGHT_MODULE || 'playwright');
const assert = require('node:assert/strict');
const path = require('node:path');
const base = process.env.CODEXPULSE_PREVIEW_URL || 'http://127.0.0.1:1430/';
if (!['127.0.0.1', 'localhost'].includes(new URL(base).hostname))
  throw Error('Use a local dev server');
const api =
  '/@fs/' +
  path.resolve(__dirname, '../apps/desktop/node_modules/@tauri-apps/api').replaceAll('\\', '/');
// Uses Tauri's official IPC mocks; this does not verify native OS menu rendering.
const boot = String.raw`
import {mockIPC,mockWindows} from '__TAURI_API__/mocks.js';
window.isTauri=true;
mockWindows('pulse');
const state=window.__panelTest={data:null,view:null,shown:true,reads:[],actions:[],menus:[],popups:[],closed:[],fail:false,hold:false,release:null};
let emit;
mockIPC(async(cmd,args)=>{
 if(cmd==='get_ui_language')return new URLSearchParams(location.search).get('language')||'zh';
 if(cmd==='get_snapshot')return structuredClone(state.data);
 if(cmd==='get_view_state')return structuredClone(state.view);
 if(cmd==='remember_view')return;
 if(cmd==='plugin:window|is_visible')return state.shown;
 if(cmd==='read_news'){
  state.reads.push([...args.keys]);
  if(state.hold)await new Promise(resolve=>state.release=resolve);
  if(state.fail)throw Error('Cannot save read state');
  state.data.news.unread_keys=state.data.news.unread_keys.filter(k=>!args.keys.includes(k));
  state.data.news.important_unread=state.data.news.unread_keys.length;
  return;
 }
 if(cmd==='window_action'){
  state.actions.push(args.action);
  if(args.action==='expand'||args.action==='compact'){
   state.view.mode=args.action==='expand'?'details':'compact';
   await emit('window-mode',[state.view.mode,null]);
  }
  if(args.action==='hide'){state.shown=false;await emit('window-visible',false)}
  return;
 }
 if(cmd==='plugin:menu|new'){
  const rid=state.menus.length+1;
  state.menus.push({rid,...args});
  return [rid,args.options.id||'test-menu'];
 }
 if(cmd==='plugin:menu|popup'){state.popups.push(args);return}
 if(cmd==='plugin:resources|close'){state.closed.push(args.rid);return}
 throw Error('Unexpected mock IPC: '+cmd);
},{shouldMockEvents:true});
({emit}=await import('__TAURI_API__/event.js'));
const {empty}=await import('/src/lib/ipc.ts');
const {previewSnapshot,previewView}=await import('/src/lib/readme-preview.ts');
state.data=previewSnapshot(empty);
state.view={...previewView(),mode:'details',page:'overview',floating_supported:true};
state.push=async(keys,success=false)=>{
 state.data.news.unread_keys=[...keys];state.data.news.important_unread=keys.length;
 if(success)state.data.news.last_success=new Date(Date.now()).toISOString();
 await emit('snapshot-changed');
};
state.visibility=async(shown)=>{state.shown=shown;await emit('window-visible',shown)};
state.mode=async(mode)=>{state.view.mode=mode;await emit('window-mode',[mode,null])};
await import('/src/main.ts');
`.replaceAll('__TAURI_API__', api);
(async () => {
  const browser = await chromium.launch({
    headless: true,
    ...(process.env.CODEXPULSE_BROWSER_CHANNEL
      ? { channel: process.env.CODEXPULSE_BROWSER_CHANNEL }
      : {}),
  });
  try {
    for (const language of ['zh', 'en']) {
      const page = await browser.newPage({ viewport: { width: 380, height: 800 } });
      page.setDefaultTimeout(15000);
      const errors = [];
      page.on('pageerror', (e) => {
        errors.push(e.message);
        console.error('PAGE ERROR', e.message);
      });
      await page.route('**/__panel_test__.js', (route) =>
        route.fulfill({ contentType: 'text/javascript', body: boot }),
      );
      await page.route(
        (url) => url.pathname === '/' && url.searchParams.has('language'),
        async (route) => {
          const response = await route.fetch();
          const html = (await response.text()).replace(
            /src="\/src\/main\.ts(?:\?[^"]*)?"/g,
            'src="/__panel_test__.js"',
          );
          await route.fulfill({ response, body: html });
        },
      );
      await page.goto(new URL('?language=' + language, base).href);
      await page
        .getByText('Build a personal dashboard', { exact: true })
        .waitFor()
        .catch(async (e) => {
          console.error(await page.locator('body').innerText());
          console.error(
            await page.evaluate(() => ({
              test: !!window.__panelTest,
              internals: !!window.__TAURI_INTERNALS__,
              data: !!window.__panelTest?.data,
            })),
          );
          throw e;
        });
      await page.waitForTimeout(150);
      assert.equal(
        await page.evaluate(() => __panelTest.reads.length),
        0,
        'Overview must not mark news read',
      );
      await page.locator('.cp-tabs button').filter({ hasText: 'Tibo' }).click();
      await page.waitForFunction(
        () => __panelTest.reads.length === 1 && __panelTest.data.news.unread_keys.length === 0,
      );
      assert.equal(
        await page.getByRole('button', { name: /标为已读|全部已读|Mark.*as read/ }).count(),
        0,
      );
      assert.equal(
        await page
          .getByRole('button', { name: language === 'zh' ? '翻译' : /Translate/, exact: false })
          .count(),
        1,
      );
      await page.evaluate(() => __panelTest.push(['new-reset', 'new-challenge']));
      await page.waitForFunction(
        () => __panelTest.reads.length === 2 && __panelTest.data.news.unread_keys.length === 0,
      );
      await page.evaluate(async () => {
        await __panelTest.visibility(false);
        await __panelTest.push(['hidden-news']);
      });
      await page.waitForTimeout(120);
      assert.equal(
        await page.evaluate(() => __panelTest.reads.length),
        2,
        'Hidden panel must retain unread news',
      );
      await page.evaluate(() => __panelTest.visibility(true));
      await page.waitForFunction(
        () => __panelTest.reads.length === 3 && __panelTest.data.news.unread_keys.length === 0,
      );
      await page.locator('.cp-tabs button').first().click();
      await page.evaluate(() => __panelTest.push(['other-page-news']));
      await page.waitForTimeout(100);
      assert.equal(await page.evaluate(() => __panelTest.reads.length), 3);
      await page.locator('.cp-tabs button').filter({ hasText: 'Tibo' }).click();
      await page.waitForFunction(
        () => __panelTest.reads.length === 4 && __panelTest.data.news.unread_keys.length === 0,
      );
      await page.evaluate(async () => {
        __panelTest.hold = true;
        await __panelTest.push(['pending-a']);
      });
      await page.waitForFunction(() => __panelTest.reads.length === 5 && __panelTest.release);
      await page.evaluate(() => __panelTest.push(['pending-a', 'pending-b']));
      await page.waitForTimeout(100);
      assert.equal(
        await page.evaluate(() => __panelTest.reads.length),
        5,
        'Pending reads must not overlap',
      );
      await page.evaluate(() => {
        __panelTest.hold = false;
        __panelTest.release();
      });
      await page.waitForFunction(
        () => __panelTest.reads.length === 6 && __panelTest.data.news.unread_keys.length === 0,
      );
      assert.deepEqual(await page.evaluate(() => __panelTest.reads[5]), ['pending-b']);
      await page.evaluate(async () => {
        __panelTest.fail = true;
        await __panelTest.push(['save-failure']);
      });
      await page.waitForFunction(() => __panelTest.reads.length === 7);
      await page.waitForTimeout(150);
      assert.deepEqual(await page.evaluate(() => __panelTest.data.news.unread_keys), [
        'save-failure',
      ]);
      assert.equal(
        await page.evaluate(() => __panelTest.reads.length),
        7,
        'Save failure must not spin',
      );
      await page.evaluate(async () => {
        __panelTest.fail = false;
        await __panelTest.push(['save-failure'], true);
      });
      await page.waitForFunction(
        () => __panelTest.reads.length === 8 && __panelTest.data.news.unread_keys.length === 0,
      );
      await page.evaluate(async () => {
        await __panelTest.mode('compact');
        await __panelTest.push(['compact-news']);
      });
      await page.locator('.cp-compact-line').waitFor();
      await page.waitForTimeout(100);
      assert.equal(
        await page.evaluate(() => __panelTest.reads.length),
        8,
        'Compact window must retain unread news',
      );
      await page.locator('.cp-compact-info').click({ button: 'right' });
      await page.waitForFunction(() => __panelTest.popups.length === 1);
      const expected = language === 'zh' ? ['展开', '退出', '隐藏'] : ['Expand', 'Quit', 'Hide'];
      assert.deepEqual(
        await page.evaluate(() =>
          __panelTest.menus.filter((m) => m.kind === 'MenuItem').map((m) => m.options.text),
        ),
        expected,
      );
      await page.locator('.cp-compact-info').click({ button: 'right' });
      await page.waitForFunction(() => __panelTest.popups.length === 2);
      assert.equal(
        await page.evaluate(() => __panelTest.menus.length),
        4,
        'Native menu must be reused',
      );
      await page.evaluate(() =>
        __panelTest.menus
          .find((m) => m.options.id === 'compact-expand')
          .handler.onmessage('compact-expand'),
      );
      await page.waitForFunction(
        () => __panelTest.actions.includes('expand') && __panelTest.reads.length === 9,
      );
      await page.evaluate(() => __panelTest.mode('compact'));
      await page.locator('.cp-compact-line').waitFor();
      await page.evaluate(() =>
        __panelTest.menus
          .find((m) => m.options.id === 'compact-hide')
          .handler.onmessage('compact-hide'),
      );
      await page.waitForFunction(() => !__panelTest.shown);
      await page.evaluate(() =>
        __panelTest.menus
          .find((m) => m.options.id === 'compact-exit')
          .handler.onmessage('compact-exit'),
      );
      assert.deepEqual(await page.evaluate(() => __panelTest.actions), ['expand', 'hide', 'exit']);
      assert.deepEqual(errors, []);
      console.log(
        language +
          ': auto-read visible Tibo only, new messages, single-flight, failed writes, compact menu labels/actions/reuse passed',
      );
      await page.close();
    }
  } finally {
    await browser.close();
  }
})().catch((e) => {
  console.error(e);
  process.exitCode = 1;
});
