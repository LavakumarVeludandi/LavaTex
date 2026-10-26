/*
 * End-to-end test of the PowerPoint add-in: `lavatex setup powerpoint` makes
 * the certificate and manifest in a temporary home folder, the real helper
 * serves the add-in over HTTPS, and Chromium runs the task pane and the
 * ribbon commands against a mock of Office.js (tests/mock-office.js).
 *
 *   node hosts/powerpoint/tests/e2e.js [path/to/lavatex] [screenshot-dir]
 */
"use strict";
const { chromium } = require("playwright");
const { spawn, spawnSync } = require("node:child_process");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const CORE = path.resolve(process.argv[2] || path.join(__dirname, "../../../target/debug/lavatex"));
const SHOTS = process.argv[3] || null;
const PORT = 7371;
const ORIGIN = "https://localhost:" + (PORT + 1);
const OFFICE_JS = "https://appsforoffice.microsoft.com/lib/1/hosted/office.js";
const MOCK = fs.readFileSync(path.join(__dirname, "mock-office.js"), "utf8");

let failures = 0;
let helper = null;
function check(name, ok, detail) {
  console.log((ok ? "PASS " : "FAIL ") + name + (detail ? ": " + detail : ""));
  if (!ok) failures++;
}

const home = fs.mkdtempSync(path.join(os.tmpdir(), "lavatex-ppt-e2e-"));
const env = Object.assign({}, process.env, {
  HOME: home,
  USERPROFILE: home,
  XDG_CONFIG_HOME: path.join(home, ".config"),
  XDG_DATA_HOME: path.join(home, ".local/share"),
  LAVATEX_CONFIG_DIR: path.join(home, "config"),
});

function waitForHealth(url) {
  const deadline = Date.now() + 15000;
  return (async function poll() {
    for (;;) {
      try {
        const r = await fetch(url);
        if (r.ok) return;
      } catch (e) {
        /* not up yet */
      }
      if (Date.now() > deadline) throw new Error("helper did not start");
      await new Promise((r) => setTimeout(r, 200));
    }
  })();
}

(async () => {
  // 1. Setup: certificate, manifest, no autostart (the test starts the helper).
  const setup = spawnSync(CORE, ["setup", "powerpoint", "--no-autostart", "--port", String(PORT)], { env, encoding: "utf8" });
  check("setup powerpoint succeeds", setup.status === 0, (setup.stderr || "").trim());
  check("setup reports a new certificate", /HTTPS certificate created/.test(setup.stdout));
  const binDir = path.join(home, ".local/share/lavatex/bin");
  const manifestPath = path.join(binDir, "lavatex-powerpoint.xml");
  const manifest = fs.existsSync(manifestPath) ? fs.readFileSync(manifestPath, "utf8") : "";
  check("manifest written with the helper's HTTPS address", manifest.includes(ORIGIN + "/powerpoint/taskpane.html"));
  check("manifest has no unfilled placeholders", manifest && !/__LAVATEX_/.test(manifest));
  check("manifest declares the LavaTeX ribbon tab", /<CustomTab id="LavaTeX.Tab">/.test(manifest) && /New Equation/.test(manifest));
  check("certificate key is private", (fs.statSync(path.join(home, "config/tls/key.pem")).mode & 0o077) === 0);

  // 2. Helper with HTTPS.
  helper = spawn(CORE, ["serve", "--port", String(PORT)], { env, stdio: ["ignore", "ignore", "pipe"] });
  let helperLog = "";
  let helperExited = false;
  helper.stderr.on("data", (d) => (helperLog += d));
  helper.on("exit", () => (helperExited = true));
  await new Promise((r) => setTimeout(r, 300));
  if (helperExited) throw new Error("the helper could not start (is port " + PORT + " in use?): " + helperLog);
  process.env.NODE_TLS_REJECT_UNAUTHORIZED = "0"; // our test CA is not trusted by Node
  await waitForHealth(ORIGIN + "/v1/health");
  for (let i = 0; i < 20 && !/PowerPoint add-in/.test(helperLog); i++) await new Promise((r) => setTimeout(r, 100));
  check("helper announces the add-in", /PowerPoint add-in at https:\/\/localhost:7372/.test(helperLog));

  const browser = await chromium.launch();
  const context = await browser.newContext({ ignoreHTTPSErrors: true, viewport: { width: 360, height: 900 } });
  await context.route(OFFICE_JS, (route) => route.fulfill({ contentType: "text/javascript", body: MOCK }));
  const pageErrors = [];

  async function openPane(action, state) {
    const page = await context.newPage();
    page.on("pageerror", (e) => pageErrors.push(String(e.stack || e)));
    await page.addInitScript((s) => {
      window.__pptInit = s;
    }, state || {});
    await page.goto(ORIGIN + "/powerpoint/taskpane.html?action=" + action);
    await page.waitForFunction(() => document.getElementById("modeTitle") && window.__ppt);
    return page;
  }
  const model = (page) => page.evaluate(() => JSON.parse(JSON.stringify(window.__ppt)));
  const status = (page) => page.textContent("#status");
  async function waitStatus(page, re) {
    try {
      // "Error" stands for the error style (its "Error:" label comes from CSS).
      await page.waitForFunction(
        (src) => {
          const el = document.getElementById("status");
          return new RegExp(src).test(el.textContent + (el.classList.contains("error") ? " Error" : ""));
        },
        re.source,
        { timeout: 30000 }
      );
    } catch (e) {
      const m = await model(page);
      throw new Error("timed out waiting for " + re + "; status: " + (await status(page)) + "; mock errors: " + m.errors.join("; ") + "; page errors: " + pageErrors.join("; "));
    }
    return status(page);
  }
  async function typeSource(page, text) {
    await page.fill("#source", text);
    await page.dispatchEvent("#source", "input");
  }

  // 3. New equation.
  let page = await openPane("new", { shapes: [] });
  check("token written into the page", await page.evaluate(() => document.querySelector('meta[name="lavatex-token"]').content.length >= 16));
  check("pane starts in new mode", (await page.textContent("#modeTitle")) === "New equation");
  await typeSource(page, "x_{1,2} &= \\frac{-b \\pm \\sqrt{b^2-4ac}}{2a} \\\\\n\\Delta &= b^2-4ac");
  check("preview renders", /pt ·/.test(await waitStatus(page, /pt ·|Error/)));
  check("toolchain list filled", (await page.$$eval("#toolchain option", (o) => o.length)) >= 2);
  if (SHOTS) await page.screenshot({ path: path.join(SHOTS, "powerpoint-taskpane.png") });
  await page.click("#insert");
  await waitStatus(page, /inserted|Error/);
  let m = await model(page);
  let eq = m.shapes[0];
  check("one picture inserted", m.shapes.length === 1 && m.inserts === 1, JSON.stringify(m.errors));
  check("inserted as SVG", eq && eq.coercion === "xmlSvg" && /<svg/.test(eq.data));
  check("LaTeX stored in the LAVATEX tag", eq && /lavatex:1:/.test(eq.tags.LAVATEX || ""));
  check("LaTeX stored in the alt text", eq && /^x_\{1,2\}/.test(eq.altTextDescription) && /lavatex:1:/.test(eq.altTextDescription));
  check("shape named", eq && eq.name === "LavaTeX equation" && eq.altTextTitle === "LaTeX equation");
  check("centred on the slide", eq && Math.abs(eq.left + eq.width / 2 - 480) < 1 && Math.abs(eq.top + eq.height / 2 - 270) < 1);
  check("new picture selected", m.selection.length === 1 && m.selection[0] === eq.id);
  check("pane switches to editing the new picture", (await page.textContent("#insert")) === "Update");
  check("no Office.js misuse", m.errors.length === 0, m.errors.join("; "));
  await page.close();

  // 4. Edit keeps scale, position, rotation and stacking order.
  const natural = { w: eq.width, h: eq.height };
  eq.width *= 2;
  eq.height *= 2;
  eq.left = 50;
  eq.top = 60;
  eq.rotation = 30;
  const cover = { id: "7", name: "Rectangle", type: "GeometricShape", left: 0, top: 0, width: 10, height: 10, rotation: 0, altTextDescription: "", altTextTitle: "", tags: {} };
  const back = Object.assign({}, cover, { id: "6", name: "Background" });
  page = await openPane("edit", { shapes: [back, eq, cover], selection: [eq.id], nextId: 200 });
  await page.waitForFunction(() => document.getElementById("modeTitle").textContent === "Edit equation", null, { timeout: 5000 }).catch(async () => {
    console.log("DEBUG", JSON.stringify((await model(page)).errors), pageErrors, await page.textContent("#banner"));
  });
  check("edit loads the source", (await page.inputValue("#source")).startsWith("x_{1,2}"));
  await typeSource(page, "x_{1,2} &= \\frac{-b \\pm \\sqrt{b^2-4ac}}{2a} \\\\\n\\Delta &= b^2-4ac \\\\\nD &= \\sqrt{\\Delta}");
  await waitStatus(page, /pt ·|Error/);
  await page.click("#insert");
  await waitStatus(page, /updated|Error/);
  m = await model(page);
  const edited = m.shapes.find((s) => s.name === "LavaTeX equation");
  check("old picture replaced, not duplicated", m.shapes.length === 3 && !m.shapes.some((s) => s.id === eq.id), m.shapes.map((s) => s.id).join(","));
  check("position kept", edited && edited.left === 50 && edited.top === 60);
  check("rotation kept", edited && edited.rotation === 30);
  check("scale kept (2x)", edited && Math.abs(edited.width / edited.height - natural.w / natural.h) > 0.01 && edited.width > natural.w * 1.9);
  check("stacking order kept", m.shapes.map((s) => s.id).join(",") === ["6", edited && edited.id, "7"].join(","), m.shapes.map((s) => s.name).join(","));
  check("tag updated", edited && /D &= \\sqrt/.test(edited.altTextDescription));
  check("no Office.js misuse while editing", m.errors.length === 0, m.errors.join("; "));
  const afterEdit = edited;

  // 5. Selecting another equation while the pane is open loads it.
  const other = Object.assign({}, afterEdit, { id: "300", left: 500, altTextDescription: afterEdit.altTextDescription.replace(/^[^\n]*/, "y"), tags: {} });
  other.altTextDescription = afterEdit.altTextDescription; // same record, different picture
  await page.evaluate((s) => {
    window.__ppt.shapes.push(s);
  }, other);
  await page.evaluate(() => window.__select(["300"]));
  await page.waitForFunction(() => document.getElementById("modeTitle").textContent === "Edit equation");
  await page.waitForTimeout(300);
  check("selection change follows the slide", await page.evaluate(() => window.__ppt.selection[0] === "300"));
  // Unsaved typing is not thrown away.
  await typeSource(page, "\\alpha");
  await page.evaluate(() => window.__select([window.__ppt.shapes[1].id]));
  await page.waitForFunction(() => !document.getElementById("banner").hidden);
  check("unsaved typing kept, offer to switch", (await page.inputValue("#source")) === "\\alpha" && /Another equation/.test(await page.textContent("#banner")));
  await page.close();

  // 6. Equation made in LibreOffice/ONLYOFFICE: alt text only, no tag.
  const foreign = Object.assign({}, afterEdit, { id: "400", tags: {} });
  page = await openPane("edit", { shapes: [foreign], selection: ["400"] });
  await page.waitForFunction(() => document.getElementById("modeTitle").textContent === "Edit equation");
  check("equation from another app is editable", (await page.inputValue("#source")).startsWith("x_{1,2}"));
  await page.close();

  // 7. Older PowerPoint (API 1.5): no alt text or rotation API, tags only.
  page = await openPane("new", { api: "1.5", shapes: [] });
  await typeSource(page, "E = mc^2");
  await waitStatus(page, /pt ·|Error/);
  await page.click("#insert");
  await waitStatus(page, /inserted|Error/);
  m = await model(page);
  check("older PowerPoint: inserted with the tag", m.shapes.length === 1 && /lavatex:1:/.test(m.shapes[0].tags.LAVATEX || ""), m.errors.join("; "));
  check("older PowerPoint: says alt text is not set", /cannot set alt text/.test(await status(page)));
  check("older PowerPoint: no Office.js misuse", m.errors.length === 0, m.errors.join("; "));
  await page.close();

  // 8. Errors point at the line.
  page = await openPane("new", { shapes: [] });
  await typeSource(page, "a\n\\fracc{1}{2}");
  const err = await waitStatus(page, /Error|pt ·/);
  check("compile error shown with line", /line 2/.test(err) && (await page.getAttribute("#status", "class")).includes("error"), err);
  await page.click("#insert");
  await page.waitForTimeout(500);
  check("nothing inserted on error", (await model(page)).shapes.length === 0);
  await page.close();

  // 9. Edit button with nothing selected explains what to do.
  page = await openPane("edit", { shapes: [] });
  await page.waitForFunction(() => !document.getElementById("banner").hidden);
  check("edit without selection explains", /Select an equation/.test(await page.textContent("#banner")));
  await page.close();

  // 10. Settings button opens the settings.
  page = await openPane("settings", { shapes: [] });
  check("settings button opens settings", await page.evaluate(() => document.getElementById("settingsBox").open));
  await page.close();

  // 11. Ribbon commands: Regenerate and Help.
  const a = Object.assign({}, afterEdit, { id: "500", left: 10, top: 20, rotation: 0 });
  const b = Object.assign({}, afterEdit, { id: "501", left: 300, top: 200, rotation: 45, tags: { LAVATEX: afterEdit.tags.LAVATEX } });
  const plain = { id: "502", name: "Picture", type: "Image", left: 0, top: 0, width: 5, height: 5, rotation: 0, altTextDescription: "a photo", altTextTitle: "", tags: {} };
  page = await context.newPage();
  page.on("pageerror", (e) => pageErrors.push(String(e.stack || e)));
  await page.addInitScript((s) => {
    window.__pptInit = s;
  }, { shapes: [a, b, plain], selection: ["500", "501", "502"], nextId: 600 });
  await page.goto(ORIGIN + "/powerpoint/commands.html");
  await page.waitForFunction(() => window.__actions && window.__actions.regenerateSelection);
  await page.evaluate(
    () =>
      new Promise((resolve) => {
        window.__actions.regenerateSelection({ completed: resolve });
      })
  );
  m = await model(page);
  const regenerated = m.shapes.filter((s) => s.name === "LavaTeX equation");
  check("regenerate replaces each selected equation", regenerated.length === 2 && !m.shapes.some((s) => s.id === "500" || s.id === "501"), m.shapes.map((s) => s.id).join(","));
  check("regenerate keeps positions and rotation", regenerated.some((s) => s.left === 10 && s.top === 20) && regenerated.some((s) => s.left === 300 && s.rotation === 45));
  check("regenerate leaves other pictures alone", m.shapes.some((s) => s.id === "502"));
  check("regenerate: no Office.js misuse", m.errors.length === 0, m.errors.join("; "));
  await page.evaluate(
    () =>
      new Promise((resolve) => {
        window.__ppt.selection = [];
        window.__actions.regenerateSelection({ completed: resolve });
        setTimeout(() => window.__ppt.dialogs.forEach((d) => d.close()), 300);
      })
  );
  m = await model(page);
  check("regenerate with nothing selected explains", m.dialogs.length === 1 && /Select one or more LavaTeX equations/.test(decodeURIComponent(m.dialogs[0].url)));
  await page.evaluate(() => new Promise((resolve) => window.__actions.openHelp({ completed: resolve })));
  m = await model(page);
  check("help opens the user guide", m.browserWindows.length === 1 && /docs\/usage\.md/.test(m.browserWindows[0]));
  await page.close();

  check("no script errors", pageErrors.length === 0, pageErrors.join("; "));

  await browser.close();
})()
  .catch((e) => {
    console.error(e);
    failures++;
  })
  .finally(() => {
    if (helper) helper.kill();
    fs.rmSync(home, { recursive: true, force: true });
    console.log("\n" + failures + " failure(s)");
    process.exit(failures ? 1 : 0);
  });
