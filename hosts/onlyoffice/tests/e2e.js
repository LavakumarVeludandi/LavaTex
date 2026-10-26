/*
 * End-to-end test of the ONLYOFFICE plugin page against the real
 * `lavatex serve` helper, in Chromium, with a mock presentation editor that
 * implements the Builder API calls the plugin uses (Api.GetSelection,
 * Api.CreateImage, ApiDrawing.Get/SetDescription, ...).
 *
 *   node hosts/onlyoffice/tests/e2e.js [path/to/lavatex] [screenshot-dir]
 */
"use strict";
const { chromium } = require("playwright");
const { spawn } = require("node:child_process");
const http = require("node:http");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const PLUGIN_DIR = path.join(__dirname, "..");
const CORE = path.resolve(process.argv[2] || path.join(__dirname, "../../../target/debug/lavatex"));
const SHOTS = process.argv[3] || null;
const HELPER_PORT = 7361;
const PAGE_PORT = 7362;
const TOKEN = "e2e-test-token-0123456789";

const SDK_STUB = `
  window.Asc = { scope: {}, plugin: {
    callCommand: function (fn, isClose, isCalc, cb) {
      var result;
      try { result = fn(); } catch (e) { window.__errors.push(String(e && e.stack || e)); }
      setTimeout(function () { if (cb) cb(result); }, 0);
    },
    executeCommand: function (cmd) { if (cmd === "close") window.__closed = true; },
    executeMethod: function () {},
  } };
  window.__errors = [];
  // Like the real SDK: the handshake starts from window.onload.
  window.onload = function () { window.Asc.plugin.init(""); };
`;

// Mock editor state lives in the page so plugin commands can reach it.
const MOCK_EDITOR = `
  (function () {
    var nextId = 1;
    function Drawing(url, w, h) {
      this.id = "d" + nextId++; this.url = url; this.w = w; this.h = h; this.x = 0; this.y = 0;
      this.rot = 0; this.desc = null; this.title = null; this.name = null;
    }
    Drawing.prototype.GetClassType = function () { return "image"; };
    Drawing.prototype.GetInternalId = function () { return this.id; };
    Drawing.prototype.GetWidth = function () { return this.w; };
    Drawing.prototype.GetHeight = function () { return this.h; };
    Drawing.prototype.SetSize = function (w, h) { this.w = w; this.h = h; return true; };
    Drawing.prototype.GetPosX = function () { return this.x; };
    Drawing.prototype.GetPosY = function () { return this.y; };
    Drawing.prototype.SetPosition = function (x, y) { this.x = x; this.y = y; };
    Drawing.prototype.GetRotation = function () { return this.rot; };
    Drawing.prototype.SetRotation = function (r) { this.rot = r; return true; };
    Drawing.prototype.SetDescription = function (d) { if (!d) return false; this.desc = d; return true; };
    Drawing.prototype.GetDescription = function () { return this.desc; };
    Drawing.prototype.SetTitle = function (t) { this.title = t; return true; };
    Drawing.prototype.SetName = function (n) { this.name = n; return true; };
    Drawing.prototype.Select = function () { window.__editor.selection = [this]; };
    Drawing.prototype.Delete = function () {
      var s = window.__editor.slide; s.objects = s.objects.filter(function (o) { return o !== this; }, this);
      return true;
    };
    var slide = {
      objects: [],
      GetWidth: function () { return 9144000; },
      GetHeight: function () { return 5143500; },
      AddObject: function (d) { this.objects.push(d); return true; },
    };
    window.__editor = { slide: slide, selection: [], Drawing: Drawing };
    window.Api = {
      GetPresentation: function () { return { GetCurrentSlide: function () { return slide; } }; },
      GetSelection: function () {
        return {
          IsEmpty: function () { return window.__editor.selection.length === 0; },
          GetShapes: function () { return window.__editor.selection.slice(); },
        };
      },
      CreateImage: function (url, w, h) { return new Drawing(url, w, h); },
      GetByInternalId: function (id) {
        return slide.objects.filter(function (o) { return o.id === id; })[0] || null;
      },
    };
  })();
`;

function serveStatic(dir, port) {
  const types = { ".html": "text/html", ".js": "text/javascript", ".css": "text/css", ".png": "image/png", ".json": "application/json" };
  const server = http.createServer((req, res) => {
    const file = path.join(dir, decodeURIComponent(req.url.split("?")[0]));
    if (!file.startsWith(dir) || !fs.existsSync(file) || fs.statSync(file).isDirectory()) {
      res.writeHead(404).end();
      return;
    }
    res.writeHead(200, { "Content-Type": types[path.extname(file)] || "application/octet-stream" });
    fs.createReadStream(file).pipe(res);
  });
  return new Promise((resolve) => server.listen(port, "127.0.0.1", () => resolve(server)));
}

function check(cond, msg) {
  console.log((cond ? "PASS " : "FAIL ") + msg);
  if (!cond) process.exitCode = 1;
}

async function main() {
  const configDir = fs.mkdtempSync(path.join(os.tmpdir(), "lavatex-e2e-"));
  const helper = spawn(CORE, ["serve", "--port", String(HELPER_PORT), "--token", TOKEN], {
    env: { ...process.env, LAVATEX_CONFIG_DIR: configDir },
    stdio: "ignore",
  });
  const server = await serveStatic(PLUGIN_DIR, PAGE_PORT);
  const browser = await chromium.launch();
  try {
    const context = await browser.newContext({ viewport: { width: 760, height: 640 } });
    await context.route("https://onlyoffice.github.io/sdkjs-plugins/v1/plugins.js", (r) =>
      r.fulfill({ contentType: "text/javascript", body: SDK_STUB })
    );
    await context.route("https://onlyoffice.github.io/sdkjs-plugins/v1/plugins-ui.js", (r) =>
      r.fulfill({ contentType: "text/javascript", body: "" })
    );
    const page = await context.newPage();
    page.on("pageerror", (e) => console.log("page error:", e.message));

    // First run: no token yet, so the setup screen must appear.
    await page.addInitScript(MOCK_EDITOR);
    await page.goto(`http://127.0.0.1:${PAGE_PORT}/index.html`);
    await page.waitForSelector("#setup:not([hidden])", { timeout: 15000 });
    check(true, "setup screen shown when no token is configured");
    check(!(await page.isVisible("#editor")), "equation editor hidden until connected");
    check(await page.evaluate(() => window.LAVATEX_SDK_BASE === "https://onlyoffice.github.io/sdkjs-plugins/v1/"),
      "SDK loader falls back to the online SDK when no local copy exists");
    if (SHOTS) await page.screenshot({ path: path.join(SHOTS, "onlyoffice-setup.png") });
    await page.fill("#helperUrl", `http://127.0.0.1:${HELPER_PORT}`);
    await page.fill("#helperToken", TOKEN);
    await page.click("#connect");
    await page.waitForSelector("#editor:not([hidden])");
    const tcCount = await page.$$eval("#toolchain option", (o) => o.length);
    check(tcCount >= 2, "connected; TeX installations listed (" + (tcCount - 1) + ")");

    // Insert a new equation.
    await page.fill("#source", "\\int_0^\\infty e^{-x^2}\\,dx = \\frac{\\sqrt{\\pi}}{2}");
    await page.waitForSelector("#preview img", { timeout: 20000 });
    await page.waitForFunction(() => document.getElementById("status").classList.contains("ok"));
    await page.evaluate(() => window.Asc.plugin.button(0));
    await page.waitForFunction(() => window.__closed === true, null, { timeout: 20000 });
    let objects = await page.evaluate(() => window.__editor.slide.objects.map((o) => ({ ...o })));
    check(objects.length === 1, "one picture inserted");
    const first = objects[0];
    check(first.url.startsWith("data:image/svg+xml;base64,"), "inserted as SVG");
    check(first.desc && first.desc.includes("lavatex:1:") && first.desc.startsWith("\\int_0"), "alt text carries source and record");
    check(first.x > 0 && first.y > 0, "centred on the slide");

    // Simulate the user enlarging and moving it, then selecting it and reopening the plugin.
    await page.evaluate(() => {
      const d = window.__editor.slide.objects[0];
      d.w *= 2; d.h *= 2; d.x = 1000000; d.y = 2000000; d.rot = 15;
      window.__editor.selection = [d];
      window.__state = JSON.stringify({ objects: window.__editor.slide.objects, sel: d.id });
    });
    const saved = await page.evaluate(() => window.__state);
    await page.addInitScript((s) => {
      // Rebuild the editor state after reload.
      const data = JSON.parse(s);
      window.addEventListener("DOMContentLoaded", () => {
        data.objects.forEach((o) => {
          const d = new window.__editor.Drawing(o.url, o.w, o.h);
          Object.assign(d, o);
          window.__editor.slide.objects.push(d);
          if (d.id === data.sel) window.__editor.selection = [d];
        });
      });
    }, saved);
    await page.reload();
    await page.waitForSelector("#banner:not([hidden])", { timeout: 15000 });
    const banner = await page.textContent("#banner");
    check(/Editing the selected equation/.test(banner), "reopening on a LavaTeX picture enters edit mode");
    check((await page.inputValue("#source")).startsWith("\\int_0^\\infty"), "source restored from the picture");
    await page.fill("#source", "\\int_0^\\infty e^{-x^2}\\,dx = \\frac{\\sqrt{\\pi}}{2} \\approx 0.886");
    await page.waitForFunction(() => document.getElementById("status").classList.contains("ok") && /0\.886/.test(document.querySelector("#preview img").alt));
    if (SHOTS) await page.screenshot({ path: path.join(SHOTS, "onlyoffice-edit.png") });
    await page.evaluate(() => window.Asc.plugin.button(0));
    await page.waitForFunction(() => window.__closed === true, null, { timeout: 20000 });
    objects = await page.evaluate(() => window.__editor.slide.objects.map((o) => ({ ...o })));
    check(objects.length === 1, "edit replaces the picture instead of adding one");
    const edited = objects[0];
    check(edited.x === 1000000 && edited.y === 2000000 && edited.rot === 15, "position and rotation kept");
    const rec = await page.evaluate((d) => window.LavaTeX.fromAltText(d), edited.desc);
    const expectedW = Math.round(rec.widthBp * 12700 * 2);
    check(Math.abs(edited.w - expectedW) <= 2, "user's 2x scale kept for the longer equation");
    check(rec.source.endsWith("0.886"), "new source stored");

    // Errors are shown inline with the line number.
    await page.fill("#source", "x +\n\\fracc{1}{2}");
    await page.waitForSelector("#status.error", { timeout: 20000 });
    const err = await page.textContent("#status");
    check(/fracc/.test(err) && /line 2/.test(err), "compile error shown with line: " + err.trim());
    if (SHOTS) await page.screenshot({ path: path.join(SHOTS, "onlyoffice-error.png") });

    const errors = await page.evaluate(() => window.__errors);
    check(errors.length === 0, "no exceptions inside editor commands " + (errors.join("; ") || ""));
  } finally {
    await browser.close();
    server.close();
    helper.kill();
  }
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
