const { chromium } = require(process.env.CODEXPULSE_PLAYWRIGHT_MODULE || 'playwright');
const assert = require('node:assert/strict');
const base = process.env.CODEXPULSE_PREVIEW_URL || 'http://127.0.0.1:1430/';
if (!['127.0.0.1', 'localhost'].includes(new URL(base).hostname))
  throw Error('Use a local dev server');
(async () => {
  const browser = await chromium.launch({
    headless: true,
    channel: process.env.CODEXPULSE_BROWSER_CHANNEL || 'msedge',
  });
  try {
    let cases = 0;
    for (const platform of ['windows', 'macos'])
      for (const language of ['zh', 'en']) {
        const page = await browser.newPage({ viewport: { width: 380, height: 800 } });
        await page.goto(
          new URL(`?preview=readme&language=${language}&platform=${platform}`, base).href,
        );
        await page.getByText('Build a personal dashboard', { exact: true }).waitFor();
        for (const theme of ['light', 'dark', 'system'])
          for (const accent of ['blue', 'violet', 'teal', 'amber', 'rose']) {
            await page.emulateMedia({ colorScheme: theme === 'dark' ? 'dark' : 'light' });
            await page.locator('main').evaluate(
              (el, { theme, accent }) => {
                el.dataset.theme = theme;
                el.dataset.accent = accent;
              },
              { theme, accent },
            );
            const selected = await page
              .locator('.cp-tabs button[aria-pressed="true"]')
              .evaluate((el) => {
                const style = getComputedStyle(el);
                return {
                  background: style.backgroundColor,
                  color: style.color,
                  weight: Number(style.fontWeight),
                  border: style.boxShadow,
                };
              });
            const other = await page
              .locator('.cp-tabs button[aria-pressed="false"]')
              .first()
              .evaluate((el) => {
                const style = getComputedStyle(el);
                return { background: style.backgroundColor, color: style.color };
              });
            assert.notEqual(
              selected.background,
              other.background,
              'Selected tab needs a distinct fill',
            );
            assert.notEqual(
              selected.color,
              other.color,
              'Selected tab needs a distinct text color',
            );
            assert.ok(selected.weight >= 600 && selected.border !== 'none');
            const rows = await page.locator('.cp-reset-status > div').evaluateAll((els) =>
              els.map((el) => {
                const label = el.querySelector('span').getBoundingClientRect(),
                  value = el.querySelector('strong').getBoundingClientRect();
                return (
                  label.top < value.bottom && value.top < label.bottom && label.right <= value.left
                );
              }),
            );
            assert.deepEqual(rows, [true, true], 'Reset labels and values must share each row');
            const resetCard = await page.locator('.cp-reset-status').evaluate((el) => {
              const style = getComputedStyle(el);
              const session = getComputedStyle(document.querySelector('.cp-session-overview'));
              return {
                background: style.backgroundColor,
                sessionBackground: session.backgroundColor,
                radius: style.borderRadius,
                sessionRadius: session.borderRadius,
                padding: style.padding,
                sessionPadding: session.padding,
                border: style.borderTopWidth,
                nextIsSession: el.nextElementSibling.classList.contains('cp-session-overview'),
              };
            });
            assert.notEqual(resetCard.background, 'rgba(0, 0, 0, 0)', 'Reset card needs a surface');
            assert.equal(resetCard.background, resetCard.sessionBackground);
            assert.equal(resetCard.radius, resetCard.sessionRadius);
            assert.equal(resetCard.nextIsSession, true, 'No extra divider between the cards');
            if (platform === 'windows') {
              assert.equal(resetCard.padding, resetCard.sessionPadding);
              assert.equal(resetCard.border, '0px', 'Replace the old rules with a single surface');
            }
            if (theme !== 'system' && accent === 'blue')
              await page.screenshot({
                path: `work/reset-card-${theme}-${platform}-${language}.png`,
              });
            cases++;
          }
        await page.close();
      }
    console.log(
      `${cases} theme/accent/platform/language combinations: selected tabs, inline reset rows and reset/session card surfaces passed`,
    );
  } finally {
    await browser.close();
  }
})().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
