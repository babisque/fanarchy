import { test } from "node:test";
import assert from "node:assert/strict";
import { loadModel } from "./load.mjs";

const Client = loadModel("FanarchyClient.js");

// The status payload from docs/protocol.md.
function sampleStatus(overrides = {}) {
  return {
    fans: [
      {
        id: "radiator",
        label: "Radiator fans",
        role: "fan",
        sensor: "cpu/temp1",
        temp_c: 48.25,
        rpm: 1180,
        percent: 38,
        min_percent: 25,
        mode: "curve",
        override: "none",
      },
      {
        id: "pump",
        label: "AIO pump",
        role: "pump",
        sensor: null,
        temp_c: null,
        rpm: 2950,
        percent: null,
        min_percent: null,
        mode: null,
        override: null,
      },
    ],
    cpu_temp_c: 48.25,
    critical_temp_c: 85.0,
    gpus: [{ index: 0, name: "NVIDIA GeForce GTX 1660 SUPER", celsius: 41.0 }],
    gpu_note: null,
    chips: [],
    ...overrides,
  };
}

function connectedWith(status) {
  const line = JSON.stringify({ ok: true, data: status });
  return Client.onLine(Client.onConnected(Client.initialState()), line).state;
}

// vm objects come from another realm; compare them as plain JSON.
const plain = (value) => JSON.parse(JSON.stringify(value));

test("subscribe is one JSON line", () => {
  assert.equal(Client.subscribeLine(), '{"cmd":"subscribe"}\n');
  assert.equal(Client.SOCKET_PATH, "/run/fanarchy.sock");
  assert.equal(Client.RECONNECT_MS, 5000);
});

test("clicking toggles the Fanarchy panel", () => {
  assert.equal(Client.togglePanelCommand(), "omarchy-shell shell toggle io.github.babisque.fanarchy");
});

test("starts down and shows a placeholder, never blank", () => {
  const view = Client.view(Client.initialState());
  assert.equal(view.state, "down");
  assert.equal(view.label, "—°");
  assert.equal(view.downReason, "stopped");
});

test("connected before the first status shows a placeholder", () => {
  const view = Client.view(Client.onConnected(Client.initialState()));
  assert.equal(view.state, "normal");
  assert.equal(view.label, "—°");
  assert.equal(view.rows.length, 0);
});

test("a status line becomes the current status", () => {
  const state = connectedWith(sampleStatus());
  const view = Client.view(state);
  assert.equal(view.state, "normal");
  assert.equal(view.label, "48°");
  assert.equal(view.accessibleName, "Fanarchy: CPU 48 degrees");
});

test("blank and malformed lines keep the state", () => {
  const state = connectedWith(sampleStatus());
  const blank = Client.onLine(state, "   ");
  assert.equal(blank.state, state);
  assert.equal(blank.log, null);

  const broken = Client.onLine(state, "{not json");
  assert.equal(broken.state, state);
  assert.match(broken.log, /malformed/);

  const scalar = Client.onLine(state, "42");
  assert.equal(scalar.state, state);
});

test("an error response keeps the last status", () => {
  const state = connectedWith(sampleStatus());
  const result = Client.onLine(state, '{"ok":false,"error":"boom"}');
  assert.equal(result.state.status, state.status);
  assert.equal(result.state.error, "boom");
  assert.match(result.log, /boom/);
});

test("unknown fields are ignored", () => {
  const state = connectedWith(sampleStatus({ future_field: [1, 2, 3] }));
  assert.equal(Client.view(state).label, "48°");
});

test("disconnect forgets the status", () => {
  const state = Client.onDisconnected(connectedWith(sampleStatus()), "");
  assert.equal(state.status, null);
  const view = Client.view(state);
  assert.equal(view.state, "down");
  assert.equal(view.label, "—°");
  assert.equal(view.rows.length, 0);
});

test("running hot starts at critical - 10", () => {
  assert.equal(Client.widgetState(connectedWith(sampleStatus({ cpu_temp_c: 74.9 }))), "normal");
  assert.equal(Client.widgetState(connectedWith(sampleStatus({ cpu_temp_c: 75.0 }))), "hot");
  const hot = Client.view(connectedWith(sampleStatus({ cpu_temp_c: 86.2 })));
  assert.equal(hot.label, "86° !");
  assert.equal(hot.accessibleName, "Fanarchy: CPU 86 degrees, running hot");
});

test("hot threshold follows the configured critical temperature", () => {
  const status = sampleStatus({ cpu_temp_c: 62, critical_temp_c: 70 });
  assert.equal(Client.widgetState(connectedWith(status)), "hot");
});

test("unreadable CPU is normal with a placeholder", () => {
  const view = Client.view(connectedWith(sampleStatus({ cpu_temp_c: null })));
  assert.equal(view.state, "normal");
  assert.equal(view.label, "—°");
});

test("tooltip rows list GPU then fans in config order", () => {
  const rows = plain(Client.tooltipRows(sampleStatus()));
  assert.deepEqual(rows, [
    { name: "GPU", value: "41 °C" },
    { name: "Radiator fans", value: "1180 RPM · 38%" },
    { name: "AIO pump", value: "2950 RPM · AUTO" },
  ]);
});

test("several GPUs are numbered", () => {
  const status = sampleStatus({
    gpus: [
      { index: 0, name: "a", celsius: 40 },
      { index: 1, name: "b", celsius: null },
    ],
  });
  const rows = plain(Client.tooltipRows(status)).slice(0, 2);
  assert.deepEqual(rows, [
    { name: "GPU 1", value: "40 °C" },
    { name: "GPU 2", value: "— °C" },
  ]);
});

test("a gpu_note shows a placeholder GPU row; no GPU shows none", () => {
  const noted = plain(Client.tooltipRows(sampleStatus({ gpus: [], gpu_note: "nvidia-smi failed" })));
  assert.deepEqual(noted[0], { name: "GPU", value: "— °C" });
  const none = plain(Client.tooltipRows(sampleStatus({ gpus: [], gpu_note: null })));
  assert.equal(none[0].name, "Radiator fans");
});

test("fans without a label fall back to their id", () => {
  const status = sampleStatus({ fans: [{ id: "case", role: "fan", rpm: 900, percent: 30 }] });
  assert.deepEqual(plain(Client.tooltipRows(status)).at(-1), {
    name: "case",
    value: "900 RPM · 30%",
  });
});

test("down reason tells permission problems apart", () => {
  assert.equal(Client.downReason("Socket access error"), "permission");
  assert.equal(Client.downReason("Permission denied"), "permission");
  assert.equal(Client.downReason("Server not found"), "stopped");
  assert.equal(Client.downReason(null), "stopped");
  const state = Client.onDisconnected(Client.initialState(), "Socket access error");
  assert.equal(Client.view(state).downReason, "permission");
});

test("socket error codes map to readable text", () => {
  assert.equal(Client.socketErrorText(3), "access denied");
  assert.equal(Client.socketErrorText(2), "server not found");
  assert.equal(Client.socketErrorText(42), "socket error 42");
  assert.equal(Client.downReason(Client.socketErrorText(3)), "permission");
  assert.equal(Client.downReason(Client.socketErrorText(0)), "stopped");
});

test("transitions never mutate their input", () => {
  const start = Client.initialState();
  const frozen = JSON.stringify(start);
  Client.onConnected(start);
  Client.onDisconnected(start, "x");
  Client.onLine(start, JSON.stringify({ ok: true, data: sampleStatus() }));
  assert.equal(JSON.stringify(start), frozen);
});

// ---------------------------------------------------------------- commands

test("request lines are single JSON lines", () => {
  assert.equal(Client.requestLine(Client.getConfigRequest()), '{"cmd":"get_config"}\n');
  assert.equal(
    Client.requestLine(Client.setCurveRequest("case", [[30, 25], [80, 100]])),
    '{"cmd":"set_curve","fan":"case","points":[[30,25],[80,100]]}\n',
  );
});

test("set_mode only sends percent with fixed", () => {
  assert.deepEqual(plain(Client.setModeRequest("case", "fixed", 59.6)), {
    cmd: "set_mode", fan: "case", mode: "fixed", percent: 60,
  });
  assert.deepEqual(plain(Client.setModeRequest("case", "auto", 60)), { cmd: "set_mode", fan: "case", mode: "auto" });
  assert.deepEqual(plain(Client.setModeRequest("case", "curve")), { cmd: "set_mode", fan: "case", mode: "curve" });
});

test("replies settle requests in order", () => {
  let queue = Client.enqueue([], Client.getConfigRequest());
  queue = Client.enqueue(queue, Client.setCurveRequest("case", [[30, 25], [80, 100]]));
  assert.equal(queue.length, 2);

  const first = Client.settle(queue, '{"ok":true,"data":{"version":1}}');
  assert.equal(first.request.cmd, "get_config");
  assert.equal(first.ok, true);
  assert.equal(first.data.version, 1);

  const second = Client.settle(first.queue, "{\"ok\":false,\"error\":\"fan 'case' is a pump and cannot be controlled\"}");
  assert.equal(second.request.cmd, "set_curve");
  assert.equal(second.ok, false);
  assert.equal(second.error, "fan 'case' is a pump and cannot be controlled");
  assert.equal(second.queue.length, 0);
});

test("an ok reply without data settles with null", () => {
  const result = Client.settle([Client.setModeRequest("case", "auto")], '{"ok":true}');
  assert.equal(result.ok, true);
  assert.equal(result.data, null);
});

test("blank, unreadable and unexpected replies", () => {
  const queue = [Client.getConfigRequest()];
  const blank = Client.settle(queue, "  ");
  assert.equal(blank.request, null);
  assert.equal(blank.queue, queue);

  const broken = Client.settle(queue, "{nope");
  assert.equal(broken.request.cmd, "get_config");
  assert.equal(broken.ok, false);
  assert.equal(broken.queue.length, 0);

  const stray = Client.settle([], '{"ok":true}');
  assert.equal(stray.request, null);
  assert.match(stray.log, /no request/);
});

test("panel toggle prefers the shell facade over the CLI", () => {
  assert.equal(Client.panelToggleRoute({ toggle() {}, hide() {} }), "api");
  assert.equal(Client.panelToggleRoute({ hide() {} }), "command");
  assert.equal(Client.panelToggleRoute(null), "command");
  assert.equal(Client.panelToggleRoute(undefined), "command");
  assert.equal(Client.togglePanelCommand(), "omarchy-shell shell toggle io.github.babisque.fanarchy");
});
