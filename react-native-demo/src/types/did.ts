/**
 * DID-screen domain types. Mirrors what the Dioxus wallet's DID
 * tab consumes from the upstream TS contract layer
 * (`@midnight-ntwrk/midnight-did-contract`).
 *
 * Status: the actual contract interaction is stubbed in this RN
 * port — see `src/screens/DidScreen.tsx` for the integration plan.
 * The types here are the contract the RN UI codes against; when
 * the contract bridge lands, the data shapes don't change.
 */

/**
 * Per the Midnight DID spec, a DID document looks like:
 *
 *   did:midnight:<contract-address>
 *     publicKey  : Ed25519 pubkey for tx authentication
 *     also_known_as: [ string, ... ] — DNS / OIDC / web identifiers
 *     services: [{ id, type, endpoint }, ... ]
 *     deactivated: boolean
 *     last_modified_block: u64
 */
export interface DidDocument {
  did: string;                   // "did:midnight:<addr>"
  publicKey: string;             // hex
  alsoKnownAs: string[];
  services: DidService[];
  deactivated: boolean;
  lastModifiedBlock: number;
}

export interface DidService {
  id: string;
  type: string;
  endpoint: string;
}

export type DidOpKind = "resolve" | "deploy" | "update" | "deactivate";

export interface DidOpInFlight {
  kind: DidOpKind;
  startedAtMs: number;
  /** Optional progress note from the wallet's WizardStage stream. */
  status?: string;
}

export interface DidOpResult {
  kind: DidOpKind;
  ok: boolean;
  /** "did:midnight:..." on success; the contract address on deploy. */
  did?: string;
  /** Wall time of the operation in ms. */
  elapsedMs: number;
  /** Optional human-readable error on failure. */
  error?: string;
}
