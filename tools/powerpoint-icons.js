#!/usr/bin/env node
/*
 * Draw the PowerPoint ribbon icons and write them as PNGs into
 * hosts/powerpoint/ribbon/images/ (64 px; Office scales them for large and
 * small buttons). Uses Playwright's Chromium to rasterize.
 *
 *   node tools/powerpoint-icons.js
 *
 * The PNGs are committed; run this only to change the icons.
 */
"use strict";

const path = require("path");
const { chromium } = require("playwright");

const OUT = path.join(__dirname, "..", "hosts", "powerpoint", "ribbon", "images");

// Orange tile with a sigma, as in the LibreOffice and ONLYOFFICE icons.
const TILE = `
  <defs>
    <linearGradient id="g" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#f76c2a"/><stop offset="1" stop-color="#e95524"/>
    </linearGradient>
  </defs>
  <rect x="1" y="1" width="30" height="30" rx="6" fill="url(#g)"/>`;
const SIGMA = `<path d="M9 7.5h14v4h-2.2l-.6-1.6h-6.4l4.6 6.1-4.8 6.1h7l.7-1.8H23v4.2H9v-2.3l5.6-6.3L9 9.8z" fill="#fff"/>`;
const SIGMA_SMALL = `<g transform="translate(-2 -2) scale(0.92)">${SIGMA}</g>`;

function badge(inner) {
  return `<circle cx="24.5" cy="24.5" r="7" fill="#fff"/><circle cx="24.5" cy="24.5" r="5.8" fill="#2b7a3d"/>${inner}`;
}

const ICONS = {
  // The add-in itself.
  lavatex: [TILE, SIGMA],
  // New equation: sigma with a plus.
  new: [TILE, SIGMA_SMALL, badge(`<path d="M24.5 21.3v6.4M21.3 24.5h6.4" stroke="#fff" stroke-width="2" stroke-linecap="round"/>`)],
  // Edit equation: sigma with a pencil.
  edit: [
    TILE,
    SIGMA_SMALL,
    `<circle cx="24.5" cy="24.5" r="7" fill="#fff"/><circle cx="24.5" cy="24.5" r="5.8" fill="#1f5fa8"/>`,
    `<path d="M21.6 27.4l.5-2.2 4.3-4.3 1.7 1.7-4.3 4.3z" fill="#fff"/>`,
  ],
  // Regenerate: two circular arrows.
  regenerate: [
    TILE,
    `<g fill="none" stroke="#fff" stroke-width="2.6" stroke-linecap="round">
       <path d="M22.6 12.2A8 8 0 0 0 8.4 14.5"/><path d="M9.4 19.8a8 8 0 0 0 14.2-2.3"/>
     </g>
     <path d="M24.8 7.6v6.2h-6.2z" fill="#fff"/><path d="M7.2 24.4v-6.2h6.2z" fill="#fff"/>`,
  ],
  // Help: a question mark.
  help: [
    TILE,
    `<path d="M11.6 12.4a4.6 4.6 0 1 1 6.9 4c-1.5.9-2.4 1.6-2.4 3.3v.6" fill="none" stroke="#fff" stroke-width="2.8" stroke-linecap="round"/>
     <circle cx="16.1" cy="24.6" r="1.9" fill="#fff"/>`,
  ],
};

const SIZE = 64;

(async () => {
  const browser = await chromium.launch();
  const page = await browser.newPage();
  require("fs").mkdirSync(OUT, { recursive: true });
  for (const [name, parts] of Object.entries(ICONS)) {
    const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${SIZE}" height="${SIZE}" viewBox="0 0 32 32">${parts.join("")}</svg>`;
    await page.setContent(`<html><body style="margin:0;background:transparent">${svg}</body></html>`);
    const file = path.join(OUT, `${name}.png`);
    await page.locator("svg").screenshot({ path: file, omitBackground: true });
    console.log(path.relative(process.cwd(), file));
  }
  await browser.close();
})();
