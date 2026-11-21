import { test } from "node:test";
import assert from "node:assert/strict";
import { loadModel } from "./load.mjs";

const Placement = loadModel("placement.js");
const plain = (value) => JSON.parse(JSON.stringify(value));

const target = { width: 60, height: 26 };
const popup = { width: 360, height: 200 };

test("top bar puts the tooltip below, centered", () => {
  assert.deepEqual(plain(Placement.tooltipOffset("top", target, popup, 10)), { x: -150, y: 36 });
});

test("bottom bar puts the tooltip above", () => {
  assert.deepEqual(plain(Placement.tooltipOffset("bottom", target, popup, 10)), { x: -150, y: -210 });
});

test("side bars put the tooltip beside the button", () => {
  assert.deepEqual(plain(Placement.tooltipOffset("left", target, popup, 10)), { x: 70, y: -87 });
  assert.deepEqual(plain(Placement.tooltipOffset("right", target, popup, 10)), { x: -370, y: -87 });
});

test("clamps along the bar axis only", () => {
  const window = { width: 1920, height: 1080 };
  assert.deepEqual(
    plain(Placement.clampToWindow("top", { x: 1800.4, y: 40 }, popup, window, 10)),
    { x: 1550, y: 40 },
  );
  assert.deepEqual(
    plain(Placement.clampToWindow("top", { x: -50, y: 40 }, popup, window, 10)),
    { x: 10, y: 40 },
  );
  assert.deepEqual(
    plain(Placement.clampToWindow("left", { x: 70, y: 1000 }, popup, window, 10)),
    { x: 70, y: 870 },
  );
});
