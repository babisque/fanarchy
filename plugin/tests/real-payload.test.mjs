// Real replies captured from fanarchyd on the dev machine (B550M + it8689),
// run through the same functions Panel.qml uses.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { loadModel } from "./load.mjs";

const Client = loadModel("FanarchyClient.js");
const Panel = loadModel("panel.js");
const Curve = loadModel("curve.js");
const plain = (value) => JSON.parse(JSON.stringify(value));

const fixtures = join(dirname(fileURLToPath(import.meta.url)), "fixtures");
const line = (name) => readFileSync(join(fixtures, name), "utf8").split("\n")[0];

function reply(request, fileName) {
  const result = Client.settle(Client.enqueue([], request), line(fileName));
  assert.equal(result.ok, true, "daemon reply is ok");
  assert.equal(result.request.cmd, request.cmd);
  assert.equal(result.queue.length, 0);
  return result.data;
}

const config = reply(Client.getConfigRequest(), "get_config.reply.jsonl");
const status = reply({ cmd: "status" }, "status.reply.jsonl");

test("real get_config: every controlled fan's curve reaches the editor", () => {
  assert.deepEqual(plain(Panel.curveOf(config, "case")), [[40, 25], [65, 45], [80, 100]]);
  assert.deepEqual(plain(Panel.curveOf(config, "radiator")), [[40, 30], [60, 50], [75, 80], [85, 100]]);
  for (const id of ["case", "radiator"]) {
    const fan = Panel.statusFan(status, id);
    assert.equal(Curve.validate(Curve.sorted(Panel.curveOf(config, id)), Panel.minPercent(fan)), null);
  }
});

test("real payload: cards show fanN · pwmN · sensor once the config is in", () => {
  const cards = plain(Panel.fanCards(status, config));
  const byId = Object.fromEntries(cards.map((card) => [card.id, card]));
  assert.equal(byId.radiator.header, "fan1 · pwm1 · cpu/temp1");
  assert.equal(byId.case.header, "fan3 · pwm3 · cpu/temp1");
  assert.equal(byId.pump.detail, "fan2 · pwm2");
  assert.equal(byId.pump.kind, "pump");
});

test("without the config the cards fall back to dashes (the reported symptom)", () => {
  const cards = plain(Panel.fanCards(status, null));
  assert.equal(cards.find((card) => card.id === "case").header, "fan— · pwm— · cpu/temp1");
  assert.deepEqual(plain(Panel.curveOf(null, "case")), []);
});
