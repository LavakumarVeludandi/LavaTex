/*
 * End-to-end test of the editor window page (hosts/editor) in Chromium,
 * talking to the real program: `lavatex editor --stdio` answers the page's
 * messages exactly as the Windows window does, just over stdin/stdout.
 *
 *   node hosts/editor/tests/e2e.js [path/to/lavatex] [screenshot-dir]
 */
"use strict";
const { chromium } = require("playwright");
const { spawn, spawnSync } = require("node:child_process");
const readline = require("node:readline");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const CORE = path.resolve(process.argv[2] || path.join(__dirname, "../../../target/debug/lavatex"));
const SHOTS = process.argv[3] || null;
const EDITOR_DIR = path.join(__dirname, "..");
const CORE_JS = path.join(__dirname, "../../onlyoffice/scripts/lavatex-core.js");
const ORIGIN = "http://lavatex.localhost";

let failures = 0;
function check(name, ok, detail) {
  console.log((ok ? "PASS " : "FAIL ") + name + (detail ? ": " + detail : ""));
  if (!ok) failures++;
}

const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "lavatex-editor-e2e-"));
const env = Object.assign({}, process.env, { LAVATEX_CONFIG_DIR: path.join(tmp, "config") });

/** Open the editor page wired to a `lavatex editor --stdio` process. */
async function openEditor(browser, args) {
  const out = path.join(tmp, "out-" + Math.random().toString(36).slice(2));
  const child = spawn(CORE, ["editor", "--stdio", "--out", out].concat(args || []), { env, stdio: ["pipe", "pipe", "inherit"] });
  const lines = readline.createInterface({ input: child.stdout });
  const queue = [];
  let waiting = null;
  lines.on("line", (l) => {
    queue.push(JSON.parse(l));
    if (waiting) waiting();
  });
  const next = () =>
    new Promise((resolve) => {
      const take = () => (queue.length ? resolve(queue.shift()) : (waiting = take));
      take();
    });
  const first = await next();
  const session = { out, child, exitCode: null, page: null };

  const page = await browser.newPage({ viewport: { width: 760, height: 680 } });
  session.page = page;
  page.on("pageerror", (e) => check("no script error", false, String(e)));
  await page.route(ORIGIN + "/**", (route) => {
    const rel = new URL(route.request().url()).pathname.slice(1) || "index.html";
    const file = rel === "lavatex-core.js" ? CORE_JS : path.join(EDITOR_DIR, rel);
    if (!fs.existsSync(file)) return route.fulfill({ status: 404, body: "not found" });
    const type = { html: "text/html", js: "text/javascript", css: "text/css" }[rel.split(".").pop()] || "application/octet-stream";
    route.fulfill({ contentType: type, body: fs.readFileSync(file) });
  });
  // Messages are answered in order, like the window's handler.
  let chain = Promise.resolve();
  await page.exposeFunction("__toProgram", (text) => {
    const id = JSON.parse(text).id;
    chain = chain.then(async () => {
      child.stdin.write(text + "\n");
      const answer = await next();
      if ("exit" in answer) {
        session.exitCode = answer.exit;
      } else {
        await page.evaluate(([i, data]) => window.__lavatexReply(i, data), [id, answer.reply]);
      }
    });
  });
  await page.addInitScript((init) => {
    window.__lavatexInit = init;
    window.ipc = { postMessage: (text) => window.__toProgram(text) };
  }, first.init);
  await page.goto(ORIGIN + "/index.html");
  return session;
}

async function waitStatus(page, re) {
  await page.waitForFunction(
    (src) => {
      const el = document.getElementById("status");
      return new RegExp(src).test(el.textContent + (el.classList.contains("error") ? " Error" : ""));
    },
    re.source,
    { timeout: 30000 }
  );
  return page.textContent("#status");
}

async function waitExit(session) {
  for (let i = 0; i < 300 && session.exitCode === null; i++) await new Promise((r) => setTimeout(r, 100));
  return session.exitCode;
}

(async () => {
  const browser = await chromium.launch();

  // 1. New equation.
  let s = await openEditor(browser);
  let page = s.page;
  check("new: button says Insert", (await page.textContent("#insert")) === "Insert");
  check("TeX list filled", await page.waitForFunction(() => document.querySelectorAll("#toolchain option").length >= 2).then(() => true, () => false));
  await page.fill("#source", "x_{1,2} &= \\frac{-b \\pm \\sqrt{b^2-4ac}}{2a} \\\\\n\\Delta &= b^2-4ac");
  await page.dispatchEvent("#source", "input");
  await page.fill("#fontSize", "28");
  await page.dispatchEvent("#fontSize", "change");
  check("preview renders", /pt ·/.test(await waitStatus(page, /pt ·|Error/)));
  check("preview image shown", (await page.$("#preview img")) !== null);
  if (SHOTS) await page.screenshot({ path: path.join(SHOTS, "powerpoint-editor.png") });
  await page.keyboard.press("Control+Enter");
  check("Ctrl+Enter inserts and closes (exit 0)", (await waitExit(s)) === 0);
  const result = fs.existsSync(path.join(s.out, "result.txt")) ? fs.readFileSync(path.join(s.out, "result.txt"), "utf8") : "";
  check("result.txt written", /^file=equation\.svg$/m.test(result) && /^widthBp=\d/m.test(result), result);
  const alt = fs.existsSync(path.join(s.out, "alt.txt")) ? fs.readFileSync(path.join(s.out, "alt.txt"), "utf8") : "";
  check("alt text written", alt.startsWith("x_{1,2}") && alt.includes("lavatex:1:"));
  check("picture written", fs.existsSync(path.join(s.out, "equation.svg")));
  const saved = JSON.parse(fs.readFileSync(path.join(tmp, "config/powerpoint.json"), "utf8"));
  check("settings remembered", saved.fontSize === 28);
  s.child.kill();
  await page.close();

  // 2. Edit an existing equation (from its alt text).
  const altFile = path.join(tmp, "alt.txt");
  fs.writeFileSync(altFile, alt);
  s = await openEditor(browser, ["--alt-text-file", altFile]);
  page = s.page;
  check("edit: button says Update", (await page.textContent("#insert")) === "Update");
  check("edit: source loaded", (await page.inputValue("#source")).startsWith("x_{1,2}"));
  check("edit: size loaded", (await page.inputValue("#fontSize")) === "28");
  await waitStatus(page, /pt ·|Error/);

  // 3. Errors keep the window open and point at the line.
  await page.fill("#source", "a\n\\fracc{1}{2}");
  await page.dispatchEvent("#source", "input");
  const err = await waitStatus(page, /line 2/);
  check("error with line number", /line 2/.test(err), err);
  await page.click("#insert");
  await page.waitForTimeout(1500);
  check("failed insert keeps the window open", s.exitCode === null);
  await page.keyboard.press("Escape");
  check("Esc cancels (exit 1)", (await waitExit(s)) === 1);
  s.child.kill();
  await page.close();

  // 4. Not an equation.
  fs.writeFileSync(path.join(tmp, "bad.txt"), "just a photo");
  const bad = spawnSync(CORE, ["editor", "--stdio", "--out", path.join(tmp, "bad"), "--alt-text-file", path.join(tmp, "bad.txt")], { env, encoding: "utf8" });
  check("non-equation is refused with a message", bad.status === 2 && /not a LavaTeX equation/.test(fs.readFileSync(path.join(tmp, "bad/error.txt"), "utf8")));

  await browser.close();
})()
  .catch((e) => {
    console.error(e);
    failures++;
  })
  .finally(() => {
    fs.rmSync(tmp, { recursive: true, force: true });
    console.log("\n" + failures + " failure(s)");
    process.exit(failures ? 1 : 0);
  });
