import { test } from "node:test";
import assert from "node:assert/strict";
import { loadModel } from "./load.mjs";

const Curve = loadModel("curve.js");
const plain = (value) => JSON.parse(JSON.stringify(value));

const base = [[40, 30], [60, 50], [75, 80], [85, 100]];

test("evaluate matches the daemon: linear inside, flat outside", () => {
  assert.equal(Curve.evaluate(base, 30), 30);
  assert.equal(Curve.evaluate(base, 40), 30);
  assert.equal(Curve.evaluate(base, 50), 40);
  assert.equal(Curve.evaluate(base, 90), 100);
  assert.equal(Curve.evaluate([], 50), 100);
  assert.equal(Curve.evaluate(base, NaN), 100);
});

test("evaluate sorts unsorted input", () => {
  assert.equal(Curve.evaluate([[60, 50], [40, 30]], 50), 40);
});

test("movePoint stays between its neighbours", () => {
  // Past the next point: stops 1 °C before it, and at its percent.
  assert.deepEqual(plain(Curve.movePoint(base, 1, 90, 95, 25))[1], [74, 80]);
  // Before the previous point: stops 1 °C after it, and at its percent.
  assert.deepEqual(plain(Curve.movePoint(base, 1, 10, 0, 25))[1], [41, 30]);
  // Free move rounds to whole numbers.
  assert.deepEqual(plain(Curve.movePoint(base, 1, 55.4, 44.6, 25))[1], [55, 45]);
});

test("movePoint never goes below the fan's minimum", () => {
  assert.deepEqual(plain(Curve.movePoint(base, 0, 35, 5, 25))[0], [35, 25]);
});

test("movePoint keeps the ends inside the chart", () => {
  assert.deepEqual(plain(Curve.movePoint(base, 0, -10, 30, 0))[0], [20, 30]);
  assert.deepEqual(plain(Curve.movePoint(base, 3, 140, 100, 0))[3], [100, 100]);
});

test("movePoint does not change its input", () => {
  const copy = plain(base);
  Curve.movePoint(base, 1, 70, 70, 0);
  assert.deepEqual(plain(base), copy);
});

test("addPointAt puts the point on the curve", () => {
  const next = plain(Curve.addPointAt(base, 50, 0));
  assert.equal(next.length, 5);
  assert.deepEqual(next[1], [50, 40]);
});

test("addPointAt respects the minimum and refuses duplicates", () => {
  assert.deepEqual(plain(Curve.addPointAt([[40, 10], [80, 100]], 25, 30))[0], [25, 30]);
  assert.equal(Curve.addPointAt(base, 60, 0).length, 4);
});

test("addPoint fills the widest gap", () => {
  const next = plain(Curve.addPoint(base, 0));
  assert.deepEqual(next[1], [50, 40]);
});

test("a curve holds 2 to 8 points", () => {
  let points = base;
  for (let i = 0; i < 10; i++) points = Curve.addPoint(points, 0);
  assert.equal(points.length, Curve.MAX_POINTS);
  assert.equal(Curve.canAdd(points), false);

  let two = [[40, 30], [80, 100]];
  assert.equal(Curve.removePoint(two, 0).length, 2);
  assert.equal(Curve.canRemove(two), false);
  assert.deepEqual(plain(Curve.removePoint(base, 1)), [[40, 30], [75, 80], [85, 100]]);
});

test("validate mirrors the daemon's rules plus the minimum", () => {
  assert.equal(Curve.validate(base, 25), null);
  assert.match(Curve.validate([[40, 30]], 0), /at least 2/);
  assert.match(Curve.validate([[40, 30], [40, 50]], 0), /rising/);
  assert.match(Curve.validate([[40, 60], [60, 50]], 0), /never go down/);
  assert.match(Curve.validate([[40, 10], [60, 50]], 25), /minimum \(25%\)/);
  assert.match(Curve.validate([[40, 30], [130, 50]], 0), /0 to 120/);
  assert.match(Curve.validate([[40, 30], [60, 150]], 0), /0 to 100/);
});

test("chart mapping round-trips", () => {
  assert.equal(Curve.xFor(20, 400), 0);
  assert.equal(Curve.xFor(100, 400), 400);
  assert.equal(Curve.xFor(60, 400), 200);
  assert.equal(Curve.yFor(0, 200), 200);
  assert.equal(Curve.yFor(100, 200), 0);
  assert.equal(Curve.tempAt(200, 400), 60);
  assert.equal(Curve.percentAt(50, 200), 75);
  assert.equal(Curve.tempAt(-5, 400), 20);
  assert.equal(Curve.percentAt(500, 200), 0);
});

test("polyline runs flat to both edges", () => {
  const line = plain(Curve.polyline([[40, 50], [80, 100]], 400, 200));
  assert.deepEqual(line, [
    { x: 0, y: 100 },
    { x: 100, y: 100 },
    { x: 300, y: 0 },
    { x: 400, y: 0 },
  ]);
});

test("labels and ticks", () => {
  assert.equal(Curve.pointLabel([65, 55]), "65° → 55%");
  assert.deepEqual(plain(Curve.tempTicks()), [20, 30, 40, 50, 60, 70, 80, 90, 100]);
  assert.deepEqual(plain(Curve.percentTicks()), [0, 25, 50, 75, 100]);
});

test("+ POINT on an empty curve makes a valid 2-point starter", () => {
  const starter = Curve.addPoint([], 20);
  assert.deepEqual(JSON.parse(JSON.stringify(starter)), [[40, 30], [80, 100]]);
  assert.equal(Curve.validate(starter, 20), null);
  // Never below the fan's minimum.
  assert.deepEqual(JSON.parse(JSON.stringify(Curve.addPoint([], 45))), [[40, 45], [80, 100]]);
  assert.equal(Curve.validate(Curve.addPoint([], 45), 45), null);
  assert.equal(Curve.canAdd([]), true);
});

test("+ POINT on a single point gives it a partner", () => {
  const low = Curve.addPoint([[50, 40]], 20);
  assert.deepEqual(JSON.parse(JSON.stringify(low)), [[50, 40], [80, 100]]);
  assert.equal(Curve.validate(low, 20), null);
  const high = Curve.addPoint([[90, 100]], 20);
  assert.deepEqual(JSON.parse(JSON.stringify(high)), [[40, 30], [90, 100]]);
  assert.equal(Curve.validate(high, 20), null);
});

test("double-click on an empty chart also yields a valid curve", () => {
  const curve = Curve.addPointAt([], 55, 20);
  assert.equal(curve.length >= 2, true);
  assert.equal(Curve.validate(curve, 20), null);
});

test("listOf accepts Qt sequence objects", () => {
  const sequence = { length: 2, 0: [40, 30], 1: [80, 100] };
  assert.deepEqual(JSON.parse(JSON.stringify(Curve.listOf(sequence))), [[40, 30], [80, 100]]);
  assert.equal(Curve.listOf(null), null);
  assert.equal(Curve.listOf("abc"), null);
  assert.equal(Curve.validate(sequence, 20), null);
});
