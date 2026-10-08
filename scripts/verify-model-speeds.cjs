// Regression against the real Svelte UI using Tauri's official IPC mocks.
const { chromium } = require(process.env.CODEXPULSE_PLAYWRIGHT_MODULE || 'playwright');
const assert = require('node:assert/strict');
const path = require('node:path');
const base = process.env.CODEXPULSE_PREVIEW_URL || 'http://127.0.0.1:1430/';
if (!['127.0.0.1', 'localhost'].includes(new URL(base).hostname))
  throw Error('Use a local dev server');
const api =
  '/@fs/' +
  path.resolve(__dirname, '../apps/desktop/node_modules/@tauri-apps/api').replaceAll('\\', '/');
const boot = String.raw`
import {mockIPC,mockWindows} from '__API__/mocks.js';
window.isTauri=true;mockWindows('pulse');
const {empty}=await import('/src/lib/ipc.ts');
const {previewSnapshot,previewView,previewModels,previewModelSpeed}=await import('/src/lib/readme-preview.ts');
const state=window.__speedTest={data:previewSnapshot(empty),fail:false,none:false,requests:[]};
const view={...previewView(),mode:'details',page:'models'};
let emit;
mockIPC(async(cmd,args)=>{
 if(cmd==='get_ui_language')return new URLSearchParams(location.search).get('language');
 if(cmd==='get_snapshot')return structuredClone(state.data);
 if(cmd==='get_view_state')return view;
 if(cmd==='get_startup_status')return 'disabled';
 if(cmd==='get_app_update_info')return {current_version:'0.1.23',status:'up_to_date',revision:1};
 if(cmd==='remember_view')return;
 if(cmd==='plugin:window|is_visible')return true;
 if(cmd==='get_model_page')return previewModels(state.data);
 if(cmd==='get_model_speed'){
  state.requests.push({...args.request});
  if(state.fail)throw Error('Cannot read run speeds. Please try again.');
  return state.none?{output_tokens:0,elapsed_ms:0,samples:0,service_tier:null,points:[],current:null,history_pending:false}:previewModelSpeed(args.request);
 }
 throw Error('Unexpected mock IPC: '+cmd);
},{shouldMockEvents:true});
({emit}=await import('__API__/event.js'));
state.push=async()=>{state.data.updated_at=new Date().toISOString();await emit('snapshot-changed')};
await import('/src/main.ts');
`.replaceAll('__API__', api);
(async () => {
  const browser = await chromium.launch({
    headless: true,
    ...(process.env.CODEXPULSE_BROWSER_CHANNEL
      ? { channel: process.env.CODEXPULSE_BROWSER_CHANNEL }
      : {}),
  });
  try {
    for (const width of [380, 320])
      for (const language of ['zh', 'en']) {
        const page = await browser.newPage({
          viewport: { width, height: 980 },
          locale: language === 'zh' ? 'zh-CN' : 'en-US',
          timezoneId: 'Asia/Shanghai',
        });
        page.setDefaultTimeout(15000);
        const errors = [];
        page.on('pageerror', (e) => errors.push(e.message));
        await page.clock.install({ time: new Date('2026-10-08T13:25:00Z') });
        await page.route('**/__speed_test__.js', (route) =>
          route.fulfill({ contentType: 'text/javascript', body: boot }),
        );
        await page.route(
          (url) => url.pathname === '/' && url.searchParams.has('language'),
          async (route) => {
            const response = await route.fetch();
            const html = (await response.text()).replace(
              /src="\/src\/main\.ts(?:\?[^"]*)?"/g,
              'src="/__speed_test__.js"',
            );
            await route.fulfill({ response, body: html });
          },
        );
        await page.goto(new URL('?language=' + language, base).href);
        await page.locator('.cp-modelrow').first().click();
        await page.locator('.cp-speed-chart').waitFor();
        assert.equal(await page.locator('.cp-speed-chart circle').count(), 50);
        if (language === 'en')
          assert.doesNotMatch(await page.locator('.cp-speed').innerText(), /\p{Script=Han}/u);
        const ticks = await page.locator('.cp-speed-chart text').allTextContents();
        assert.ok(ticks.includes('09:24'));
        assert.ok(ticks.includes('21:24'));
        await page.locator('.cp-speed-current').hover();
        await page.locator('.cp-speed-tooltip').waitFor();
        assert.match(await page.locator('.cp-speed-tooltip').innerText(), /Fast/);
        await page.locator('.cp-speed-head').hover();
        await page.locator('.cp-speed select').selectOption('recent100');
        await page.waitForFunction(
          () => document.querySelectorAll('.cp-speed-chart circle').length === 100,
        );
        await page.locator('.cp-speed select').selectOption('month');
        await page.waitForFunction(
          () => document.querySelectorAll('.cp-speed-chart circle').length === 120,
        );
        await page.locator('.cp-speed-chart circle').first().focus();
        assert.match(
          await page.locator('.cp-speed-tooltip').innerText(),
          language === 'zh' ? /标准/ : /Standard/,
        );
        await page.locator('.cp-speed-chart circle').first().blur();
        for (const theme of ['dark', 'light']) {
          await page.locator('main').evaluate((el, theme) => (el.dataset.theme = theme), theme);
          const overflow = await page.evaluate(() =>
            [...document.querySelectorAll('.cp-speed,.cp-modelrow,.cp-model-detail')].some(
              (el) => el.scrollWidth > el.clientWidth + 1,
            ),
          );
          assert.equal(overflow, false, `${width} ${language} ${theme}`);
          await page.locator('.cp-speed-current').hover();
          const contrast = await page.locator('.cp-speed-tooltip').evaluate((el) => ({
            bg: getComputedStyle(el).backgroundColor,
            fg: getComputedStyle(el).color,
          }));
          assert.notEqual(contrast.bg, contrast.fg);
        }
        await page.evaluate(() => (window.__speedTest.fail = true));
        await page.locator('.cp-speed select').selectOption('recent50');
        await page.locator('.cp-speed [role="alert"]').waitFor();
        assert.equal(await page.locator('.cp-speed-chart circle').count(), 120); // Keep the last successful curve on failure.
        await page.evaluate(() => (window.__speedTest.fail = false));
        await page.locator('.cp-speed [role="alert"] button').click();
        await page.waitForFunction(
          () => document.querySelectorAll('.cp-speed-chart circle').length === 50,
        );
        const before = await page.evaluate(() => window.__speedTest.requests.length);
        await page.clock.fastForward(61000);
        await page.waitForFunction(() => !document.querySelector('.cp-speed-current'));
        assert.equal(await page.evaluate(() => window.__speedTest.requests.length), before); // Local clock expiry never requests the network.
        await page.evaluate(() => (window.__speedTest.none = true));
        await page.locator('.cp-speed select').selectOption('recent100');
        await page.waitForFunction(() => !document.querySelector('.cp-speed-chart'));
        assert.deepEqual(errors, []);
        console.log(
          `${language} ${width}px: modes, local time, ranges, themes, cached error/retry, sample expiry and empty state passed`,
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
