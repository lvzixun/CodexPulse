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
const state=window.__panelTest={data:null,view:null,shown:true,reads:[],actions:[],menus:[],popups:[],closed:[],fail:false,hold:false,release:null,checks:[],cacheReads:0,upgrades:0,installs:0,updateRevision:0,drags:0};
let emit;
mockIPC(async(cmd,args)=>{
 if(cmd==='get_ui_language')return new URLSearchParams(location.search).get('language')||'zh';
 if(cmd==='get_snapshot')return structuredClone(state.data);
 if(cmd==='get_view_state')return structuredClone(state.view);
 if(cmd==='remember_view')return;
 if(cmd==='get_startup_status')return 'disabled';
 if(cmd==='check_updates'){
  state.checks.push('manual');
  return state.cached=state.updateInfo('available');
 }
 if(cmd==='get_app_update_info'){state.cacheReads++;return state.cached}
 if(cmd==='install_app_update'){
  state.installs++;
  if(state.installs===1){await state.update('ready');throw Error('Installation failed')}
  await state.update('installing');
  await new Promise(resolve=>state.finishInstall=resolve);
  await state.update('ready');
  return;
 }
 if(cmd==='open_app_release'){state.upgrades++;return}
 if(cmd==='plugin:window|is_visible')return state.shown;
 if(cmd==='plugin:window|start_dragging'){state.drags++;return}
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
state.updateInfo=(status)=>({current_version:'0.1.14',latest_version:'0.1.15',update_available:true,checked_at:new Date().toISOString(),next_check_at:Date.now()/1000+86400,status,revision:++state.updateRevision,downloaded_bytes:25,total_bytes:100});
// The Rust startup check has completed before this WebView is created.
state.cached=state.updateInfo('available');
state.update=async(status)=>emit('app-update',state.cached=state.updateInfo(status));
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
      const header = await page.locator('.cp-header').boundingBox();
      await page.mouse.move(header.x + 3, header.y + 3);
      await page.mouse.down();
      await page.mouse.up();
      await page.locator('.cp-brand strong').click();
      await page.waitForFunction(() => __panelTest.drags === 2);
      await page.locator('.app-update-banner').waitFor();
      assert.deepEqual(
        await page.evaluate(() => __panelTest.checks),
        [],
        'Mount must only read the Rust startup result',
      );
      await page.locator('.app-update-banner button').first().click();
      await page.waitForFunction(() => __panelTest.upgrades === 1);
      await page.locator('.app-update-banner button').last().click();
      assert.equal(await page.locator('.app-update-banner').count(), 0);
      await page
        .getByRole('button', {
          name: language === 'zh' ? '打开设置' : 'Open settings',
          exact: true,
        })
        .click();
      await page
        .getByRole('button', {
          name: language === 'zh' ? '检查更新' : 'Check for updates',
          exact: true,
        })
        .click();
      await page.waitForFunction(() => __panelTest.checks.length === 1);
      assert.deepEqual(await page.evaluate(() => __panelTest.checks), ['manual']);
      await page.getByRole('button', { name: /查看发布说明|View release notes/ }).click();
      await page.waitForFunction(() => __panelTest.upgrades === 2);
      await page.evaluate(() => __panelTest.update('downloading'));
      await page.getByText(/正在下载更新 25%|Downloading update 25%/).waitFor();
      assert.equal(
        await page.getByRole('button', { name: /检查更新|Check for updates/ }).isDisabled(),
        true,
      );
      assert.equal(
        await page.getByRole('button', { name: /重启并更新|Restart and update/ }).count(),
        0,
      );
      await page.evaluate(() => __panelTest.update('ready'));
      await page.getByRole('button', { name: /重启并更新|Restart and update/ }).click();
      await page.getByText('Installation failed', { exact: true }).waitFor();
      await page.getByRole('button', { name: /重启并更新|Restart and update/ }).click();
      await page.getByText(/正在安装，即将重新启动…|Installing and restarting…/).waitFor();
      assert.equal(await page.evaluate(() => __panelTest.installs), 2);
      assert.equal(
        await page.getByRole('button', { name: /重启并更新|Restart and update/ }).count(),
        0,
      );
      await page.evaluate(() => __panelTest.finishInstall());
      await page.getByRole('button', { name: /重启并更新|Restart and update/ }).waitFor();
      assert.equal(
        await page.evaluate(() => __panelTest.drags),
        2,
        'Header controls must not start dragging',
      );
      await page.locator('.cp-tabs button').first().click();
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
      const info = await page.locator('.cp-compact-info').boundingBox();
      await page.mouse.move(info.x + 15, info.y + 15);
      await page.mouse.down();
      await page.mouse.move(info.x + 17, info.y + 15);
      assert.equal(
        await page.evaluate(() => __panelTest.drags),
        2,
        'Click jitter must not start dragging',
      );
      await page.mouse.move(info.x + 24, info.y + 15);
      await page.waitForFunction(() => __panelTest.drags === 3);
      await page.mouse.up();
      assert.deepEqual(
        await page.evaluate(() => __panelTest.actions),
        [],
        'Dragging model/speed must not expand the window',
      );
      await page.locator('.cp-compact-info').click();
      await page.waitForFunction(() => __panelTest.actions.length === 1);
      assert.deepEqual(
        await page.evaluate(() => __panelTest.actions),
        ['news'],
        'The next intentional click must open the unread messages',
      );
      await page.evaluate(() => (__panelTest.actions.length = 0));
      const strip = await page.locator('.cp-compact-line').boundingBox();
      await page.mouse.move(strip.x + 2, strip.y + 18);
      await page.mouse.down();
      await page.mouse.move(strip.x - 20, strip.y + 18);
      await page.waitForFunction(() => __panelTest.drags === 4);
      await page.mouse.up();
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
      assert.deepEqual(
        await page.evaluate(() => __panelTest.checks),
        ['manual'],
        'Compact/hidden/reopening must reuse the update cache',
      );
      await page.clock.install();
      // Jump wall time without expiring Vite's WebSocket heartbeat/reloading.
      await page.clock.setSystemTime(new Date(Date.now() + 25 * 60 * 60 * 1000));
      await page.evaluate(() => __panelTest.visibility(true));
      await page.evaluate(() => __panelTest.mode('details'));
      await page.locator('.cp-tabs').waitFor();
      await page.clock.runFor(61000);
      assert.deepEqual(
        await page.evaluate(() => __panelTest.checks),
        ['manual'],
        'Elapsed time and reopening must never make an update request',
      );
      assert.ok(await page.evaluate(() => __panelTest.cacheReads > 1));
      // Same upcoming plan in Overview and Tibo, including expiry with no fetch.
      await page.evaluate(async () => {
        const now = Date.now();
        __panelTest.data.settings.timezone = 'Asia/Shanghai';
        __panelTest.data.news.latest_reset.occurred_at = new Date(now - 21 * 3600000).toISOString();
        __panelTest.data.news.scheduled_reset = {
          ...__panelTest.data.news.latest_reset,
          id: 'example-next-reset',
          kind: 'scheduled',
          occurred_at: new Date(now - 3600000).toISOString(),
          scheduled_for: new Date(now + 5.5 * 3600000).toISOString(),
        };
        await __panelTest.push([]);
      });
      for (const tab of ['overview', 'news']) {
        await page
          .locator('.cp-tabs button')
          .nth(tab === 'overview' ? 0 : 3)
          .click();
        const reminder = page.locator('.cp-reset-plan');
        await reminder.waitFor();
        assert.equal(await reminder.count(), 1);
        if (tab === 'overview') {
          assert.equal(await page.locator('.cp-reset-status > small').count(), 0);
          const frame = await page.locator('.cp-reset-status').evaluate((el) => {
            const css = getComputedStyle(el);
            return {
              border: css.borderTopWidth,
              background: css.backgroundColor,
              padding: css.paddingTop,
            };
          });
          assert.deepEqual(
            frame,
            { border: '0px', background: 'rgba(0, 0, 0, 0)', padding: '0px' },
            'Only the upcoming reminder should have a visible frame',
          );
        }
        assert.match(
          await reminder.innerText(),
          language === 'zh' ? /5 小时 30 分钟后重置/ : /Reset in 5h 30m/,
        );
        const expectedDate = await page.evaluate(
          (language) =>
            new Date(__panelTest.data.news.scheduled_reset.scheduled_for).toLocaleString(
              language === 'zh' ? 'zh-CN' : 'en-US',
              {
                timeZone: 'Asia/Shanghai',
                year: 'numeric',
                month: 'numeric',
                day: 'numeric',
                hour: '2-digit',
                minute: '2-digit',
              },
            ),
          language,
        );
        assert.ok((await reminder.locator('.cp-plan-time').innerText()).includes(expectedDate));
        for (const theme of ['dark', 'light']) {
          await page.locator('main').evaluate((el, theme) => (el.dataset.theme = theme), theme);
          await reminder.scrollIntoViewIfNeeded();
          await page.screenshot({ path: `work/reset-reminder-${language}-${tab}-${theme}.png` });
          for (const width of [320, 380]) {
            await page.setViewportSize({ width, height: 800 });
            assert.ok(
              await reminder.evaluate(
                (el) =>
                  el.scrollWidth <= el.clientWidth &&
                  [...el.querySelectorAll('p,strong,span')].every(
                    (child) => child.scrollWidth <= child.clientWidth,
                  ),
              ),
              `${language}/${tab}/${theme}/${width}: reminder must not clip text`,
            );
          }
        }
      }
      await page.evaluate(async () => {
        __panelTest.data.news.status = 'network_error';
        await __panelTest.push([]);
      });
      await page
        .locator('.cp-plan-caveat')
        .getByText(/离线缓存|Offline cache/)
        .waitFor();
      const deadline = await page.evaluate(
        () => __panelTest.data.news.scheduled_reset.scheduled_for,
      );
      await page.clock.setSystemTime(new Date(deadline));
      await page.clock.runFor(1100);
      assert.equal(await page.locator('.cp-reset-plan').count(), 0);
      await page
        .getByText(/计划时间已过，待确认执行|Scheduled time passed; awaiting confirmation/)
        .waitFor();
      await page.locator('.cp-tabs button').first().click();
      assert.equal(await page.locator('.cp-reset-plan').count(), 0);
      await page
        .getByText(/计划时间已过，待确认执行|Scheduled time passed; awaiting confirmation/)
        .waitFor();
      await page.evaluate(async () => {
        __panelTest.data.news.latest_reset.occurred_at =
          __panelTest.data.news.scheduled_reset.scheduled_for;
        await __panelTest.push([]);
      });
      await page
        .locator('.cp-reset-plan-status')
        .getByText(/尚未公布|Not announced/)
        .waitFor();
      await page.locator('.cp-tabs button').last().click();
      assert.equal(await page.locator('.cp-reset-plan-status').count(), 0);
      assert.deepEqual(await page.evaluate(() => __panelTest.checks), ['manual']);
      assert.deepEqual(errors, []);
      console.log(
        language +
          ': compact/header drag, update cache, Tibo read, menus, upcoming reset in both tabs/themes, offline cache and deadline expiry passed',
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
