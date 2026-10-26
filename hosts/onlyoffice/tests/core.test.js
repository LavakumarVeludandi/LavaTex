// node --test hosts/onlyoffice/tests/
"use strict";
const test = require("node:test");
const assert = require("node:assert");
const fs = require("node:fs");
const path = require("node:path");
const LT = require("../scripts/lavatex-core.js");

const fixture = JSON.parse(fs.readFileSync(path.join(__dirname, "..", "..", "..", "fixtures", "metadata", "alt-text.json"), "utf8"));

test("parses alt text written by the Rust core", () => {
  const record = LT.fromAltText(fixture.altText);
  assert.ok(record, "record parsed");
  assert.strictEqual(record.source, fixture.meta.source);
  assert.deepStrictEqual(record, fixture.meta);
});

test("tolerates text appended by hosts", () => {
  const record = LT.fromAltText(fixture.altText + "\n\n  ");
  assert.strictEqual(record.source, fixture.meta.source);
});

test("ignores ordinary alt text and other schemas", () => {
  assert.strictEqual(LT.fromAltText("A photo of a cat"), null);
  assert.strictEqual(LT.fromAltText(null), null);
  const other = Buffer.from(JSON.stringify({ schema: "lavatex-equation/2", source: "x" })).toString("base64url");
  assert.strictEqual(LT.fromAltText("x\n\nlavatex:1:" + other), null);
  assert.strictEqual(LT.fromAltText("x\n\nlavatex:1:!!!"), null);
});

test("keeps the user's scale when editing", () => {
  const record = { widthBp: 100, heightBp: 20 };
  const previous = { record, widthEmu: 100 * LT.EMU_PER_BP * 1.5 };
  const size = LT.sizeEmu({ widthBp: 200, heightBp: 30 }, previous);
  assert.strictEqual(size.width, Math.round(200 * LT.EMU_PER_BP * 1.5));
  assert.strictEqual(size.height, Math.round(30 * LT.EMU_PER_BP * 1.5));
  const fresh = LT.sizeEmu({ widthBp: 10, heightBp: 5 }, null);
  assert.strictEqual(fresh.width, 10 * LT.EMU_PER_BP);
});
