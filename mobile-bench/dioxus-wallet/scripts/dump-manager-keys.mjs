#!/usr/bin/env node
//
// One-shot helper: decrypts `manager-secrets.json` from the
// midnight-did-manager profile, extracts each stored key's raw
// 32-byte private scalar from its PKCS8-DER record, and writes a
// flat JSON array to `/tmp/manager-keys.json` that the Rust
// `import_manager_keys` test consumes.
//
// Usage:
//   node mobile-bench/dioxus-wallet/scripts/dump-manager-keys.mjs
//
// Env overrides:
//   MANAGER_SECRETS  — path to manager-secrets.json
//     (default ~/.midnight-did/profiles/preprod/preproad-default/manager-secrets.json)
//   MANAGER_PASS     — passphrase
//     (default "midnight-dev-passphrase" — the upstream
//      `did-manager-service`'s `defaultSecretPassphrase`,
//      see ~/iohk/midnight-did/did-manager-service/src/config.ts:73)
//   OUT_FILE         — output path (default /tmp/manager-keys.json)
//
// Output JSON shape:
//   [{
//     "id": "key-1",
//     "key_ref": "06323caf-c779-4827-a0d4-8636a8ab6bca",
//     "kty": "OKP",
//     "crv": "Ed25519",
//     "private_key_hex": "32d9ca8dfade9e78f9a6736eb4...",
//     "public_jwk": { "kty": "OKP", "crv": "Ed25519", "x": "..." }
//   }, ...]

import {
  createDecipheriv,
  createPrivateKey,
  scrypt as scryptCb,
} from "node:crypto";
import { promisify } from "node:util";
import { readFileSync, writeFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";

const scrypt = promisify(scryptCb);

const MANAGER_SECRETS =
  process.env.MANAGER_SECRETS ??
  path.join(
    os.homedir(),
    ".midnight-did/profiles/preprod/preproad-default/manager-secrets.json",
  );
const MANAGER_PASS = process.env.MANAGER_PASS ?? "midnight-dev-passphrase";
const OUT_FILE = process.env.OUT_FILE ?? "/tmp/manager-keys.json";

// Step 1: decrypt the envelope. Mirrors
// `@midnight-ntwrk/midnight-did-secret-storage/dist/crypto.js`.
async function decryptManagerSecrets(envelope, pass) {
  const salt = Buffer.from(envelope.salt, "base64");
  const iv = Buffer.from(envelope.iv, "base64");
  const tag = Buffer.from(envelope.tag, "base64");
  const ct = Buffer.from(envelope.ciphertext, "base64");
  const key = await scrypt(pass, salt, 32);
  const d = createDecipheriv("aes-256-gcm", key, iv);
  d.setAuthTag(tag);
  return Buffer.concat([d.update(ct), d.final()]).toString("utf8");
}

// Step 2: PKCS8-DER → raw scalar. Node's `createPrivateKey`
// parses any PKCS8 KeyObject; `.export({format: "jwk"})` gives us
// the JWK with `d` carrying the base64url-encoded raw scalar.
// Works for Ed25519, P-256, X25519 — the three curves the manager
// supports.
function pkcs8ToRawScalar(pkcs8DerBase64) {
  const der = Buffer.from(pkcs8DerBase64, "base64");
  const ko = createPrivateKey({ key: der, format: "der", type: "pkcs8" });
  const jwk = ko.export({ format: "jwk" });
  if (!jwk.d) {
    throw new Error(`JWK has no 'd' field: ${JSON.stringify(jwk)}`);
  }
  // base64url → bytes
  const norm = jwk.d.replace(/-/g, "+").replace(/_/g, "/");
  const pad = norm.length % 4 ? "=".repeat(4 - (norm.length % 4)) : "";
  return Buffer.from(norm + pad, "base64");
}

const wrapped = JSON.parse(readFileSync(MANAGER_SECRETS, "utf8"));
const decrypted = await decryptManagerSecrets(wrapped.encrypted, MANAGER_PASS);
const store = JSON.parse(decrypted);

const out = [];
for (const [keyRef, entry] of Object.entries(store.keys ?? {})) {
  const meta = entry.meta ?? {};
  const priv = entry.privateRecord ?? {};
  const publicJwk = entry.publicJwk ?? {};
  if (!priv.privateKey) {
    console.warn(`[skip] key ${keyRef}: no privateKey field`);
    continue;
  }
  let raw;
  try {
    if (priv.encoding === "pkcs8-der") {
      raw = pkcs8ToRawScalar(priv.privateKey);
    } else if (priv.encoding === "raw32") {
      // Jubjub keys ship as base64 of 32 raw scalar bytes.
      raw = Buffer.from(priv.privateKey, "base64");
      if (raw.length !== 32) {
        throw new Error(`raw32 wrong length: ${raw.length}`);
      }
    } else {
      throw new Error(`unsupported encoding "${priv.encoding}"`);
    }
  } catch (e) {
    console.warn(`[skip] key ${keyRef}: ${e.message}`);
    continue;
  }
  out.push({
    id: meta.id ?? keyRef,
    key_ref: keyRef,
    kty: priv.kty ?? publicJwk.kty,
    crv: priv.crv ?? publicJwk.crv,
    private_key_hex: raw.toString("hex"),
    public_jwk: publicJwk,
    // Pass through every other field on `meta` so callers can
    // pick up DID associations, purpose tags, etc. without
    // touching this script. Drops `id` since it's already
    // hoisted, and avoids overwriting the canonical fields
    // above.
    meta: Object.fromEntries(
      Object.entries(meta).filter(([k]) => k !== "id"),
    ),
  });
}

writeFileSync(OUT_FILE, JSON.stringify(out, null, 2));
console.log(`wrote ${out.length} keys to ${OUT_FILE}`);
for (const k of out) {
  console.log(`  ${k.id.padEnd(20)} ${k.kty}/${k.crv}  ref=${k.key_ref}`);
}
