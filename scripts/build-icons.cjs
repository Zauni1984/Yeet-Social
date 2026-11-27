// Renders the app icons from the logo (lime disc, black "Y") into
// frontend/icons. Run: node scripts/build-icons.cjs  (needs playwright).
// The outputs are committed; rerun only when the logo changes.
const { chromium } = require('playwright');
const path = require('path');
const OUT = path.join(__dirname, '..', 'frontend', 'icons');
const LIME = '#c6f135', DARK = '#05060a';
const glyph = (size, scale) =>
  `<text x="${size / 2}" y="${size / 2}" text-anchor="middle" dominant-baseline="central" font-family="FreeSans, Helvetica, Arial, sans-serif" font-weight="700" font-size="${size * scale}" fill="#000">Y</text>`;
// "any": the round logo on a transparent square.
const round = s => `<svg xmlns="http://www.w3.org/2000/svg" width="${s}" height="${s}"><circle cx="${s / 2}" cy="${s / 2}" r="${s / 2}" fill="${LIME}"/>${glyph(s, 0.62)}</svg>`;
// "maskable": full-bleed lime, glyph inside the 80 % safe zone (launchers crop the rest).
const maskable = s => `<svg xmlns="http://www.w3.org/2000/svg" width="${s}" height="${s}"><rect width="${s}" height="${s}" fill="${LIME}"/>${glyph(s, 0.46)}</svg>`;
// Splash / notification art: the round logo on the app's dark background.
const onDark = s => `<svg xmlns="http://www.w3.org/2000/svg" width="${s}" height="${s}"><rect width="${s}" height="${s}" fill="${DARK}"/><circle cx="${s / 2}" cy="${s / 2}" r="${s * 0.36}" fill="${LIME}"/>${glyph(s, 0.45)}</svg>`;
// Monochrome (themed icons, notification small icon): white glyph, transparent.
const mono = s => `<svg xmlns="http://www.w3.org/2000/svg" width="${s}" height="${s}">${glyph(s, 0.46).replace('fill="#000"', 'fill="#fff"')}</svg>`;
const jobs = [
  ['icon-192.png', 192, round], ['icon-512.png', 512, round],
  ['maskable-192.png', 192, maskable], ['maskable-512.png', 512, maskable],
  ['monochrome-512.png', 512, mono], ['splash-512.png', 512, onDark],
  ['apple-touch-icon.png', 180, maskable], ['favicon-48.png', 48, round], ['favicon-32.png', 32, round],
  ['store-icon-512.png', 512, maskable],
];
// Android wrapper (android/): adaptive-icon layers, splash image, notification icon.
const RES = path.join(__dirname, '..', 'android', 'app', 'src', 'main', 'res');
// Adaptive icon layers are 108dp with a 66dp safe zone: keep the glyph small.
const fg = s => `<svg xmlns="http://www.w3.org/2000/svg" width="${s}" height="${s}">${glyph(s, 0.36)}</svg>`;
const fgMono = s => fg(s).replace('fill="#000"', 'fill="#fff"');
const androidJobs = [
  ['mipmap-xxxhdpi/ic_launcher_foreground.png', 432, fg],
  ['mipmap-xxxhdpi/ic_launcher_monochrome.png', 432, fgMono],
  ['drawable-nodpi/splash.png', 512, round],
  ['drawable-nodpi/ic_notification_icon.png', 96, s => mono(s).replace(`font-size="${s * 0.46}"`, `font-size="${s * 0.8}"`)],
];
(async () => {
  const b = await chromium.launch();
  const all = jobs.map(j => [path.join(OUT, j[0]), j[1], j[2]]).concat(androidJobs.map(j => [path.join(RES, j[0]), j[1], j[2]]));
  for (const [name, size, svg] of all) {
    const p = await b.newPage({ viewport: { width: size, height: size }, deviceScaleFactor: 1 });
    await p.setContent(`<html><body style="margin:0;background:transparent">${svg(size)}</body></html>`);
    await p.screenshot({ path: name, omitBackground: true, clip: { x: 0, y: 0, width: size, height: size } });
    await p.close();
    console.log(path.relative(path.join(__dirname, '..'), name));
  }
  await b.close();
})();
