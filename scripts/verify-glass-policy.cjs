// Verify the actual Svelte controls with Tauri's official IPC mocks, not native drag latency.
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
const {previewSnapshot,previewView}=await import('/src/lib/readme-preview.ts');
const params=new URLSearchParams(location.search);
const available=params.get('platform')!=='win10';
const state=window.__glassTest={data:previewSnapshot(empty),saves:[]};
// Deliberately retain an enabled legacy setting, even on unsupported Windows.
state.data.settings.glass=true;
const view={...previewView(),mode:'details',page:'settings',glass_available:available,glass_supported:true,floating_supported:params.get('platform')!=='macos'};
let emit;
mockIPC(async(cmd,args)=>{
 if(cmd==='get_ui_language')return params.get('language');
 if(cmd==='get_snapshot')return structuredClone(state.data);
 if(cmd==='get_view_state')return view;
 if(cmd==='remember_view')return;
 if(cmd==='get_startup_status')return 'disabled';
 if(cmd==='get_app_update_info')return {current_version:'0.1.25',status:'up_to_date',revision:1};
 if(cmd==='plugin:window|is_visible')return true;
 if(cmd==='set_settings'){
  state.data.settings=JSON.parse(JSON.stringify(args.settings));
  state.saves.push(structuredClone(state.data.settings));
  await emit('settings-applied',state.data.settings);
  await emit('glass-supported',available&&state.data.settings.glass);
  return;
 }
 throw Error('Unexpected mock IPC: '+cmd);
},{shouldMockEvents:true});
({emit}=await import('__API__/event.js'));
state.push=async()=>emit('snapshot-changed');
state.mode=async(mode)=>emit('window-mode',[mode,null]);
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
    let cases = 0;
    for (const platform of ['win10', 'win11', 'macos'])
      for (const language of ['zh', 'en'])
        for (const width of [320, 380]) {
          const page = await browser.newPage({ viewport: { width, height: 980 } });
          const errors = [];
          page.on('pageerror', (error) => errors.push(error.message));
          await page.route('**/__glass_test__.js', (route) =>
            route.fulfill({ contentType: 'text/javascript', body: boot }),
          );
          await page.route(
            (url) => url.pathname === '/' && url.searchParams.has('platform'),
            async (route) => {
              const response = await route.fetch();
              const html = (await response.text()).replace(
                /src="\/src\/main\.ts(?:\?[^\"]*)?"/g,
                'src="/__glass_test__.js"',
              );
              await route.fulfill({ response, body: html });
            },
          );
          await page.goto(new URL(`?platform=${platform}&language=${language}`, base).href);
          const control = page.getByRole('switch', {
            name: language === 'zh' ? /毛玻璃效果/ : /Glass effect/,
          });
          await control.waitFor();
          const save = page.getByRole('button', {
            name: language === 'zh' ? '保存设置' : 'Save settings',
            exact: true,
          });
          if (platform === 'win10') {
            assert.equal(await control.isDisabled(), true);
            assert.equal(await control.isChecked(), false);
            await control.evaluate((el) => el.click());
            await page.evaluate(() => window.__glassTest.push());
            assert.equal(await control.isChecked(), false);
            const hint = await control.locator('..').innerText();
            assert.match(hint, language === 'zh' ? /当前系统已禁用毛玻璃/ : /Glass is disabled/);
            for (const theme of ['light', 'dark']) {
              await page.locator('main').evaluate((el, theme) => (el.dataset.theme = theme), theme);
              assert.equal(
                await page.locator('main').evaluate((el) => el.classList.contains('opaque')),
                true,
              );
              assert.match(
                await page.locator('main').evaluate((el) => getComputedStyle(el).backgroundColor),
                /^rgb\(/,
              );
            }
            await save.click();
            await page.waitForFunction(() => window.__glassTest.saves.length === 1);
            assert.equal(await page.evaluate(() => window.__glassTest.saves[0].glass), false);
            assert.equal(await control.isDisabled(), true);
            await page.evaluate(() => window.__glassTest.mode('compact'));
            await page.locator('main.compact.opaque').waitFor();
            await page.evaluate(() => window.__glassTest.mode('details'));
            await control.waitFor();
            if (language === 'zh' && width === 380) {
              await control.scrollIntoViewIfNeeded();
              await page.screenshot({ path: 'work/win10-glass-disabled.png' });
            }
          } else {
            assert.equal(await control.isDisabled(), false);
            assert.equal(await control.isChecked(), true);
            await control.uncheck();
            await save.click();
            await page.waitForFunction(() => window.__glassTest.saves.length === 1);
            assert.equal(await page.evaluate(() => window.__glassTest.saves[0].glass), false);
            // An inactive effect must not disable the ability to enable it again.
            assert.equal(await control.isDisabled(), false);
            await control.check();
            await save.click();
            await page.waitForFunction(() => window.__glassTest.saves.length === 2);
            assert.equal(await page.evaluate(() => window.__glassTest.saves[1].glass), true);
          }
          assert.deepEqual(errors, []);
          assert.equal(
            await page.locator('main').evaluate((el) => el.scrollWidth > el.clientWidth),
            false,
          );
          await page.close();
          cases++;
        }
    console.log(
      `${cases} glass policy UI cases passed: Windows 10 locked off; Windows 11/macOS can toggle off and on.`,
    );
  } finally {
    await browser.close();
  }
})().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
