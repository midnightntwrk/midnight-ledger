/**
 * useKeys — state machine for the Keys screen.
 *
 * Wraps the FFI `Wallet` interface: open or create the wallet
 * file, list / generate / get-public-key / sign / delete.
 *
 * Note: the FFI's `Wallet.new` is synchronous (UniFFI 0.31's
 * sync constructor). We don't wrap in a worker thread because
 * the redb open is fast (~5 ms). For multi-second operations
 * like the 5-minute prove, see `useBench.ts` and the
 * `proveAsync` wrapper.
 */

import { useCallback, useEffect, useReducer, useRef } from "react";
import { Platform } from "react-native";
import { Wallet, type KeyInfo } from "@midnight-ntwrk/react-native-prover";

type Action =
  | { type: "set-status"; status: string }
  | { type: "set-error"; error: string | null }
  | { type: "set-keys"; keys: KeyInfo[] }
  | { type: "open-success"; network: string };

interface State {
  /** Has `Wallet.new(...)` succeeded once? */
  open: boolean;
  network: string | null;
  keys: KeyInfo[];
  status: string;
  error: string | null;
}

const INITIAL: State = {
  open: false,
  network: null,
  keys: [],
  status: "idle",
  error: null,
};

function reducer(state: State, action: Action): State {
  switch (action.type) {
    case "set-status":
      return { ...state, status: action.status };
    case "set-error":
      return { ...state, error: action.error };
    case "set-keys":
      return { ...state, keys: action.keys };
    case "open-success":
      return { ...state, open: true, network: action.network };
    default:
      return state;
  }
}

/**
 * Default wallet-store location for the demo. Real apps should
 * use a private-data path under the platform's app sandbox
 * (Documents/Caches/Application Support).
 */
function defaultStorePath(): string {
  // `react-native-fs` would normally provide platform-specific
  // sandbox paths. For the demo we use a path the simulator /
  // emulator filesystem accepts; on a real device the wallet
  // would supply the path from `Files.dir()`.
  if (Platform.OS === "ios") {
    return "/tmp/midnight-rn-demo-wallet.redb";
  }
  // Android — using cache dir works without extra perms.
  return "/data/local/tmp/midnight-rn-demo-wallet.redb";
}

export function useKeys(network: string = "preprod", passphrase: string = "demo-passphrase") {
  const [state, dispatch] = useReducer(reducer, INITIAL);
  const walletRef = useRef<Wallet | null>(null);

  // Open or create the wallet store on first mount. Idempotent —
  // dispatches `open-success` whether the store was freshly
  // created or already existed.
  useEffect(() => {
    if (walletRef.current !== null) return;
    try {
      dispatch({ type: "set-status", status: "opening store…" });
      const w = new Wallet(
        defaultStorePath(),
        passphrase,
        "demo-wallet",
        network,
        null,
      );
      walletRef.current = w;
      dispatch({ type: "open-success", network: w.network() });
      dispatch({ type: "set-status", status: "ready" });
      // Initial list_keys call so the screen renders any keys
      // present from a prior session.
      refreshKeys();
    } catch (e) {
      dispatch({
        type: "set-error",
        error: e instanceof Error ? e.message : String(e),
      });
      dispatch({ type: "set-status", status: "error opening store" });
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const refreshKeys = useCallback(() => {
    const w = walletRef.current;
    if (!w) return;
    try {
      const ks = w.listKeys();
      dispatch({ type: "set-keys", keys: ks });
      dispatch({ type: "set-error", error: null });
    } catch (e) {
      dispatch({
        type: "set-error",
        error: e instanceof Error ? e.message : String(e),
      });
    }
  }, []);

  const generateKey = useCallback(
    (algorithm: string, label?: string) => {
      const w = walletRef.current;
      if (!w) return;
      try {
        dispatch({ type: "set-status", status: `generating ${algorithm}…` });
        w.generateKey(algorithm, label ?? null);
        dispatch({ type: "set-status", status: "generated" });
        refreshKeys();
      } catch (e) {
        dispatch({
          type: "set-error",
          error: e instanceof Error ? e.message : String(e),
        });
        dispatch({ type: "set-status", status: "error generating" });
      }
    },
    [refreshKeys],
  );

  const deleteKey = useCallback(
    (keyRef: string) => {
      const w = walletRef.current;
      if (!w) return;
      try {
        dispatch({ type: "set-status", status: `deleting ${keyRef}…` });
        w.deleteKey(keyRef);
        dispatch({ type: "set-status", status: "deleted" });
        refreshKeys();
      } catch (e) {
        dispatch({
          type: "set-error",
          error: e instanceof Error ? e.message : String(e),
        });
      }
    },
    [refreshKeys],
  );

  const signPayload = useCallback((keyRef: string, payload: Uint8Array): Uint8Array | null => {
    const w = walletRef.current;
    if (!w) return null;
    try {
      const sig = w.sign(keyRef, payload);
      dispatch({ type: "set-error", error: null });
      return sig;
    } catch (e) {
      dispatch({
        type: "set-error",
        error: e instanceof Error ? e.message : String(e),
      });
      return null;
    }
  }, []);

  return {
    open: state.open,
    network: state.network,
    keys: state.keys,
    status: state.status,
    error: state.error,
    refreshKeys,
    generateKey,
    deleteKey,
    signPayload,
  };
}
