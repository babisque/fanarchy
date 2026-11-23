import { test } from "node:test";
import assert from "node:assert/strict";
import { loadModel } from "./load.mjs";

const Setup = loadModel("setup.js");
const plain = (value) => JSON.parse(JSON.stringify(value));

const STARTER = [[40, 30], [60, 50], [75, 80], [85, 100]];

function output(pwm, assigned = { kind: "none", id: null }, extra = {}) {
  return {
    chip: "board",
    pwm,
    path: `/sys/class/hwmon/hwmon1/pwm${pwm}`,
    mode: "automatic",
    percent: 40,
    rpm: pwm <= 3 ? 800 + pwm : null,
    rpm_input: pwm <= 3 ? pwm : null,
    assigned,
    ...extra,
  };
}

// What list_outputs returns right after `fanarchyctl init` on the fixture.
const freshOutputs = [1, 2, 3, 4, 5].map((pwm) => output(pwm));

const config = {
  version: 1,
  chips: { board: "it8689", cpu: "k10temp" },
  fans: [
    { id: "radiator", label: "Radiator fans", chip: "board", pwm: 1, role: "fan", sensor: "cpu/temp1", curve: [[40, 30], [85, 100]], min_percent: 25, rpm_input: 3 },
    { id: "pump", label: "AIO pump", chip: "board", pwm: 2, role: "pump" },
    { id: "case", label: "Case fans", chip: "board", pwm: 3, role: "fan", sensor: "board/temp2", curve: [[40, 25], [80, 100]], min_percent: 5 },
  ],
  safety: { critical_temp_c: 85, sensor_fail_percent: 100, cpu_sensor: "cpu/temp1" },
};

const configuredOutputs = [
  output(1, { kind: "fan", id: "radiator" }),
  output(2, { kind: "pump", id: "pump" }),
  output(3, { kind: "fan", id: "case" }),
  output(4),
  output(5),
];

const status = {
  chips: [
    { name: "k10temp", temps: [{ index: 1, label: "Tctl", celsius: 47.4 }, { index: 3, label: "Tccd1", celsius: 44 }], fans: [], pwms: [] },
    { name: "it8689_61030507", temps: [{ index: 2, label: null, celsius: null }], fans: [{ index: 1, label: null, rpm: 1477 }, { index: 3, label: null, rpm: 815 }], pwms: [] },
    { name: "acpitz", temps: [{ index: 1, label: null, celsius: 30 }], fans: [], pwms: [] },
  ],
};

const empty = { ...config, fans: [] };

test("slug turns a label into an id", () => {
  assert.equal(Setup.slug("Case fans"), "case-fans");
  assert.equal(Setup.slug("  Rear  #2 (top) "), "rear-2-top");
  assert.equal(Setup.slug("Ventilação"), "ventila-o");
  assert.equal(Setup.slug("---"), "");
  assert.equal(Setup.slug(null), "");
});

test("uniqueId numbers taken ids and never returns an empty one", () => {
  assert.equal(Setup.uniqueId("case", []), "case");
  assert.equal(Setup.uniqueId("case", ["case"]), "case-2");
  assert.equal(Setup.uniqueId("case", ["case", "case-2"]), "case-3");
  assert.equal(Setup.uniqueId("", []), "fan");
});

test("step header and titles", () => {
  assert.equal(Setup.stepHeader(1), "STEP 1 / 3");
  assert.deepEqual([1, 2, 3].map(Setup.stepTitle), ["MARK THE PUMP", "PICK YOUR FANS", "CHECK AND APPLY"]);
});

test("a fresh draft starts at the pump with nothing chosen", () => {
  const draft = plain(Setup.initialDraft(freshOutputs, empty));
  assert.equal(draft.step, 1);
  assert.equal(draft.noPump, false);
  assert.deepEqual(draft.rows.map((r) => r.key), ["board/1", "board/2", "board/3", "board/4", "board/5"]);
  assert.ok(draft.rows.every((r) => !r.pump && !r.control && !r.locked));
  assert.equal(Setup.canLeaveStep1(draft), false);
  assert.equal(Setup.canAdvance(draft), false);
  assert.equal(Setup.next(draft).step, 1, "NEXT stays disabled");
});

test("an existing config locks its pumps and prefills its fans", () => {
  const draft = plain(Setup.initialDraft(configuredOutputs, config));
  const [radiator, pump, caseRow, free] = draft.rows;
  assert.equal(draft.step, 1, "the pump step comes first every time");
  assert.deepEqual([pump.pump, pump.locked, pump.id, pump.label], [true, true, "pump", "AIO pump"]);
  assert.deepEqual(
    [radiator.control, radiator.existing, radiator.label, radiator.id, radiator.sensor, radiator.minPercent, radiator.rpmInput],
    [true, true, "Radiator fans", "radiator", "cpu/temp1", 25, 3]
  );
  assert.deepEqual(radiator.curve, [[40, 30], [85, 100]]);
  // A minimum below the slider range is kept, not clamped.
  assert.deepEqual([caseRow.minPercent, caseRow.minLow, caseRow.minHigh], [5, 5, 60]);
  assert.equal(free.control, false);
  assert.equal(Setup.canLeaveStep1(draft), true);
  assert.equal(Setup.hasLockedPump(draft), true);
});

test("locked pumps cannot be unmarked", () => {
  const draft = Setup.initialDraft(configuredOutputs, config);
  const after = plain(Setup.togglePump(draft, "board/2"));
  assert.equal(after.rows[1].pump, true);
  assert.equal(plain(Setup.chooseNoPump(draft)).rows[1].pump, true);
});

test("marking a pump unlocks NEXT; I HAVE NO PUMP too", () => {
  let draft = Setup.initialDraft(freshOutputs, empty);
  const marked = plain(Setup.togglePump(draft, "board/2"));
  assert.deepEqual([marked.rows[1].pump, marked.rows[1].id, marked.rows[1].label], [true, "pump", "Pump"]);
  assert.equal(Setup.canLeaveStep1(marked), true);
  assert.equal(Setup.next(marked).step, 2);

  const none = plain(Setup.chooseNoPump(marked));
  assert.equal(none.noPump, true);
  assert.equal(none.rows[1].pump, false, "no pump unmarks what was marked");
  assert.equal(Setup.canLeaveStep1(none), true);

  // Marking one again cancels "no pump".
  assert.equal(plain(Setup.togglePump(none, "board/4")).noPump, false);
  assert.equal(plain(draft).rows[1].pump, false, "input never changes");
});

test("several pumps get numbered ids and labels", () => {
  let draft = Setup.initialDraft(freshOutputs, empty);
  draft = Setup.togglePump(draft, "board/2");
  draft = Setup.togglePump(draft, "board/4");
  draft = Setup.togglePump(draft, "board/5");
  const pumps = plain(Setup.payload(draft, STARTER)).pumps;
  assert.deepEqual(pumps.map((p) => [p.pwm, p.id, p.label]), [
    [2, "pump", "Pump"],
    [4, "pump-2", "Pump 2"],
    [5, "pump-3", "Pump 3"],
  ]);
  assert.deepEqual(plain(Setup.summary(draft, STARTER)).pumps.map((p) => p.name), ["PUMP", "PUMP 2", "PUMP 3"]);
  assert.deepEqual(plain(Setup.validateDraft(draft)), {});

  // Unmarking the first frees "pump" for the next one marked.
  draft = Setup.togglePump(draft, "board/2");
  draft = Setup.togglePump(draft, "board/1");
  const ids = plain(Setup.payload(draft, STARTER)).pumps.map((p) => p.id);
  assert.deepEqual(ids, ["pump", "pump-2", "pump-3"]);
  assert.equal(new Set(ids).size, ids.length);
});

test("a new pump next to a locked one is numbered after it", () => {
  const draft = Setup.togglePump(Setup.initialDraft(configuredOutputs, config), "board/4");
  const pumps = plain(Setup.payload(draft, STARTER)).pumps;
  assert.deepEqual(pumps.map((p) => [p.id, p.label]), [["pump", "AIO pump"], ["pump-2", "Pump 2"]]);
});

test("pumpLabel follows the proposed id", () => {
  assert.equal(Setup.pumpLabel("pump"), "Pump");
  assert.equal(Setup.pumpLabel("pump-2"), "Pump 2");
  assert.equal(Setup.pumpLabel("pump-12"), "Pump 12");
});

test("a fan marked as pump by mistake gets its fields back", () => {
  const draft = Setup.initialDraft(configuredOutputs, config);
  const pumped = Setup.togglePump(draft, "board/1");
  assert.equal(plain(pumped).rows[0].control, false);
  const back = plain(Setup.togglePump(pumped, "board/1"));
  assert.deepEqual([back.rows[0].control, back.rows[0].label, back.rows[0].id], [true, "Radiator fans", "radiator"]);
});

test("CONTROL fills defaults and the id follows the label until edited", () => {
  let draft = Setup.togglePump(Setup.initialDraft(freshOutputs, empty), "board/2");
  draft = Setup.toggleControl(draft, "board/3", "cpu/temp1");
  let row = plain(draft).rows[2];
  assert.deepEqual([row.control, row.label, row.id, row.sensor, row.minPercent], [true, "Fan 3", "fan-3", "cpu/temp1", 20]);

  draft = Setup.setField(draft, "board/3", "label", "Case fans");
  assert.equal(plain(draft).rows[2].id, "case-fans");

  draft = Setup.setField(draft, "board/3", "id", "case");
  draft = Setup.setField(draft, "board/3", "label", "Front fans");
  row = plain(draft).rows[2];
  assert.deepEqual([row.label, row.id], ["Front fans", "case"], "an edited id stays");

  // Pumps cannot be controlled.
  assert.equal(plain(Setup.toggleControl(draft, "board/2")).rows[1].control, false);
});

test("proposed ids never clash with other fans or pumps", () => {
  let draft = Setup.togglePump(Setup.initialDraft(freshOutputs, empty), "board/2");
  draft = Setup.toggleControl(draft, "board/1");
  draft = Setup.setField(draft, "board/1", "label", "Pump");
  assert.equal(plain(draft).rows[0].id, "pump-2");
  draft = Setup.toggleControl(draft, "board/3");
  draft = Setup.setField(draft, "board/3", "label", "Pump");
  assert.equal(plain(draft).rows[2].id, "pump-3");
});

test("minimum % stays inside the row's range", () => {
  let draft = Setup.toggleControl(Setup.initialDraft(freshOutputs, empty), "board/1");
  assert.equal(plain(Setup.setField(draft, "board/1", "minPercent", 80)).rows[0].minPercent, 60);
  assert.equal(plain(Setup.setField(draft, "board/1", "minPercent", 2)).rows[0].minPercent, 10);
  assert.equal(plain(Setup.setField(draft, "board/1", "minPercent", 33.6)).rows[0].minPercent, 34);
});

test("validation names the problem per row and gates step 2", () => {
  let draft = Setup.chooseNoPump(Setup.initialDraft(freshOutputs, empty));
  draft = Setup.next(draft);
  assert.equal(draft.step, 2);
  assert.equal(Setup.canLeaveStep2(draft), true, "no fans is allowed: all stay with the BIOS");

  draft = Setup.toggleControl(draft, "board/1");
  draft = Setup.toggleControl(draft, "board/3");
  assert.deepEqual(plain(Setup.validateDraft(draft)), {});

  const named = (d, key, field, value) => Setup.setField(d, key, field, value);
  assert.deepEqual(plain(Setup.validateDraft(named(draft, "board/1", "label", "  "))), { "board/1": "give it a name" });
  assert.deepEqual(plain(Setup.validateDraft(named(draft, "board/1", "id", "Case Fans"))), { "board/1": "id: lowercase letters, digits and - only" });
  assert.deepEqual(plain(Setup.validateDraft(named(draft, "board/3", "id", "fan-1"))), { "board/3": "id 'fan-1' is taken" });
  assert.deepEqual(plain(Setup.validateDraft(named(draft, "board/3", "sensor", ""))), { "board/3": "pick a sensor" });

  const bad = named(draft, "board/3", "id", "");
  assert.equal(Setup.canLeaveStep2(bad), false);
  assert.equal(Setup.next(bad).step, 2);
  assert.equal(Setup.back(bad).step, 1);
});

test("sensor options use config aliases, matched like the daemon", () => {
  const options = plain(Setup.sensorOptions(config, status));
  assert.deepEqual(options.map((o) => o.value), ["board/temp2", "cpu/temp1", "cpu/temp3"]);
  assert.equal(options[1].label, "cpu/temp1 · Tctl · 47 °C");
  assert.equal(options[0].label, "board/temp2 · — °C");
  assert.equal(Setup.defaultSensor(options), "cpu/temp1");
  assert.equal(Setup.defaultSensor([{ value: "board/temp2" }]), "board/temp2");
  assert.equal(Setup.defaultSensor([]), "cpu/temp1");
  assert.deepEqual(plain(Setup.sensorOptions(config, null)), []);

  assert.equal(Setup.chipNameMatches("it8689", "it8689_61030507"), true);
  assert.equal(Setup.chipNameMatches("it8689", "it86890"), false);
  // Two chips match one name: ambiguous, so no options from it.
  const twice = { chips: [...status.chips, { name: "it8689", temps: [{ index: 1, celsius: 30 }], fans: [] }] };
  assert.deepEqual(plain(Setup.sensorOptions(config, twice)).map((o) => o.value), ["cpu/temp1", "cpu/temp3"]);
});

test("tachometer options and the no-reading note", () => {
  const options = plain(Setup.rpmInputOptions(config, status, "board"));
  assert.deepEqual(options, [
    { value: 1, label: "fan1 · 1477 RPM" },
    { value: 3, label: "fan3 · 815 RPM" },
  ]);
  assert.equal(Setup.optionLabel(options, 3), "fan3 · 815 RPM");
  assert.equal(Setup.optionLabel(options, null), "—");
  assert.equal(Setup.noTachometer({ rpm: 0 }), true);
  assert.equal(Setup.noTachometer({ rpm: null }), true);
  assert.equal(Setup.noTachometer({ rpm: 900 }), false);
});

test("starter curve preview is clamped to the minimum", () => {
  assert.deepEqual(plain(Setup.starterCurve(STARTER, 20)), STARTER);
  assert.deepEqual(plain(Setup.starterCurve(STARTER, 55)), [[40, 55], [60, 55], [75, 80], [85, 100]]);
});

test("the spec scenario: pwm2 pump, pwm1 and pwm3 fans", () => {
  let draft = Setup.initialDraft(freshOutputs, empty);
  draft = Setup.togglePump(draft, "board/2");
  draft = Setup.next(draft);
  draft = Setup.toggleControl(draft, "board/1", "cpu/temp1");
  draft = Setup.setField(draft, "board/1", "label", "Radiator fans");
  draft = Setup.setField(draft, "board/1", "id", "radiator");
  draft = Setup.setField(draft, "board/1", "minPercent", 25);
  draft = Setup.toggleControl(draft, "board/3", "cpu/temp1");
  draft = Setup.setField(draft, "board/3", "label", "Case fans");
  draft = Setup.setField(draft, "board/3", "id", "case");
  draft = Setup.next(draft);
  assert.equal(draft.step, 3);
  assert.equal(Setup.next(draft).step, 3, "APPLY, not NEXT, on the last step");

  const summary = plain(Setup.summary(draft, STARTER));
  assert.deepEqual(summary.fans.map((f) => [f.name, f.line]), [
    ["RADIATOR FANS", "pwm1 · cpu/temp1 · min 25%"],
    ["CASE FANS", "pwm3 · cpu/temp1 · min 20%"],
  ]);
  assert.deepEqual(summary.fans[0].curve, [[40, 30], [60, 50], [75, 80], [85, 100]]);
  assert.deepEqual(summary.pumps, [{ key: "board/2", name: "PUMP", line: "pwm2 · board" }]);

  assert.deepEqual(plain(Setup.payload(draft, STARTER)), {
    pumps: [{ id: "pump", label: "Pump", chip: "board", pwm: 2 }],
    fans: [
      { id: "radiator", label: "Radiator fans", chip: "board", pwm: 1, sensor: "cpu/temp1", min_percent: 25, curve: [[40, 30], [60, 50], [75, 80], [85, 100]], rpm_input: 1 },
      { id: "case", label: "Case fans", chip: "board", pwm: 3, sensor: "cpu/temp1", min_percent: 20, curve: [[40, 30], [60, 50], [75, 80], [85, 100]], rpm_input: 3 },
    ],
  });
});

test("re-running the wizard keeps existing curves and pumps", () => {
  const draft = Setup.initialDraft(configuredOutputs, config);
  const body = plain(Setup.payload(draft, STARTER));
  assert.deepEqual(body.pumps, [{ id: "pump", label: "AIO pump", chip: "board", pwm: 2 }]);
  assert.deepEqual(body.fans.map((f) => [f.id, f.curve, f.min_percent, f.sensor]), [
    ["radiator", [[40, 30], [85, 100]], 25, "cpu/temp1"],
    ["case", [[40, 25], [80, 100]], 5, "board/temp2"],
  ]);
  assert.equal(body.fans[0].rpm_input, 3);
});

test("fans without a tachometer send no rpm_input", () => {
  let draft = Setup.chooseNoPump(Setup.initialDraft(freshOutputs, empty));
  draft = Setup.toggleControl(draft, "board/4");
  const fan = plain(Setup.payload(draft, STARTER)).fans[0];
  assert.equal(fan.pwm, 4);
  assert.equal("rpm_input" in fan, false);
});

test("live readings refresh without touching choices", () => {
  let draft = Setup.togglePump(Setup.initialDraft(freshOutputs, empty), "board/2");
  draft = Setup.toggleControl(draft, "board/1");
  const refreshed = freshOutputs.map((o) => ({ ...o, rpm: o.pwm === 1 ? 1500 : o.rpm }));
  refreshed.push({ ...output(6), chip: "board" });
  const merged = plain(Setup.mergeOutputs(draft, refreshed, empty));
  assert.equal(merged.rows[0].rpm, 1500);
  assert.equal(merged.rows[0].control, true);
  assert.equal(merged.rows[1].pump, true);
  assert.equal(merged.rows.length, 6);
  assert.equal(plain(draft).rows[0].rpm, 801, "input never changes");
});
