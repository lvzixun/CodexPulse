// Capture the real Svelte UI with synthetic, development-only data.
// Requires Playwright and an already running Vite development server.
const { chromium } = require(process.env.CODEXPULSE_PLAYWRIGHT_MODULE || 'playwright');
const path = require('node:path');
const fs = require('node:fs/promises');

async function main() {
  const base = process.env.CODEXPULSE_PREVIEW_URL || 'http://127.0.0.1:1430/';
  if (!['127.0.0.1', 'localhost'].includes(new URL(base).hostname)) {
    throw new Error('Screenshots must use a local development server.');
  }
  const output = path.resolve(__dirname, '../docs/screenshots');
  await fs.mkdir(output, { recursive: true });
  const browser = await chromium.launch({
    headless: true,
    ...(process.env.CODEXPULSE_BROWSER_CHANNEL
      ? { channel: process.env.CODEXPULSE_BROWSER_CHANNEL }
      : {}),
  });
  try {
    for (const language of ['zh', 'en']) {
      for (const pageName of ['overview', 'models', 'sessions', 'news', 'compact']) {
        if (process.env.CODEXPULSE_CAPTURE_PAGE && pageName !== process.env.CODEXPULSE_CAPTURE_PAGE)
          continue;
        const compact = pageName === 'compact';
        const page = await browser.newPage({
          viewport: {
            width: compact ? 184 : 380,
            height: compact ? 36 : pageName === 'models' ? 980 : 800,
          },
          deviceScaleFactor: 3,
          timezoneId: 'Asia/Shanghai',
          locale: language === 'zh' ? 'zh-CN' : 'en-US',
          reducedMotion: 'reduce',
        });
        const errors = [];
        page.on('pageerror', (error) => errors.push(error.message));
        await page.route('**/*', (route) => {
          const hostname = new URL(route.request().url()).hostname;
          return ['127.0.0.1', 'localhost'].includes(hostname) ? route.continue() : route.abort();
        });
        const url = new URL(base);
        url.search = new URLSearchParams({
          preview: 'readme',
          language,
          page: compact ? 'overview' : pageName,
          ...(compact ? { mode: 'compact', platform: 'windows' } : {}),
        }).toString();
        await page.goto(url.href);
        await page.locator(compact ? '.cp-compact-model' : '.cp-tabs').waitFor();
        if (compact) await page.getByText('6.1-sol', { exact: true }).waitFor();
        else if (pageName === 'models') {
          await page.getByText('gpt-6-astra', { exact: true }).waitFor();
          await page.locator('.cp-modelrow').first().click();
          await page.locator('.cp-speed-chart').waitFor();
        } else if (pageName === 'sessions')
          await page.locator('.cp-session-detail[aria-busy="false"] .cp-metrics').first().waitFor();
        else if (pageName === 'news')
          await page.getByText('A smoother review experience', { exact: true }).waitFor();
        else await page.getByText('Build a personal dashboard', { exact: true }).waitFor();
        await page.evaluate(() => document.fonts.ready);
        // Let async page data and the Svelte layout settle before capturing.
        await page.waitForTimeout(600);
        if (compact) {
          const overlaps = await page.evaluate(() => {
            const state = document.querySelector('.cp-compact-drag').getBoundingClientRect();
            const model = document.querySelector('.cp-compact-model').getBoundingClientRect();
            return state.right > model.left;
          });
          if (overlaps) throw new Error(`${language}: compact status overlaps the model`);
        }
        // Native macOS rounds the host window. Reproduce only its outer clipping;
        // do not simulate a desktop backdrop or alter the app's content/styles.
        if (!compact)
          await page.addStyleTag({ content: 'main.cp-shell { clip-path: inset(0 round 18px); }' });
        if (errors.length) throw new Error(`${language}/${pageName}: ${errors.join('; ')}`);
        const name = `${pageName === 'news' ? 'tibo' : pageName}-${language}.png`;
        await page.screenshot({
          path: path.join(output, name),
          omitBackground: true,
          animations: 'disabled',
        });
        console.log(
          `${name}: ${compact ? '552 × 108' : pageName === 'models' ? '1140 × 2940' : '1140 × 2400'} PNG`,
        );
        await page.close();
      }
    }
  } finally {
    await browser.close();
  }
}
main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
