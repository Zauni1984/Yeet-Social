#!/usr/bin/env node
// Renders the public rewards guide (docs/rewards/rewards-<lang>.html) to
// frontend/yeet-rewards-<lang>.pdf with headless Chromium (Playwright).
//
//   node scripts/build-rewards-pdf.cjs
//
// The PDFs are committed; the deploy script copies them into the web root.
const path = require('path');
const { chromium } = require('playwright');

const root = path.resolve(__dirname, '..');
const docs = [
  { lang: 'de', title: 'Das YEET-Belohnungssystem', page: 'Seite' },
  { lang: 'en', title: 'The YEET Rewards System', page: 'Page' },
];

(async () => {
  const browser = await chromium.launch();
  for (const d of docs) {
    const page = await browser.newPage();
    await page.goto('file://' + path.join(root, 'docs', 'rewards', `rewards-${d.lang}.html`), { waitUntil: 'load' });
    const out = path.join(root, 'frontend', `yeet-rewards-${d.lang}.pdf`);
    const small = 'font-family:Liberation Sans,DejaVu Sans,Arial,sans-serif;font-size:7.5pt;color:#555;width:100%;padding:0 18mm;';
    await page.pdf({
      path: out,
      format: 'A4',
      printBackground: true,
      preferCSSPageSize: true,
      displayHeaderFooter: true,
      headerTemplate: '<span></span>',
      footerTemplate: `<div style="${small}display:flex;justify-content:space-between;"><span>${d.title} · justyeet.it</span><span>${d.page} <span class="pageNumber"></span> / <span class="totalPages"></span></span></div>`,
    });
    await page.close();
    console.log('written', path.relative(root, out));
  }
  await browser.close();
})().catch((e) => { console.error(e); process.exit(1); });
