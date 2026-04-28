import type {
    AlignedValue,
    Op,
    QueryContext,
    StateValue,
    ContractAddress
} from '@midnight-ntwrk/onchain-runtime';

export type ProvingProvider = {
  check(
    serializedPreimage: Uint8Array,
    keyLocation: string,
  ): Promise<(bigint | undefined)[]>;
  prove(
    serializedPreimage: Uint8Array,
    keyLocation: string,
    overwriteBindingInput?: bigint,
  ): Promise<Uint8Array>;
};

export type ProvingKeyMaterial = {
  proverKey: Uint8Array,
  verifierKey: Uint8Array,
  ir: Uint8Array,
};

export type KeyMaterialProvider = {
    lookupKey(keyLocation: string): Promise<ProvingKeyMaterial | undefined>;
    getParams(k: number): Promise<Uint8Array>;
}

export function prove(
    serializedPreimage: Uint8Array,
    provider: KeyMaterialProvider,
    overwriteBindingInput?: bigint,
): Promise<Uint8Array>;

export function check(
    serializedPreimage: Uint8Array,
    provider: KeyMaterialProvider,
): Promise<(bigint | undefined)[]>;

export function provingProvider(kmProvider: KeyMaterialProvider): ProvingProvider;

export function jsonIrToBinary(json: String): Uint8Array;

export class Zkir {
  static fromJson(json: string): Zkir;
  static deserialize(bytes: Uint8Array): Zkir;

  private constructor();
  getK(): number;
  serialize(): Uint8Array;
}

/**
 * Provider interface consulted during execution to fetch the IR and ledger
 * state of every contract that participates in the call tree — including
 * the top-level contract (whose address is `context.address`) and every
 * callee invoked transitively via `ContractCall`.
 *
 * TODO: `getWitness` is part of the surface for forward-compatibility but
 * is **not** consulted by the current `execute` implementation. Top-level
 * circuits with `PrivateInput` instructions will fail.
 */
export type ExecutionProvider = {
    /**
     * Resolve the contract at `address` for the given `entryPoint`,
     * returning the compiled `Zkir` and current ledger `state` together
     * as a single object so the executor observes a consistent snapshot.
     * Implementations backed by mutable stores must serialize their reads
     * (or return `undefined`) rather than expose a torn snapshot. Resolve
     * to `undefined` if the contract or entry point is unknown.
     */
    getContract(
        address: ContractAddress,
        entryPoint: string,
    ): Promise<{ zkir: Zkir; state: StateValue } | undefined>;
    /**
     * Resolve a named witness function. Reserved for future use; the
     * current `execute` implementation does not call this.
     */
    getWitness(address: ContractAddress, witnessName: string): ((args: any[]) => any) | undefined;
};

/**
 * Parameters describing a single execution. `address` and `circuit` together identify the top-level call.
 *
 * `inputs` is a single `AlignedValue` whose `value_only_field_repr` is the
 * flat field-element representation of the circuit's typed inputs, in
 * declaration order. Build it from the Compact-emitted descriptors, e.g.
 *
 *     const inputs: AlignedValue = {
 *         value: descs.flatMap((d, i) => d.toValue(args[i])),
 *         alignment: descs.flatMap(d => d.alignment()),
 *     };
 *
 * `maxCallDepth` caps recursion through `ContractCall`.
 */
export type ExecutionContext = {
    input: AlignedValue;
    address: ContractAddress;
    circuit: string;
    maxCallDepth: number;
};

/**
 * Role-specific data carried by a `Call`:
 *   - `root` — top-level invocation.
 *   - `sub`  — invoked via `ContractCall`; carries the communication
 *     commitment that links its proof to its parent's claim.
 */
export type CallRole =
    | { kind: 'root' }
    | { kind: 'sub'; commComm: bigint; commCommRand: bigint };

/**
 * One execution of a contract circuit. Field accessors are getters that
 * marshal the underlying Rust data lazily.
 *
 * `input` and `output` are single `AlignedValue`s reconstructed from the
 * executor's flat `Vec<Fr>` via `Alignment::parse_field_repr`. The alignment
 * is derived from the circuit's IR-level typed inputs (concatenation of
 * `IrType::alignment()` over `IrSource.inputs` for `input`; `[Field; n]`
 * for `output`, since each `Output` instruction emits a single `Fr`).
 *
 * `value_only_field_repr` of either side is the canonical `Vec<Fr>` the
 * executor uses for commitment computation (`comm_comm =
 * transient_commit(input ∥ output, rand)`). Compact descriptors read each
 * logical value back via `desc.fromValue(call.input.value)` chained through
 * `Value.shift()`.
 */
export class Call {
    private constructor();
    /** Hex-encoded contract address (no `0x` prefix). */
    readonly address: ContractAddress;
    /** Entry-point name (UTF-8 decoded from `EntryPointBuf` bytes). */
    readonly circuit: string;
    /** Concatenated input values. */
    readonly input: AlignedValue;
    /** Concatenated output values. */
    readonly output: AlignedValue;
    /** Verify-mode public-transcript program, with read results filled in. */
    readonly program: Op<AlignedValue>[];
    /** Query context after this call's Impact blocks executed. */
    readonly context: QueryContext;
    /** Index of this call's parent in the enclosing array, or `null` for root. */
    readonly parent: number | null;
    /** Role-specific data — see `CallRole`. */
    readonly role: CallRole;
    /**
     * The `ContractCallPrototype.private_transcript_outputs`-shaped witness
     * sequence for this call, in instruction-emission order. Each entry is
     * one logical witness:
     *   - one AV per `PrivateInput` instruction (root only — the witness
     *     limitation forbids private inputs in sub-calls), aligned per
     *     that instruction's `val_t`;
     *   - for each `ContractCall`, one single-`Field`-aligned AV per
     *     callee output Fr followed by one single-`Field`-aligned AV for
     *     the parent-supplied `comm_rand` (the Compact-emitted
     *     `tmpDoCall` / `tmpCallRand` pattern).
     *
     * Concatenating each AV's `value_only_field_repr` produces exactly the
     * flat `Vec<Fr>` the proof preimage's `private_transcript` carries —
     * so this array drops directly into the prototype builder.
     */
    readonly privateTranscriptOutputs: AlignedValue[];
}

/**
 * Result of executing an `IrSource`. Always non-empty; index 0 is the
 * root call. Order is depth-first preorder.
 */
export type ExecutionResult = Call[];

/**
 * Execute a ZKIR circuit, fetching IRs and ledger states on demand.
 *
 * The provider's `getContract(context.address, context.circuit)` is
 * invoked once to resolve the top-level call, then again with the
 * appropriate `(address, entryPoint)` pair for each `ContractCall`
 * encountered transitively. The single call returns both the compiled
 * `Zkir` and the contract's ledger state as a consistent snapshot.
 */
export function execute(provider: ExecutionProvider, context: ExecutionContext): Promise<ExecutionResult>;

/**
 * The top-level call in an execution result. Equivalent to `result[0]`
 * but throws a clear error if the array is empty.
 */
export function rootOf(result: ExecutionResult): Call;

/**
 * Immediate children of `parentIndex` in `result`, preserving array order.
 * Equivalent to `result.filter(c => c.parent === parentIndex)`.
 */
export function subCallsOf(result: ExecutionResult, parentIndex: number): Call[];
