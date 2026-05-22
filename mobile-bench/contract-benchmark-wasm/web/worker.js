// This module runs *inside* a dedicated Web Worker. The
// `wasm-bindgen-rayon` thread pool can only be initialised
// from a context whose `WebAssembly.Memory` was created as
// shared — and that means running the wasm in a worker
// rather than on the main page. Mirrors the
// `wasm-proving-demos/webpage/src/workerMt.js` pattern.
//
// Protocol with the main page:
//   - On boot: `postMessage({ type: "ready", threads: N })`
//     after `init()` + `initThreadPool()` complete.
//   - Receive `{ type: "run", k: number, rid: string }`.
//   - Reply `{ type: "result", rid, stats }` or
//     `{ type: "error", rid, message }`.
//   - Receive `{ type: "clearCache" }` to drop the IndexedDB
//     SRS cache.

import init, { runProof, initThreadPool } from "./pkg/contract_benchmark_wasm.js";

// ─── IndexedDB-backed SRS cache (worker side) ─────────────────

const DB_NAME = "midnight-srs-cache";
const STORE = "params";

function openDb() {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open(DB_NAME, 1);
    req.onupgradeneeded = () => req.result.createObjectStore(STORE);
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}

async function cacheGet(key) {
  const db = await openDb();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE, "readonly");
    const req = tx.objectStore(STORE).get(key);
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}

async function cachePut(key, value) {
  const db = await openDb();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE, "readwrite");
    tx.objectStore(STORE).put(value, key);
    tx.oncomplete = () => resolve();
    tx.onerror = () => reject(tx.error);
  });
}

async function cacheClear() {
  const db = await openDb();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE, "readwrite");
    tx.objectStore(STORE).clear();
    tx.oncomplete = () => resolve();
    tx.onerror = () => reject(tx.error);
  });
}

function log(msg) {
  postMessage({ type: "log", msg });
}

// ─── Params provider — handed to runProof ─────────────────────
//
// In-memory memoisation on top of IndexedDB means each
// `runProof(k)` only causes ONE fetch + parse per distinct k
// across the worker's lifetime — even though the Rust side calls
// `get_params(k)` 2-3 times per prove (keygen + prove + resolver
// chain). Saves ~5–10% at k ≥ 15 (one of the perf-punch-list
// items the architecture doc flags).

const memo = new Map();

const provider = {
  async getParams(k) {
    const cached = memo.get(k);
    if (cached) return cached;
    const name = `bls_midnight_2p${k}`;
    const indexed = await cacheGet(name);
    let bytes;
    if (indexed) {
      log(`worker: cache hit ${name} (${(indexed.byteLength / 1024).toFixed(1)} KiB)`);
      bytes = new Uint8Array(indexed);
    } else {
      const url = `/srs/${name}`;
      log(`worker: fetching ${url} …`);
      const t0 = performance.now();
      const res = await fetch(url);
      if (!res.ok) throw new Error(`fetch ${name}: ${res.status} ${res.statusText}`);
      const buf = await res.arrayBuffer();
      const dt = performance.now() - t0;
      log(`worker: fetched ${name} (${(buf.byteLength / 1024 / 1024).toFixed(2)} MiB, ${dt.toFixed(0)} ms)`);
      await cachePut(name, buf).catch((e) => log(`worker: cache put failed: ${e}`));
      bytes = new Uint8Array(buf);
    }
    memo.set(k, bytes);
    return bytes;
  },
};

// ─── Boot: load wasm + start the thread pool ─────────────────

(async () => {
  try {
    log("worker: init() starting");
    await init();
    log("worker: init() done");
    // Try to bring up the rayon thread pool. If it fails — most
    // commonly because the wasm Memory is not `shared`, an
    // upstream wasm-bindgen/wasm-bindgen-rayon configuration
    // snag we haven't fully resolved yet — fall through to
    // single-threaded mode rather than blocking the whole
    // bench. Reported back to the main page as `threads = 1`.
    const target = self.navigator.hardwareConcurrency || 4;
    let threads = 1;
    try {
      log(`worker: initThreadPool(${target}) starting`);
      await initThreadPool(target);
      threads = target;
      log("worker: initThreadPool done");
    } catch (e) {
      log(`worker: initThreadPool failed (falling back to 1 thread): ${e?.message ?? e}`);
    }
    postMessage({ type: "ready", threads });
  } catch (e) {
    log(`worker boot error: ${e?.message ?? e}`);
    postMessage({ type: "error", rid: null, message: `boot: ${e?.message ?? e}` });
  }
})();

// ─── Message handler ─────────────────────────────────────────

onmessage = async (e) => {
  const m = e.data;
  if (m.type === "run") {
    try {
      const stats = await runProof(m.k, provider);
      postMessage({ type: "result", rid: m.rid, stats });
    } catch (err) {
      postMessage({ type: "error", rid: m.rid, message: err.message ?? String(err) });
    }
    return;
  }
  if (m.type === "clearCache") {
    memo.clear();
    await cacheClear();
    postMessage({ type: "log", msg: "worker: SRS cache cleared" });
    return;
  }
};
