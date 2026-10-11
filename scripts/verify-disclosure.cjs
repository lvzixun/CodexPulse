// Real Svelte disclosure with delayed local IPC: geometry, interruptions and cleanup.
const { chromium } = require(process.env.CODEXPULSE_PLAYWRIGHT_MODULE || 'playwright');
const assert = require('node:assert/strict');
const path = require('node:path');
const fs = require('node:fs');
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
const {previewSnapshot,previewView,previewModels,previewModelSpeed,previewSessionDetail}=await import('/src/lib/readme-preview.ts');
const params=new URLSearchParams(location.search);
const state=window.__disclosureTest={data:previewSnapshot(empty),delay:Number(params.get('delay')),fail:false,empty:false,observers:0,writes:[],frames:[]};
const NativeObserver=ResizeObserver;
window.ResizeObserver=class extends NativeObserver{
 tracked=false;
 observe(target,...args){if(target.classList.contains('cp-disclosure-content')&&!this.tracked){state.observers++;this.tracked=true;}super.observe(target,...args);}
 disconnect(){if(this.tracked){state.observers--;this.tracked=false;}super.disconnect();}
};
const scroll=Object.getOwnPropertyDescriptor(Element.prototype,'scrollTop');
Object.defineProperty(Element.prototype,'scrollTop',{...scroll,set(value){if(this.classList?.contains('cp-body'))state.writes.push(value);return scroll.set.call(this,value);}});
const pause=async()=>{await new Promise(r=>setTimeout(r,state.delay));if(state.fail)throw Error('Simulated local read failure');};
let emit;
mockIPC(async(cmd,args)=>{
 if(cmd==='get_ui_language')return params.get('language');
 if(cmd==='get_snapshot')return structuredClone(state.data);
 if(cmd==='get_view_state')return {...previewView(),mode:'details',page:'models',floating_supported:params.get('platform')==='windows'};
 if(cmd==='remember_view')return;
 if(cmd==='get_startup_status')return 'disabled';
 if(cmd==='get_app_update_info')return {current_version:'0.1.29',status:'up_to_date',revision:1};
 if(cmd==='read_news'){state.data.news.unread_keys=[];return;}
 if(cmd==='plugin:window|is_visible')return true;
 if(cmd==='get_model_page')return previewModels(state.data);
 if(cmd==='get_model_speed'){
  await pause();const result=previewModelSpeed(args.request);
  if(state.empty){result.points=[];result.current=null;}
  return result;
 }
 if(cmd==='get_session_page')return {items:structuredClone(state.data.recent),older:null,newer:null};
 if(cmd==='get_session_detail'){await pause();return previewSessionDetail(state.data,args.request.id);}
 throw Error('Unexpected IPC: '+cmd);
},{shouldMockEvents:true});
({emit}=await import('__API__/event.js'));
state.push=async()=>{state.data.updated_at=new Date().toISOString();await emit('snapshot-changed');};
await import('/src/main.ts');
`.replaceAll('__API__', api);

async function sample(page, kind, duration = 650) {
  await page.evaluate(
    ({ kind, duration }) => {
      const state = window.__disclosureTest;
      state.frames = [];
      state.writes = [];
      const start = performance.now();
      const frame = (at) => {
        const el = document.querySelector(`[id^="${kind}-detail-"]:not([inert])`);
        const animation = el?.getAnimations().find((a) => a.playState === 'running');
        const keys = animation?.effect.getKeyframes();
        state.frames.push({
          at: at - start,
          height: el?.getBoundingClientRect().height ?? 0,
          natural: el?.firstElementChild.getBoundingClientRect().height ?? 0,
          active: !!animation,
          target: keys ? parseFloat(keys.at(-1).height) : null,
        });
        if (at - start < duration) requestAnimationFrame(frame);
      };
      requestAnimationFrame(frame);
      document.querySelector(kind === 'model' ? '.cp-modelrow' : '.cp-sessionrow').click();
    },
    { kind, duration },
  );
  await page.waitForTimeout(duration + 60);
  const data = await page.evaluate(() => ({
    frames: __disclosureTest.frames,
    writes: __disclosureTest.writes,
  }));
  const ends = data.frames.slice(1).flatMap((frame, index) => {
    const previous = data.frames[index];
    return previous.active && !frame.active ? [Math.abs(frame.height - previous.target)] : [];
  });
  assert.ok(ends.length, 'Must observe at least one completed height animation');
  assert.ok(
    ends.every((delta) => delta < 1),
    `Animation end height jumped: ${ends}`,
  );
  const last = data.frames.at(-1);
  assert.equal(last.active, false, 'No animation should remain after settling');
  assert.ok(
    Math.abs(last.height - last.natural) < 1,
    'Full natural content height must be visible',
  );
  assert.deepEqual(data.writes, [], 'Click expansion must not restore page scroll');
  return Math.max(...ends);
}

async function checkRestoration(page, kind, language) {
  await page.setViewportSize({ width: 320, height: 420 });
  await page.waitForTimeout(220);
  const expected = await page.locator('.cp-body').evaluate((el) => {
    const value = el.scrollHeight - el.clientHeight - 5;
    el.scrollTop = value;
    return el.scrollTop;
  });
  assert.ok(expected > 0, 'Restoration fixture needs overflowing content');
  await page.waitForTimeout(80);
  await page
    .getByRole('button', { name: language === 'zh' ? '总览' : 'Overview', exact: true })
    .click();
  await page
    .getByRole('button', {
      name: kind === 'session' ? 'Sessions' : language === 'zh' ? '模型' : 'Models',
      exact: true,
    })
    .click();
  await page.waitForTimeout(700);
  const actual = await page.locator('.cp-body').evaluate((el) => el.scrollTop);
  assert.ok(
    Math.abs(actual - expected) < 1,
    `${kind} restored scroll ${actual}, expected ${expected}`,
  );
  assert.equal(await page.locator(`.cp-${kind}row[aria-expanded="true"]`).count(), 1);
  await page.setViewportSize({ width: 320, height: 800 });
  await page.waitForTimeout(220);
}

(async () => {
  const browser = await chromium.launch({
    headless: true,
    channel: process.env.CODEXPULSE_BROWSER_CHANNEL || 'msedge',
  });
  const reports = [];
  try {
    for (const platform of ['windows', 'macos'])
      for (const language of ['zh', 'en'])
        for (const delay of [0, 120, 240, 600]) {
          const page = await browser.newPage({
            viewport: { width: delay === 120 ? 320 : 380, height: 800 },
            reducedMotion: 'no-preference',
          });
          const errors = [];
          page.on('pageerror', (error) => errors.push(error.message));
          await page.route('**/__disclosure_test__.js', (route) =>
            route.fulfill({ contentType: 'text/javascript', body: boot }),
          );
          await page.route(
            (url) => url.pathname === '/' && url.searchParams.has('delay'),
            async (route) => {
              const response = await route.fetch();
              await route.fulfill({
                response,
                body: (await response.text()).replace(
                  /src="\/src\/main\.ts(?:\?[^\"]*)?"/g,
                  'src="/__disclosure_test__.js"',
                ),
              });
            },
          );
          await page.goto(
            new URL(`?language=${language}&platform=${platform}&delay=${delay}`, base).href,
          );
          await page.locator('.cp-modelrow').first().waitFor();
          assert.equal(
            await page.locator('.cp-body').evaluate((el) => getComputedStyle(el).overflowAnchor),
            'none',
          );
          await page.waitForTimeout(200);
          const modelEndDelta = await sample(page, 'model', Math.max(650, delay + 260));
          if (delay === 120) await checkRestoration(page, 'model', language);
          if (delay === 240) {
            for (const index of [1, 0]) {
              await page.locator('.cp-modelrow').nth(index).click();
              await page.waitForTimeout(35);
            }
            await page.waitForTimeout(480);
            assert.equal(await page.locator('.cp-modelrow[aria-expanded="true"]').count(), 1);
            assert.equal(await page.evaluate(() => __disclosureTest.observers), 1);
          }
          assert.equal(await page.evaluate(() => __disclosureTest.observers), 1);
          await page.locator('.cp-modelrow').first().click();
          assert.equal(
            await page.locator('.cp-disclosure[inert]').count(),
            1,
            'Closing content must be inert',
          );
          await page.waitForTimeout(240);
          assert.equal(await page.evaluate(() => __disclosureTest.observers), 0);
          if (delay === 240) {
            for (const mode of ['fail', 'empty']) {
              await page.evaluate((mode) => {
                __disclosureTest[mode] = true;
              }, mode);
              await sample(page, 'model');
              await page.locator('.cp-modelrow').first().click();
              await page.waitForTimeout(240);
              await page.evaluate((mode) => {
                __disclosureTest[mode] = false;
              }, mode);
            }
          }
          await page.getByRole('button', { name: 'Sessions', exact: true }).click();
          await page.locator('.cp-sessionrow').first().waitFor();
          assert.equal(
            await page.locator('.cp-body').evaluate((el) => getComputedStyle(el).overflowAnchor),
            'none',
          );
          await page.waitForTimeout(100);
          const sessionEndDelta = await sample(page, 'session', Math.max(650, delay + 260));
          if (delay === 120) await checkRestoration(page, 'session', language);
          await page.evaluate(() => __disclosureTest.push());
          await page.waitForTimeout(delay + 220);
          assert.equal(await page.locator('.cp-sessionrow[aria-expanded="true"]').count(), 1);
          assert.equal(await page.evaluate(() => __disclosureTest.observers), 1);
          const row = page.locator('.cp-sessionrow').first();
          if (delay === 240) {
            for (const index of [1, 0]) {
              await page.locator('.cp-sessionrow').nth(index).click();
              await page.waitForTimeout(35);
            }
            await page.waitForTimeout(480);
            assert.equal(await page.evaluate(() => __disclosureTest.observers), 1);
          }
          for (let i = 0; i < 6; i++) {
            await row.click();
            await page.waitForTimeout(35);
          }
          await page.waitForTimeout(450);
          assert.equal(await page.locator('.cp-sessionrow[aria-expanded="true"]').count(), 1);
          assert.equal(
            await page.evaluate(() => __disclosureTest.observers),
            1,
            'Reversal must not leak observers',
          );
          await row.click();
          await page.waitForTimeout(35);
          await page.emulateMedia({ reducedMotion: 'reduce' });
          await page.waitForTimeout(30);
          assert.equal(await page.locator('.cp-disclosure').count(), 0);
          assert.equal(await page.evaluate(() => __disclosureTest.observers), 0);
          await row.click();
          await page.waitForTimeout(delay + 80);
          assert.equal(
            await page.locator('.cp-disclosure').evaluate((el) => el.getAnimations().length),
            0,
          );
          await page
            .getByRole('button', { name: language === 'zh' ? '总览' : 'Overview', exact: true })
            .click();
          await page.waitForTimeout(50);
          assert.equal(
            await page.evaluate(() => __disclosureTest.observers),
            0,
            'Page unmount must release observers',
          );
          assert.equal(
            await page.locator('.cp-body').evaluate((el) => getComputedStyle(el).overflowAnchor),
            'auto',
            'Overview retains browser anchoring',
          );
          await page.getByRole('button', { name: /^Tibo/ }).click();
          assert.equal(
            await page.locator('.cp-body').evaluate((el) => getComputedStyle(el).overflowAnchor),
            'auto',
            'Tibo retains browser anchoring for inserted messages',
          );
          if (delay === 120) {
            await page.emulateMedia({ reducedMotion: 'no-preference' });
            await page
              .getByRole('button', { name: language === 'zh' ? '模型' : 'Models', exact: true })
              .click();
            await page.locator('.cp-modelrow').first().click();
            await page.waitForTimeout(20);
            await page
              .getByRole('button', { name: language === 'zh' ? '总览' : 'Overview', exact: true })
              .click();
            await page.waitForTimeout(320);
            assert.equal(
              await page.evaluate(() => __disclosureTest.observers),
              0,
              'Unmount during loading must release the observer',
            );
          }
          assert.deepEqual(errors, []);
          reports.push({ platform, language, delay, modelEndDelta, sessionEndDelta });
          console.log(
            `${platform}/${language}/${delay}ms: no end jump or click scroll; resize, reversal, reduced motion and cleanup passed`,
          );
          await page.close();
        }
    fs.writeFileSync(
      path.resolve(__dirname, '../work/disclosure-verification.json'),
      JSON.stringify(reports, null, 2),
    );
  } finally {
    await browser.close();
  }
})().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
