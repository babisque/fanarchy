// Real replies from fanarchyd on the dev machine (B550M + it8689), copied
// verbatim from the daemon socket, run through what Panel.qml uses.
import { test } from "node:test";
import assert from "node:assert/strict";
import { loadModel } from "./load.mjs";

const Client = loadModel("FanarchyClient.js");
const Panel = loadModel("panel.js");
const Curve = loadModel("curve.js");
const plain = (value) => JSON.parse(JSON.stringify(value));

// printf '{"cmd":"get_config"}\n' | socat - UNIX-CONNECT:/run/fanarchy.sock
const GET_CONFIG_REPLY = String.raw`{"ok":true,"data":{"chips":{"board":"it8689","cpu":"k10temp"},"fans":[{"chip":"board","curve":[[40.0,30.0],[60.0,50.0],[75.0,80.0],[85.0,100.0]],"id":"radiator","label":"Radiator fans","min_percent":25,"pwm":1,"role":"fan","sensor":"cpu/temp1"},{"chip":"board","id":"pump","label":"AIO pump","pwm":2,"role":"pump"},{"chip":"board","curve":[[40.0,25.0],[65.0,45.0],[80.0,100.0]],"id":"case","label":"Case fans","min_percent":20,"pwm":3,"role":"fan","sensor":"cpu/temp1"}],"safety":{"cpu_sensor":"cpu/temp1","critical_temp_c":85.0,"sensor_fail_percent":100},"version":1}}`;

// printf '{"cmd":"status"}\n' | socat - UNIX-CONNECT:/run/fanarchy.sock
const STATUS_REPLY = String.raw`{"ok":true,"data":{"chips":[{"fans":[],"name":"acpitz","pwms":[],"temps":[{"celsius":16.799999237060547,"index":1,"label":null},{"celsius":16.799999237060547,"index":2,"label":null}]},{"fans":[],"name":"gigabyte_wmi","pwms":[],"temps":[{"celsius":36.0,"index":1,"label":null},{"celsius":48.0,"index":2,"label":null},{"celsius":46.0,"index":3,"label":null},{"celsius":26.0,"index":4,"label":null},{"celsius":48.0,"index":5,"label":null},{"celsius":57.0,"index":6,"label":null}]},{"fans":[{"index":1,"label":null,"rpm":928},{"index":2,"label":null,"rpm":1670},{"index":3,"label":null,"rpm":576}],"name":"it8689_61030507","pwms":[{"index":1,"mode":1,"value":96},{"index":2,"mode":2,"value":63},{"index":3,"mode":1,"value":79},{"index":4,"mode":2,"value":0},{"index":5,"mode":2,"value":0}],"temps":[{"celsius":36.0,"index":1,"label":null},{"celsius":48.0,"index":2,"label":null},{"celsius":46.0,"index":3,"label":null},{"celsius":26.0,"index":4,"label":null},{"celsius":48.0,"index":5,"label":null},{"celsius":57.0,"index":6,"label":null}]},{"fans":[],"name":"k10temp","pwms":[],"temps":[{"celsius":46.75,"index":1,"label":"Tctl"},{"celsius":46.25,"index":3,"label":"Tccd1"}]}],"cpu_temp_c":46.75,"critical_temp_c":85.0,"fans":[{"id":"radiator","label":"Radiator fans","min_percent":25,"mode":"curve","override":"none","percent":38,"role":"fan","rpm":928,"sensor":"cpu/temp1","temp_c":46.75},{"id":"pump","label":"AIO pump","min_percent":null,"mode":null,"override":null,"percent":null,"role":"pump","rpm":1670,"sensor":null,"temp_c":null},{"id":"case","label":"Case fans","min_percent":20,"mode":"curve","override":"none","percent":31,"role":"fan","rpm":576,"sensor":"cpu/temp1","temp_c":46.75}],"gpu_note":null,"gpus":[{"celsius":58.0,"index":0,"name":"NVIDIA GeForce GTX 1660 SUPER"}]}}`;

// What a QML `var` signal argument does to a value: arrays come back as Qt
// sequence objects (indexable, with a length) that Array.isArray() rejects.
function asQtSequences(value) {
  if (Array.isArray(value)) {
    const sequence = { length: value.length };
    value.forEach((item, i) => { sequence[i] = asQtSequences(item); });
    return sequence;
  }
  if (value && typeof value === "object") {
    return Object.fromEntries(Object.entries(value).map(([k, v]) => [k, asQtSequences(v)]));
  }
  return value;
}

function settled(request, line) {
  const result = Client.settle(Client.enqueue([], request), line);
  assert.equal(result.ok, true);
  assert.equal(result.request.cmd, request.cmd);
  return result;
}

// The panel's path: settle on the request socket, cross the replied()
// signal as JSON text, decode in Panel.qml.
function configThroughPanel() {
  const result = settled(Client.getConfigRequest(), GET_CONFIG_REPLY);
  const encoded = Client.encodeReply(result.request, result.data);
  return Client.decodeReply(encoded.requestJson, encoded.dataJson);
}

const status = settled({ cmd: "status" }, STATUS_REPLY).data;

test("real get_config through the panel: request and every curve survive", () => {
  const reply = configThroughPanel();
  assert.equal(reply.request.cmd, "get_config");
  assert.equal(Array.isArray(reply.data.fans), true);
  assert.equal(Panel.curvesSummary(reply.data),
    "radiator=[[40,30],[60,50],[75,80],[85,100]] case=[[40,25],[65,45],[80,100]]");
});

test("real payload: the editor gets each fan's curve and it is valid", () => {
  const config = configThroughPanel().data;
  assert.deepEqual(plain(Panel.curveOf(config, "radiator")), [[40, 30], [60, 50], [75, 80], [85, 100]]);
  assert.deepEqual(plain(Panel.curveOf(config, "case")), [[40, 25], [65, 45], [80, 100]]);
  for (const id of ["radiator", "case"]) {
    const curve = Curve.sorted(Panel.curveOf(config, id));
    assert.equal(Curve.validate(curve, Panel.minPercent(Panel.statusFan(status, id))), null);
    assert.equal(Curve.canAdd(curve), true);
  }
});

test("real payload: cards show fanN · pwmN · sensor", () => {
  const cards = plain(Panel.fanCards(status, configThroughPanel().data));
  const byId = Object.fromEntries(cards.map((card) => [card.id, card]));
  assert.equal(byId.radiator.header, "fan1 · pwm1 · cpu/temp1");
  assert.equal(byId.case.header, "fan3 · pwm3 · cpu/temp1");
  assert.equal(byId.pump.kind, "pump");
  assert.equal(byId.pump.detail, "fan2 · pwm2");
});

test("the reported bug: the config as a var-signal Qt sequence still parses", () => {
  // Before the fix this config gave "(no controlled fans)", curveOf() = []
  // and fan— · pwm— on every card.
  const config = asQtSequences(JSON.parse(GET_CONFIG_REPLY).data);
  assert.equal(Array.isArray(config.fans), false);
  assert.equal(Panel.curvesSummary(config),
    "radiator=[[40,30],[60,50],[75,80],[85,100]] case=[[40,25],[65,45],[80,100]]");
  assert.deepEqual(plain(Panel.curveOf(config, "case")), [[40, 25], [65, 45], [80, 100]]);
  const card = plain(Panel.fanCards(status, config)).find((c) => c.id === "case");
  assert.equal(card.header, "fan3 · pwm3 · cpu/temp1");
  const points = Panel.curveOf(config, "case");
  assert.equal(Curve.validate(Curve.copy(points), 20), null);
  assert.equal(Curve.canAdd(asQtSequences([[40, 25], [80, 100]])), true);
});
