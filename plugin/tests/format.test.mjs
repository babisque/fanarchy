import { test } from "node:test";
import assert from "node:assert/strict";
import { loadModel } from "./load.mjs";

const Format = loadModel("format.js");

test("temperature rounds and keeps the degree sign", () => {
  assert.equal(Format.temperature(47.4), "47°");
  assert.equal(Format.temperature(85.6), "86°");
  assert.equal(Format.temperature(0), "0°");
});

test("missing temperatures never show NaN or blanks", () => {
  for (const value of [null, undefined, NaN, Infinity, "47"]) {
    assert.equal(Format.temperature(value), "—°");
    assert.equal(Format.celsius(value), "— °C");
  }
});

test("celsius and rpm keep their units", () => {
  assert.equal(Format.celsius(41.0), "41 °C");
  assert.equal(Format.rpm(1180), "1180 RPM");
  assert.equal(Format.rpm(0), "0 RPM");
  assert.equal(Format.rpm(null), "— RPM");
});

test("percent shows AUTO for pumps and motherboard-driven fans", () => {
  assert.equal(Format.percent({ role: "fan", percent: 38 }), "38%");
  assert.equal(Format.percent({ role: "fan", percent: null, mode: "auto" }), "AUTO");
  assert.equal(Format.percent({ role: "pump", percent: null }), "AUTO");
  assert.equal(Format.percent(null), "AUTO");
});

test("fanValue joins RPM and percent", () => {
  assert.equal(Format.fanValue({ role: "fan", rpm: 1180, percent: 38 }), "1180 RPM · 38%");
  assert.equal(Format.fanValue({ role: "pump", rpm: 2950, percent: null }), "2950 RPM · AUTO");
  assert.equal(Format.fanValue({ role: "fan", rpm: null, percent: 100 }), "— RPM · 100%");
});

test("accessible names describe each state", () => {
  assert.equal(Format.accessibleName("normal", 47.2), "Fanarchy: CPU 47 degrees");
  assert.equal(Format.accessibleName("hot", 86), "Fanarchy: CPU 86 degrees, running hot");
  assert.equal(Format.accessibleName("down", null), "Fanarchy: daemon stopped");
  assert.equal(
    Format.accessibleName("normal", null),
    "Fanarchy: CPU temperature not available yet",
  );
});
