// Exercise retained-window refresh and session disclosure against the real Svelte UI.
const { chromium } = require(process.env.CODEXPULSE_PLAYWRIGHT_MODULE || 'playwright');
const assert = require('node:assert/strict');
const path = require('node:path');
const base = process.env.CODEXPULSE_PREVIEW_URL || 'http://127.0.0.1:1430/';
if (!['127.0.0.1', 'localhost'].includes(new URL(base).hostname))
  throw Error('Use a local dev server');
const api = '/@fs/' + path.resolve(__dirname, '../apps/desktop/node_modules/@tauri-apps/api');
const boot = String.raw`
import {mockIPC,mockWindows} from '__API__/mocks.js';
window.isTauri=true;mockWindows('pulse');
const {empty}=await import('/src/lib/ipc.ts');
const {previewSnapshot,previewView,previewModels,previewModelSpeed,previewSessionDetail}=await import('/src/lib/readme-preview.ts');
const params=new URLSearchParams(location.search);
const state=window.__polishTest={data:previewSnapshot(empty),snapshots:0,reverse:false,hold:null,sessionReads:0};
let emit;
mockIPC(async(cmd,args)=>{
 if(cmd==='get_ui_language')return params.get('language');
 if(cmd==='get_snapshot'){state.snapshots++;return structuredClone(state.data)}
 if(cmd==='get_view_state')return {...previewView(),mode:'details',page:'models',floating_supported:params.get('platform')==='windows'};
 if(cmd==='remember_view')return;
 if(cmd==='get_startup_status')return 'disabled';
 if(cmd==='get_app_update_info')return {current_version:'0.1.25',status:'up_to_date',revision:1};
 if(cmd==='plugin:window|is_visible')return true;
 if(cmd==='get_model_page'){
  const result=previewModels(state.data);if(state.reverse)result.items.reverse();return result;
 }
 if(cmd==='get_model_speed')return previewModelSpeed(args.request);
 if(cmd==='get_session_page'){
  state.sessionReads++;if(state.hold)await state.hold;
  return {items:structuredClone(state.data.recent),older:null,newer:null};
 }
 if(cmd==='get_session_detail'){
  if(state.hold)await state.hold;
  return previewSessionDetail(state.data,args.request.id);
 }
 throw Error('Unexpected mock IPC: '+cmd);
},{shouldMockEvents:true});
({emit}=await import('__API__/event.js'));
state.push=async()=>{state.data.updated_at=new Date().toISOString();await emit('snapshot-changed')};
state.visibility=async(value)=>emit('window-visible',value);
await import('/src/main.ts');
`.replaceAll('__API__', api.replaceAll('\\', '/'));

(async () => {
  const browser = await chromium.launch({
    headless: true,
    channel: process.env.CODEXPULSE_BROWSER_CHANNEL || 'chrome',
  });
  try {
    for (const language of ['zh', 'en']) {
      const page = await browser.newPage({ viewport: { width: 380, height: 800 } });
      const errors = [];
      page.on('pageerror', (error) => errors.push(error.message));
      await page.route('**/__polish_test__.js', (route) =>
        route.fulfill({ contentType: 'text/javascript', body: boot }),
      );
      await page.route(
        (url) => url.pathname === '/' && url.searchParams.has('language'),
        async (route) => {
          const response = await route.fetch();
          const html = (await response.text()).replace(
            /src="\/src\/main\.ts(?:\?[^\"]*)?"/g,
            'src="/__polish_test__.js"',
          );
          await route.fulfill({ response, body: html });
        },
      );
      await page.goto(new URL(`?language=${language}&platform=macos`, base).href);
      await page.locator('.cp-modelrow').first().waitFor();
      const colors = () =>
        page
          .locator('.cp-modelrow')
          .evaluateAll((rows) =>
            Object.fromEntries(
              rows.map((row) => [
                row.querySelector('strong').textContent,
                getComputedStyle(row.querySelector('.cp-model-bar > span')).backgroundColor,
              ]),
            ),
          );
      const before = await colors();
      assert.equal(new Set(Object.values(before)).size, 3, 'Displayed models need distinct colors');
      await page.locator('.cp-modelrow').first().click();
      await page.locator('.cp-model-detail .cp-speed-chart').waitFor();
      assert.equal(await page.locator('.cp-modelrow[aria-expanded="true"]').count(), 1);
      await page.evaluate(async () => {
        __polishTest.reverse = true;
        await __polishTest.push();
      });
      await page.waitForFunction(
        () => document.querySelector('.cp-modelrow strong')?.textContent === 'gpt-6-luna',
      );
      assert.deepEqual(await colors(), before, 'Ranking changes must preserve each model color');
      await page.waitForFunction(
        () => !document.getAnimations().some((animation) => animation.playState === 'running'),
      );
      assert.ok(
        (await page.locator('.cp-model-detail').boundingBox()).height > 150,
        'Model disclosure must finish at its full content height',
      );
      await page.screenshot({ path: `work/ui-polish-models-${language}.png` });

      await page.emulateMedia({ reducedMotion: 'reduce' });
      await page.locator('.cp-modelrow[aria-expanded="true"]').click();
      await page.waitForFunction(() => !document.querySelector('.cp-model-detail'));
      await page.locator('.cp-modelrow').first().click();
      await page.locator('.cp-model-detail').waitFor();
      assert.equal(
        await page.evaluate(() =>
          document
            .getAnimations()
            .some(
              (animation) =>
                animation.effect?.target?.closest?.('[id^="model-detail-"]') &&
                Number(animation.effect.getTiming().duration) > 0,
            ),
        ),
        false,
      );

      await page.getByRole('button', { name: 'Sessions', exact: true }).click();
      await page.locator('.cp-sessionrow').first().click();
      const detail = page.locator('.cp-session-detail');
      await detail.getByText('Session tokens', { exact: true }).waitFor();
      assert.equal(
        await detail.locator('details, summary').count(),
        0,
        'Session detail needs no nested disclosure',
      );
      assert.equal(await detail.locator('.cp-id, .cp-price, .cp-session-model').count(), 0);
      assert.equal(await detail.locator('.cp-session-breakdown > span').count(), 3);
      assert.equal(await detail.locator(':scope > .cp-metrics > div').count(), 3);
      const bounds = await detail.boundingBox();
      assert.ok(bounds.height < 200, 'Default session expansion should remain compact');
      await page.screenshot({ path: `work/ui-polish-session-${language}.png` });
      assert.equal(
        await detail
          .getByRole('button', { name: /模型首页|Model first page|下一页模型|Next model page/ })
          .count(),
        0,
      );
      await page.evaluate(() => {
        __polishTest.hold = new Promise((resolve) => (__polishTest.release = resolve));
      });
      const reads = await page.evaluate(() => __polishTest.sessionReads);
      await page.evaluate(() => __polishTest.push());
      await page.waitForFunction((reads) => __polishTest.sessionReads > reads, reads);
      assert.equal(await page.locator('.cp-session-list').getAttribute('aria-busy'), 'false');
      assert.equal(await page.locator('.cp-sessionrow[aria-expanded="true"]').count(), 1);
      assert.equal(await detail.locator('.cp-session-breakdown').isVisible(), true);
      await page.evaluate(() => {
        __polishTest.hold = null;
        __polishTest.release();
      });

      await page.evaluate(() => __polishTest.visibility(false));
      await page.waitForTimeout(200);
      const snapshots = await page.evaluate(() => __polishTest.snapshots);
      await page.evaluate(() => __polishTest.push());
      await page.waitForTimeout(250);
      assert.equal(
        await page.evaluate(() => __polishTest.snapshots),
        snapshots,
        'Native hiding must pause snapshot reads even when document.hidden is false',
      );
      await page.evaluate(() => __polishTest.visibility(true));
      await page.waitForFunction((count) => __polishTest.snapshots > count, snapshots);
      assert.equal(
        await detail.locator('.cp-session-breakdown').isVisible(),
        true,
        'Reopening must retain disclosure state',
      );

      await page
        .getByRole('button', {
          name: language === 'zh' ? '打开设置' : 'Open settings',
          exact: true,
        })
        .click();
      await page
        .getByRole('switch', { name: language === 'zh' ? /毛玻璃效果/ : /Glass effect/ })
        .waitFor();
      for (const theme of ['light', 'dark']) {
        await page.locator('main').evaluate((el, theme) => {
          el.dataset.theme = theme;
        }, theme);
        await page.screenshot({ path: `work/ui-polish-settings-${theme}-${language}.png` });
      }
      assert.equal(
        await page.locator('main').evaluate((el) => el.scrollWidth > el.clientWidth),
        false,
      );
      assert.deepEqual(errors, []);
      await page.close();
      console.log(
        `${language}: model colors stable across reorder; compact session disclosure, retained state, hidden refresh suspension and reduced motion passed`,
      );
    }
  } finally {
    await browser.close();
  }
})().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
