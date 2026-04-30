// Stub for upstream packages we *don't* want to ship to the WebView
// because they pull in Node-only crypto / file-system that has no
// browser equivalent. Importing the package succeeds (the bundle
// builds); calling any of its members throws loudly.
//
// Phase 4 of the wallet plan replaces these via our own:
//   - secret-storage  → wallet keys live in Rust; signing goes via
//     window.midnightWallet.signData(...)
//   - level-private-state-provider → custom IndexedDB-backed provider
//     (or a Rust-served `mn-state://` URL scheme) when DID create
//     lands.

function trap() {
  throw new Error(
    "midnight-did: upstream Node-only module is not available in the WebView. " +
      "Use the wallet bridge instead (window.midnightWallet.*)."
  );
}

// Recursive proxy: any property access returns *another* proxy, so
// chains like `DIDContract.KeyType.EC` resolve all the way down
// without hitting `undefined`. The terminal call (`DIDContract.foo()`)
// throws.
function makeProxy() {
  return new Proxy(function () {}, {
    get(_target, prop) {
      // `Symbol.toPrimitive`, `then`, `Symbol.iterator` etc. need to
      // return undefined for cross-realm code that probes them; if we
      // return a proxy, things like `await proxy` would mis-trigger.
      if (typeof prop === "symbol") return undefined;
      if (prop === "then") return undefined;
      return makeProxy();
    },
    apply: trap,
    construct: trap,
  });
}

const proxy = makeProxy();

export default proxy;

// Each named export is its own recursive proxy so property access
// chains in upstream code (`DIDContract.KeyType.EC`,
// `compactRuntime.StateValue.newNull()`, etc.) all resolve to
// non-null values at module-eval time. Calling any of them throws.
export const __midnightStub = true;
export const parseSeed = makeProxy();
export const HDWallet = makeProxy();
export const Roles = makeProxy();
export const levelPrivateStateProvider = makeProxy();
export const InMemoryPrivateStateProvider = makeProxy();
// midnight-did-contract
export const DIDContract = makeProxy();
export const witnesses = makeProxy();
// compact-runtime
export const CompactError = class CompactError extends Error {};
export const bigIntToValue = makeProxy();
export const valueToBigInt = makeProxy();
export const ecMul = makeProxy();
export const ecMulGenerator = makeProxy();
export const ecAdd = makeProxy();
export const maxField = 0n;
export const persistentHash = makeProxy();
export const constructorContext = makeProxy();
export const witnessContext = makeProxy();
export const QueryContext = makeProxy();
export const StateValue = makeProxy();
export const ContractState = makeProxy();
export const ContractAddress = makeProxy();
export const decodeContractAddress = makeProxy();
export const encodeContractAddress = makeProxy();
