import { test } from "node:test";
import assert from "node:assert/strict";
import { loadModel } from "./load.mjs";

const Panel = loadModel("panel.js");
const Client = loadModel("FanarchyClient.js");
const plain = (value) => JSON.parse(JSON.stringify(value));

const status = {
  fans: [
    { id: "radiator", label: "Radiator fans", role: "fan", sensor: "cpu/temp1", temp_c: 48, rpm: 1180, percent: 34, min_percent: 25, mode: "curve", override: "none" },
    { id: "pump", label: "AIO pump", role: "pump", sensor: null, temp_c: null, rpm: 2950, percent: null, min_percent: null, mode: null, override: null },
    { id: "case", label: "Case fans", role: "fan", sensor: "cpu/temp1", temp_c: 48, rpm: 900, percent: null, min_percent: 20, mode: "auto", override: "none" },
  ],
  cpu_temp_c: 47.2,
  critical_temp_c: 85,
  gpus: [{ index: 0, name: "GTX", celsius: 41 }],
  gpu_note: null,
  chips: [],
};

const config = {
  version: 1,
  fans: [
    { id: "radiator", label: "Radiator fans", chip: "board", pwm: 1, role: "fan", sensor: "cpu/temp1", curve: [[40, 30], [85, 100]], min_percent: 25 },
    { id: "pump", label: "AIO pump", chip: "board", pwm: 2, role: "pump" },
    { id: "case", label: "Case fans", chip: "board", pwm: 3, role: "fan", sensor: "cpu/temp1", curve: [[40, 25], [80, 100]], min_percent: 20 },
  ],
};

test("panel state: down, pending, empty, ready", () => {
  assert.equal(Panel.panelState(false, status), "down");
  assert.equal(Panel.panelState(true, null), "pending");
  assert.equal(Panel.panelState(true, { ...status, fans: [status.fans[1]] }), "empty");
  assert.equal(Panel.panelState(true, { ...status, fans: [] }), "empty");
  assert.equal(Panel.panelState(true, status), "ready");
});

test("fan cards carry header, sensor, RPM, % and mode", () => {
  const cards = plain(Panel.fanCards(status, config));
  assert.deepEqual(cards[0], {
    id: "radiator",
    kind: "fan",
    name: "RADIATOR FANS",
    header: "fan1 · pwm1 · cpu/temp1",
    rpm: "1180 RPM",
    detail: "34% · curve",
  });
  assert.equal(cards[2].detail, "AUTO · bios");
});

test("the pump card is read-only and says so", () => {
  const pump = plain(Panel.fanCards(status, config))[1];
  assert.equal(pump.kind, "pump");
  assert.equal(pump.header, "Pump · automatic control, always");
  assert.equal(pump.rpm, "2950 RPM");
  assert.equal(pump.detail, "fan2 · pwm2");
});

test("cards work before the config arrives", () => {
  const cards = plain(Panel.fanCards(status, null));
  assert.equal(cards[0].header, "fan— · pwm— · cpu/temp1");
});

test("overrides show in the card", () => {
  const hot = { ...status.fans[0], percent: 100, override: "critical" };
  assert.equal(plain(Panel.fanCard(hot, config)).detail, "100% · CRITICAL");
  const failed = { ...status.fans[0], percent: 100, override: "sensor_failed" };
  assert.equal(plain(Panel.fanCard(failed, config)).detail, "100% · SENSOR FAILED");
});

test("placeholder cards never show NaN or blanks", () => {
  for (const card of plain(Panel.placeholderCards())) {
    for (const value of Object.values(card)) assert.doesNotMatch(String(value), /NaN|undefined/);
    assert.equal(card.rpm, "— RPM");
  }
});

test("selection keeps the current fan and skips pumps", () => {
  assert.equal(Panel.pickSelection(status, "case"), "case");
  assert.equal(Panel.pickSelection(status, "pump"), "radiator");
  assert.equal(Panel.pickSelection(status, "gone"), "radiator");
  assert.equal(Panel.pickSelection(null, "case"), "");
});

test("header temperatures", () => {
  assert.deepEqual(plain(Panel.headerTemps(status)), { cpu: "47°", gpu: "41°" });
  assert.deepEqual(plain(Panel.headerTemps({ ...status, gpus: [], cpu_temp_c: null })), { cpu: "—°", gpu: "—°" });
  assert.deepEqual(plain(Panel.headerTemps(null)), { cpu: "—°", gpu: "—°" });
});

test("curve and config lookups", () => {
  assert.deepEqual(plain(Panel.curveOf(config, "case")), [[40, 25], [80, 100]]);
  assert.deepEqual(plain(Panel.curveOf(config, "pump")), []);
  assert.deepEqual(plain(Panel.curveOf(null, "case")), []);
  assert.equal(Panel.statusFan(status, "case").rpm, 900);
  assert.equal(Panel.editorTitle(status.fans[0]), "CURVE — RADIATOR FANS");
});

test("fixed slider starts at the current speed within the minimum", () => {
  assert.equal(Panel.fixedStart(status.fans[0]), 34);
  assert.equal(Panel.fixedStart({ percent: 10, min_percent: 25 }), 25);
  assert.equal(Panel.fixedStart({ percent: null, min_percent: 20 }), 50);
  assert.equal(Panel.minPercent(status.fans[2]), 20);
});

test("modes map BIOS to the protocol's auto", () => {
  assert.deepEqual(plain(Panel.MODES).map((m) => m.mode), ["curve", "fixed", "auto"]);
  assert.deepEqual(plain(Panel.MODES).map((m) => m.label), ["CURVE", "FIXED", "BIOS"]);
});

test("confirmations for every accepted change", () => {
  assert.equal(Panel.confirmation(Client.setCurveRequest("case", [[40, 30], [80, 100]])), "✓ curve applied");
  assert.equal(Panel.confirmation(Client.setModeRequest("case", "fixed", 60)), "✓ fixed at 60%");
  assert.equal(Panel.confirmation(Client.setModeRequest("case", "auto")), "✓ BIOS in control");
  assert.equal(Panel.confirmation(Client.setModeRequest("case", "curve")), "✓ following the curve");
  assert.equal(Panel.confirmation({ cmd: "copy" }), "✓ copied");
  assert.equal(Panel.confirmation(null), "");
});

test("a widget click right after an outside click does not reopen", () => {
  assert.equal(Panel.ignoreReopen(1000, 1200), true);
  assert.equal(Panel.ignoreReopen(1000, 1500), false);
  assert.equal(Panel.ignoreReopen(null, 1200), false);
});

test("the reopen guard swallows at most one open", () => {
  const first = plain(Panel.reopenGuard(1000, 1200));
  assert.deepEqual(first, { ignore: true, nextLastDismissMs: -1 });
  const second = plain(Panel.reopenGuard(first.nextLastDismissMs, 1250));
  assert.deepEqual(second, { ignore: false, nextLastDismissMs: -1 });
  assert.equal(Panel.reopenGuard(1000, 5000).ignore, false);
  assert.equal(Panel.reopenGuard(-1, 5000).ignore, false);
});

test("windowLost: open state follows the window", () => {
  assert.equal(Panel.windowLost(true, false), true);
  assert.equal(Panel.windowLost(true, undefined), true);
  assert.equal(Panel.windowLost(true, true), false);
  assert.equal(Panel.windowLost(false, false), false);
  assert.equal(Panel.windowLost(false, true), false);
});

test("empty state copy", () => {
  assert.equal(Panel.emptyState("down", "stopped").command, "sudo systemctl start fanarchyd");
  assert.equal(Panel.emptyState("down", "stopped").headline, "THE DAEMON STOPPED LISTENING.");
  assert.equal(Panel.emptyState("down", "permission").command, "sudo usermod -aG fanarchy $USER");
  const empty = plain(Panel.emptyState("empty"));
  assert.equal(empty.command, "fanarchyctl init");
  assert.equal(empty.button, "START SETUP");
});

test("cards split by kind", () => {
  const cards = Panel.fanCards(status, config);
  assert.deepEqual(plain(Panel.cardsOfKind(cards, "fan")).map((c) => c.id), ["radiator", "case"]);
  assert.deepEqual(plain(Panel.cardsOfKind(cards, "pump")).map((c) => c.id), ["pump"]);
});

test("a requested mode shows until the daemon reports it", () => {
  const fan = status.fans[0];
  const pending = { fan: "radiator", mode: "fixed" };
  assert.equal(Panel.displayMode(fan, pending), "fixed");
  assert.equal(Panel.displayMode(fan, null), "curve");
  assert.equal(Panel.displayMode(fan, { fan: "case", mode: "auto" }), "curve");
  assert.equal(Panel.pendingSettled(fan, pending), false);
  assert.equal(Panel.pendingSettled({ ...fan, mode: "fixed" }, pending), true);
  assert.equal(Panel.pendingSettled(fan, null), true);
});

test("withCurve replaces one fan's curve without touching the input", () => {
  const next = Panel.withCurve(config, "case", [[30, 20], [70, 100]]);
  assert.deepEqual(plain(Panel.curveOf(next, "case")), [[30, 20], [70, 100]]);
  assert.deepEqual(plain(Panel.curveOf(config, "case")), [[40, 25], [80, 100]]);
  assert.equal(Panel.withCurve(null, "case", []), null);
});

test("footer names the socket", () => {
  assert.equal(Panel.footerText(true), "daemon online · /run/fanarchy.sock");
  assert.equal(Panel.footerText(false), "daemon offline · /run/fanarchy.sock");
});
