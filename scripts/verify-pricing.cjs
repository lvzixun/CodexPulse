// Real Svelte UI with Tauri IPC mocks; synthetic usage only.
const { chromium } = require(process.env.CODEXPULSE_PLAYWRIGHT_MODULE || 'playwright');
const assert = require('node:assert/strict');
const path = require('node:path');
const fs = require('node:fs');
const base = process.env.CODEXPULSE_PREVIEW_URL || 'http://127.0.0.1:1430/';
if (!['127.0.0.1', 'localhost'].includes(new URL(base).hostname))
  throw Error('Use a local dev server');
const api = '/@fs/' + path.resolve(__dirname, '../apps/desktop/node_modules/@tauri-apps/api');
const boot = String.raw`
import {mockIPC,mockWindows} from '__API__/mocks.js';
window.isTauri=true;mockWindows('pulse');
const {empty}=await import('/src/lib/ipc.ts');
const {previewSnapshot,previewView,previewModels,previewModelSpeed}=await import('/src/lib/readme-preview.ts');
const params=new URLSearchParams(location.search);
const state=window.__pricingTest={data:previewSnapshot(empty),mapped:false,calls:[]};
state.data.settings.theme=params.get('theme');
state.data.usage.reference={price_date:'2026-11-03',cost_nanousd:1300e9,max_cost_nanousd:1900e9,unpriced_tokens:1e6,unpriced_events:3,pending:false,estimate_flags:1|2|4|16,reasons:['unknown_model_id']};
state.data.pricing={syncing:false,error:true,checked_at:null,aliases:{},models:['gpt-6.1-sol','deepseek-flash'],dates:{openai:'2026-11-03',deepseek:'2026-11-03'}};
let emit;
mockIPC(async(cmd,args)=>{
 if(cmd==='get_ui_language')return params.get('language');
 if(cmd==='get_snapshot')return structuredClone(state.data);
 if(cmd==='get_view_state')return {...previewView(),mode:'details',page:'models',floating_supported:params.get('platform')==='windows'};
 if(cmd==='remember_view')return;
 if(cmd==='get_startup_status')return 'disabled';
 if(cmd==='get_app_update_info')return {current_version:'0.1.41',status:'up_to_date',revision:1};
 if(cmd==='plugin:window|is_visible')return true;
 if(cmd==='get_model_speed')return previewModelSpeed(args.request);
 if(cmd==='get_model_page'){
  const page=previewModels(state.data);
  page.items.forEach((item,i)=>{
   item.model=['gpt-6.1-sol','deepseek-v4.1-flash','mode-test'][i];
   item.reference={...state.data.usage.reference,cost_nanousd:i===2?(state.mapped?5e9:0):100e9,max_cost_nanousd:i===2?(state.mapped?10e9:0):200e9,unpriced_events:i===2&&!state.mapped?item.events:0,reasons:i===2&&!state.mapped?['unknown_model_id']:[],estimate_flags:i===1?16:1|4};
  });return page;
 }
 if(cmd==='set_price_alias'){
  state.calls.push(args);state.mapped=!!args.model;
  if(args.model)state.data.pricing.aliases[args.alias]=args.model;else delete state.data.pricing.aliases[args.alias];
  await state.push();return;
 }
 throw Error('Unexpected mock IPC: '+cmd);
},{shouldMockEvents:true});
({emit}=await import('__API__/event.js'));
state.push=async()=>{state.data.updated_at=new Date(Date.parse(state.data.updated_at)+1).toISOString();await emit('snapshot-changed')};
await import('/src/main.ts');
`.replaceAll('__API__', api.replaceAll('\\', '/'));
(async () => {
  const browser = await chromium.launch({
    headless: true,
    channel: process.env.CODEXPULSE_BROWSER_CHANNEL || 'chrome',
  });
  let cases = 0;
  try {
    for (const width of [320, 380])
      for (const platform of ['windows', 'macos'])
        for (const language of ['zh', 'en'])
          for (const theme of ['light', 'dark']) {
            const page = await browser.newPage({ viewport: { width, height: 1000 } });
            const errors = [];
            page.on('pageerror', (e) => errors.push(e.message));
            await page.route('**/__pricing_test__.js', (route) =>
              route.fulfill({ contentType: 'text/javascript', body: boot }),
            );
            await page.route(
              (url) => url.pathname === '/' && url.searchParams.has('language'),
              async (route) => {
                const response = await route.fetch();
                await route.fulfill({
                  response,
                  body: (await response.text()).replace(
                    /src="\/src\/main\.ts(?:\?[^\"]*)?"/g,
                    'src="/__pricing_test__.js"',
                  ),
                });
              },
            );
            await page.goto(
              new URL(`?language=${language}&platform=${platform}&theme=${theme}`, base).href,
            );
            await page.locator('.cp-modelrow').first().waitFor();
            assert.match(await page.locator('.cp-total b').innerText(), /\$1,300.00–\$1,900.00\*/);
            const boxes = await page
              .locator('.cp-total')
              .evaluate((el) => ({
                left: el.querySelector('strong').getBoundingClientRect().right,
                right: el.querySelector('b').getBoundingClientRect().left,
              }));
            assert.ok(boxes.left <= boxes.right + 1, 'Token total and cost range must not overlap');
            if (width === 320 && language === 'zh' && theme === 'dark' && platform === 'windows') {
              fs.mkdirSync('work/pricing-0.1.41', { recursive: true });
              await page.screenshot({ path: 'work/pricing-0.1.41/summary.png', fullPage: true });
            }
            const row = page.locator('.cp-modelrow').filter({ hasText: 'mode-test' });
            assert.match(
              await row.innerText(),
              language === 'zh' ? /模型标识未关联/ : /Model ID not mapped/,
            );
            await row.click();
            const editor = page.locator('.cp-price-alias');
            await editor.locator('input').fill('gpt-6.1-sol');
            await editor
              .getByRole('button', {
                name: language === 'zh' ? '保存关联' : 'Save mapping',
                exact: true,
              })
              .click();
            await page.waitForFunction(
              () => window.__pricingTest.data.pricing.aliases['mode-test'] === 'gpt-6.1-sol',
            );
            await page.waitForFunction(() =>
              [...document.querySelectorAll('.cp-modelrow')]
                .find((el) => el.textContent.includes('mode-test'))
                ?.textContent.includes('$5.00'),
            );
            assert.match(await row.innerText(), /mode-test/);
            await editor
              .getByRole('button', {
                name: language === 'zh' ? '取消关联' : 'Remove mapping',
                exact: true,
              })
              .click();
            await page.waitForFunction(
              () => !window.__pricingTest.data.pricing.aliases['mode-test'],
            );
            assert.equal(
              await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth),
              true,
              'No horizontal overflow',
            );
            if (language === 'en') assert.doesNotMatch(await editor.innerText(), /\p{Script=Han}/u);
            assert.deepEqual(errors, []);
            if (width === 380 && language === 'zh' && theme === 'dark' && platform === 'windows') {
              fs.mkdirSync('work/pricing-0.1.41', { recursive: true });
              await page.screenshot({ path: 'work/pricing-0.1.41/models.png', fullPage: true });
            }
            await page.close();
            cases++;
          }
    console.log(
      `Passed ${cases} pricing UI cases (platform, language, theme, width, mapping save/remove)`,
    );
  } finally {
    await browser.close();
  }
})().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
