#!/usr/bin/env node
/**
 * One-shot dump of the operator's `midnightDIDPrivateState.secretKey`
 * from the encrypted LevelDB the manager-service writes to.
 *
 * The upstream `levelPrivateStateProvider` (used inside
 * `midnight-did/api/src/lib.ts`) wraps every put/get in AES-256-GCM
 * with a PBKDF2-derived key. The password is deterministic:
 *
 *     storagePassword = hex(unshieldedKeystore.getSecretKey()) + "!A"
 *
 * where `unshieldedKeystore.getSecretKey()` returns the raw 32-byte
 * HD child key at `m/44'/2400'/0'/0/0` (`Roles.NightExternal`) — see
 * `midnight-did/api/src/lib.ts::deriveKeysFromSeed`. Since we have
 * the operator's seed, we can recompute that secret and decrypt the
 * LevelDB without running the manager-service or knowing any
 * user-supplied password.
 *
 * The `MidnightDIDPrivateState.secretKey` is the value the contract's
 * `localSecretKey()` witness must return for any `ContractCall` to
 * verify on-chain. Once printed, paste it into
 * `preprod_smoke_live.rs::OPERATOR_CONTROLLER_SK_HEX` and re-run the
 * write test.
 *
 * Run with:
 *
 *   NODE_PATH=$HOME/iohk/midnight-did/node_modules \
 *     node mobile-bench/wallet-core/tests/js-harness/dump_operator_secret.mjs
 *
 * `NODE_PATH` is needed because the upstream packages aren't in our
 * own `node_modules` — we reuse the operator's install.
 */

import { Buffer } from "node:buffer";
import path from "node:path";
import os from "node:os";
import { pathToFileURL } from "node:url";
import { createHash, pbkdf2Sync, createDecipheriv } from "node:crypto";

// ESM doesn't honor NODE_PATH, so resolve upstream packages by
// absolute file URL against the operator's existing install.
const UPSTREAM_NM = path.join(os.homedir(), "iohk", "midnight-did", "node_modules");
// Resolve a package by walking its package.json's main/module entry —
// works for both `<pkg>/dist/index.js` and the `level` shape where
// the entry isn't in `dist/`.
const importUpstream = async (specifier) => {
  const candidates = [
    path.join(UPSTREAM_NM, specifier),
    // level is hoisted under the provider's nested node_modules
    path.join(
      UPSTREAM_NM,
      "@midnight-ntwrk/midnight-js-level-private-state-provider/node_modules",
      specifier,
    ),
  ];
  for (const root of candidates) {
    try {
      const pkg = JSON.parse(
        await (await import("node:fs/promises")).readFile(
          path.join(root, "package.json"),
          "utf8",
        ),
      );
      const entry = pkg.module ?? pkg.main ?? "index.js";
      return await import(pathToFileURL(path.join(root, entry)).href);
    } catch (e) {
      if (e.code !== "ENOENT") throw e;
    }
  }
  throw new Error(`couldn't resolve ${specifier} under ${UPSTREAM_NM}`);
};

const { HDWallet, Roles } = await importUpstream("@midnight-ntwrk/wallet-sdk-hd");
const levelMod = await importUpstream("level");
const Level = levelMod.Level ?? levelMod.default?.Level ?? levelMod.default ?? levelMod;

// Operator's PreProd seed (matches preprod_smoke_live.rs and the
// dioxus-wallet `preprod-live` feature flag).
const SEED_HEX =
  "c1e8d986d10a2aff5d5f6fbf3d568f447b1cd46ccb190f838e0cf2707f5622a2";

// Defaults from `midnight-did/api/src/config.ts`:
// - `contractConfig.privateStateStoreName = "did-private-state"`
// - `MidnightDIDPrivateStateId = "midnightDIDPrivateState"`
const PRIVATE_STATE_STORE_NAME = "did-private-state";
const MIDNIGHT_DID_PRIVATE_STATE_ID = "midnightDIDPrivateState";

// `did-manager-service/src/wallet-state-store.ts::privateStateDbSeedHash`
const privateStateDbSeedHash = (seedHex) =>
  createHash("sha256").update(seedHex).digest("hex").slice(0, 16);

// Profile dir layout (operator's machine):
//   ~/.midnight-did/profiles/preprod/preproad-default/midnight-level-db/<seedHash16>
const PROFILE_ROOT = path.join(
  os.homedir(),
  ".midnight-did",
  "profiles",
  "preprod",
  "preproad-default",
);
const MIDNIGHT_DB_DIR = path.join(
  PROFILE_ROOT,
  "midnight-level-db",
  privateStateDbSeedHash(SEED_HEX),
);

// `midnight-did/api/src/lib.ts::deriveKeysFromSeed` — same call path.
function deriveNightExternalSecret(seedHex) {
  const seedBytes = Buffer.from(seedHex, "hex");
  const hdWallet = HDWallet.fromSeed(seedBytes);
  if (hdWallet.type !== "seedOk") {
    throw new Error(`HDWallet.fromSeed failed: ${hdWallet.type}`);
  }
  const derived = hdWallet.hdWallet
    .selectAccount(0)
    .selectRoles([Roles.NightExternal])
    .deriveKeysAt(0);
  if (derived.type !== "keysDerived") {
    throw new Error(`deriveKeysAt failed: ${derived.type}`);
  }
  hdWallet.hdWallet.clear();
  return Buffer.from(derived.keys[Roles.NightExternal]);
}

// `unshieldedKeystore.getSecretKey()` in `wallet-sdk-unshielded-wallet`
// returns these exact bytes. The accountId for the level provider is
// the wallet's coin public key, but the storage encryption is
// per-account: we can pass *any* accountId so long as the LevelDB
// already has a sublevel for it. We don't know the operator's
// coinPublicKey without re-running the full wallet bootstrap, so we
// dump every sublevel and look for the secret state.
function main() {
  const nightSk = deriveNightExternalSecret(SEED_HEX);
  const password = `${nightSk.toString("hex")}!A`;
  console.log(`[deriv] m/44'/2400'/0'/0/0 = ${nightSk.toString("hex")}`);
  console.log(`[deriv] storage password = ${password.slice(0, 8)}…${password.slice(-2)}`);
  console.log(`[paths] midnight-level-db: ${MIDNIGHT_DB_DIR}`);

  // We don't have the coinPublicKey handy, so iterate over the
  // sublevel directories inside `midnight-level-db/<seedHash>/` —
  // the scoped store name is `<storeName>-<hash6(accountId)>`. Each
  // sublevel is one Level open call, so try the bare provider with
  // a passwordProvider that always returns our derived password and
  // catch decryption errors.
  //
  // Simpler: open Level directly with `level` and enumerate keys
  // matching `${MIDNIGHT_DID_PRIVATE_STATE_ID}` across all
  // sublevels.
  enumerateRaw().catch((e) => {
    console.error(`[fatal] ${e?.stack ?? e}`);
    process.exit(1);
  });
}

async function enumerateRaw() {
  const root = new Level(MIDNIGHT_DB_DIR);
  await root.open();
  console.log(`[level] opened ${MIDNIGHT_DB_DIR}`);

  // `levelPrivateStateProvider` uses subleveldown internally. The
  // top-level db has sublevels keyed by `<store>-<accountIdHash>`.
  // For us, the store name is `did-private-state`, and the account
  // hash is 6 hex chars of sha256(accountId).
  //
  // The encrypted value we want lives at key
  // `<scoped>!midnightDIDPrivateState` (`!` is the subleveldown
  // separator).
  let foundAny = false;
  const out = {};
  for await (const [key, value] of root.iterator()) {
    if (typeof key === "string" && key.includes(MIDNIGHT_DID_PRIVATE_STATE_ID)) {
      foundAny = true;
      console.log(`[level] key=${key} (encrypted ${value.length}B)`);
      out[key] = value;
    }
  }
  if (!foundAny) {
    // Try the explicit subleveldown path: keys start with `!<store>!`.
    console.log("[level] no direct match; iterating ALL keys to find sublevels");
    let count = 0;
    for await (const [key, value] of root.iterator()) {
      count++;
      if (count <= 20) {
        const keyStr = typeof key === "string" ? key : Buffer.from(key).toString();
        console.log(`  [${count}] ${keyStr.slice(0, 80)}  (val ${value.length}B)`);
      }
    }
    console.log(`[level] total keys: ${count}`);
  }
  await root.close();

  // Now decrypt each `midnightDIDPrivateState` blob we found.
  const seedSk = deriveNightExternalSecret(SEED_HEX);
  const password = `${seedSk.toString("hex")}!A`;
  for (const [key, value] of Object.entries(out)) {
    const decrypted = tryDecrypt(value, password);
    if (decrypted) {
      console.log(`\n[ok] ${key}`);
      console.log(`  plaintext (superjson): ${decrypted.slice(0, 400)}${decrypted.length > 400 ? "…" : ""}`);
      // The value is superjson-stringified `{ secretKey: Uint8Array(32) }`.
      try {
        const sjMod = await importUpstream("superjson");
        const sj = sjMod.default ?? sjMod;
        const parsed = sj.parse(decrypted);
        if (parsed && parsed.secretKey instanceof Uint8Array) {
          console.log(`\n[secret] ${Buffer.from(parsed.secretKey).toString("hex")}`);
        } else {
          console.log(`[secret] parsed=${JSON.stringify(parsed).slice(0, 200)}`);
        }
      } catch (e) {
        console.log(`[secret] superjson.parse failed: ${e.message}`);
      }
    }
  }
}

function tryDecrypt(buf, password) {
  // Reproduce `extractEncryptedComponents` from
  // `midnight-js-level-private-state-provider/dist/index.mjs`.
  // The on-disk LevelDB value is a base64 STRING (encrypt() does
  // `.toString('base64')` before storing) — decode that first.
  if (!Buffer.isBuffer(buf)) buf = Buffer.from(buf);
  const b64 = buf.toString("utf8");
  buf = Buffer.from(b64, "base64");
  const VERSION_PREFIX_LENGTH = 1;
  const SALT_LENGTH = 32;
  const IV_LENGTH = 12;
  const AUTH_TAG_LENGTH = 16;
  const HEADER_LENGTH =
    VERSION_PREFIX_LENGTH + SALT_LENGTH + IV_LENGTH + AUTH_TAG_LENGTH;
  if (buf.length < HEADER_LENGTH) {
    console.log(`[skip] too short (${buf.length}B)`);
    return null;
  }
  const version = buf[0];
  const salt = buf.subarray(1, 1 + SALT_LENGTH);
  const iv = buf.subarray(1 + SALT_LENGTH, 1 + SALT_LENGTH + IV_LENGTH);
  const tag = buf.subarray(
    1 + SALT_LENGTH + IV_LENGTH,
    1 + SALT_LENGTH + IV_LENGTH + AUTH_TAG_LENGTH,
  );
  const ct = buf.subarray(HEADER_LENGTH);
  const iters = version === 1 ? 100_000 : 600_000;
  const key = pbkdf2Sync(password, salt, iters, 32, "sha256");
  const decipher = createDecipheriv("aes-256-gcm", key, iv);
  decipher.setAuthTag(tag);
  try {
    const plain = Buffer.concat([decipher.update(ct), decipher.final()]);
    return plain.toString("utf-8");
  } catch (e) {
    console.log(`[err] decrypt failed: ${e.message}`);
    return null;
  }
}

main();
