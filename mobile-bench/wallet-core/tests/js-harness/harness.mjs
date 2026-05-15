// JSON-RPC harness driven by `NodeChildBridge` in Rust integration
// tests. Reads newline-delimited JSON requests from stdin, writes
// `{ id, result } | { id, error }` responses to stdout. Diagnostics
// go to stderr so they don't pollute the RPC channel.
//
// Method registry grows as we wire up the ContractCall pipeline:
// today it only carries `ping` and `echo` — enough to verify the
// Rust ↔ Node JSON-RPC transport. Step 2 lands `bridgeInspectCircuit`
// and the rest of the DID circuit surface.
//
// Run manually:
//   node mobile-bench/wallet-core/tests/js-harness/harness.mjs
//   {"id":1,"method":"ping"}
//   ← {"id":1,"result":{"ok":true,"version":"0.1.0"}}

import * as readline from "node:readline";

const HARNESS_VERSION = "0.1.0";

/** Dispatch table — method name → async handler(params). */
const methods = {
  ping: async () => ({ ok: true, version: HARNESS_VERSION }),
  echo: async (params) => ({ echoed: params?.message ?? null }),
};

function reply(id, payload) {
  process.stdout.write(JSON.stringify({ id, ...payload }) + "\n");
}

const rl = readline.createInterface({ input: process.stdin });

rl.on("line", async (line) => {
  const trimmed = line.trim();
  if (trimmed === "") return;
  let req;
  try {
    req = JSON.parse(trimmed);
  } catch (e) {
    process.stderr.write(`[harness] bad JSON: ${e.message}\n`);
    return;
  }
  const { id, method, params } = req;
  const handler = methods[method];
  if (typeof handler !== "function") {
    reply(id, { error: `unknown method: ${method}` });
    return;
  }
  try {
    const result = await handler(params);
    reply(id, { result });
  } catch (e) {
    reply(id, { error: e?.stack || String(e?.message || e) });
  }
});

rl.on("close", () => {
  process.stderr.write("[harness] stdin closed; exiting\n");
  process.exit(0);
});

process.stderr.write(`[harness] ready (v${HARNESS_VERSION})\n`);
