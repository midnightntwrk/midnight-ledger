<% print(fs.readFileSync('../onchain-runtime-wasm/onchain-runtime-v1.d.ts', 'utf8')); %>

/**
 * A zero-knowledge proof.
 */
export class Proof {
  constructor(data: String);
  serialize(): Uint8Array;
  static deserialize(raw: Uint8Array): Proof;
  toString(compact?: boolean): string;
  instance: 'proof';
  private type_: 'proof';
}

/**
 * The preimage, or data required to produce, a {@link Proof}.
 */
export class PreProof {
  constructor(data: String);
  serialize(): Uint8Array;
  static deserialize(raw: Uint8Array): PreProof;
  toString(compact?: boolean): string;
  instance: 'pre-proof';
  private type_: 'pre-proof';
}

/**
 * A unit type used to indicate the absence of proofs.
 */
export class NoProof {
  constructor();
  toString(compact?: boolean): string;
  instance: 'no-proof';
  private type_: 'no-proof';
}

/**
 * How proofs are currently being represented, between:
 * - Actual zero-knowledge proofs, as should be transmitted to the network
 * - The data required to *produce* proofs, for constructing and preparing
 *   transactions.
 * - Proofs not being provided, largely for testing use or replaying already
 *   validated transactions.
 */
export type Proofish = Proof | PreProof | NoProof;

/**
 * A Fiat-Shamir proof of exponent binding (or ephemerally signing) an
 * {@link Intent}.
 */
export class Binding {
  constructor(data: String);
  serialize(): Uint8Array;
  static deserialize(raw: Uint8Array): Binding;
  toString(compact?: boolean): string;
  instance: 'binding';
  private type_: 'binding';
}

/**
 * Information that will be used to bind an {@link Intent} in the future, but
 * does not yet prevent modification of it.
 */
export class PreBinding {
  constructor(data: String);
  serialize(): Uint8Array;
  static deserialize(raw: Uint8Array): PreBinding;
  toString(compact?: boolean): string;
  instance: 'pre-binding';
  private type_: 'pre-binding';
}

export class NoBinding {
  constructor(data: String);
  serialize(): Uint8Array;
  static deserialize(raw: Uint8Array): NoBinding;
  toString(compact?: boolean): string;
  instance: 'no-binding';
  private type_: 'no-binding';
}

/**
 * Whether an intent has binding cryptography applied or not. An intent's
 * content can no longer be modified after it is {@link Binding}.
 */
export type Bindingish = Binding | PreBinding | NoBinding;

export class SignatureEnabled {
  constructor(data: Signature);
  serialize(): Uint8Array;
  static deserialize(raw: Uint8Array): SignatureEnabled;
  toString(compact?: boolean): string;
  readonly instance: 'signature';
  private type_: 'signature';
}

export class SignatureErased {
  constructor();
  toString(compact?: boolean): string;
  readonly instance: 'signature-erased';
  private type_: 'signature-erased';
}

export type Signaturish = SignatureEnabled | SignatureErased;

/**
 * A type representing a transaction that has not been proven yet
 */
export type UnprovenInput = ZswapInput<PreProof>;

/**
 * A type representing a transaction output that has not been proven yet.
 */
export type UnprovenOutput = ZswapOutput<PreProof>;

/**
 * A type representing a transaction transient that has not been proven yet.
 */
export type UnprovenTransient = ZswapTransient<PreProof>;

/**
 * A type representing an offer that has not been proven yet.
 */
export type UnprovenOffer = ZswapOffer<PreProof>;

/**
 * A type representing an intent that has not been proven yet.
 */
export type UnprovenIntent = Intent<SignatureEnabled, PreProof, PreBinding>;

/**
 * An interactions with a contract
 */
export type ContractAction<P extends Proofish> = ContractCall<P> | ContractDeploy | MaintenanceUpdate;

/**
 * Strictness criteria for evaluating transaction well-formedness, used for
 * disabling parts of transaction validation for testing.
 */
export class WellFormedStrictness {
  constructor();

  /**
   * Whether to require the transaction to have a non-negative balance
   */
  enforceBalancing: boolean;
  /**
   * Whether to validate Midnight-native (non-contract) proofs in the transaction
   */
  verifyNativeProofs: boolean;
  /**
   * Whether to validate contract proofs in the transaction
   */
  verifyContractProofs: boolean;
  /**
   * Whether to enforce the transaction byte limit
   */
  enforceLimits: boolean;
  /**
   * Whether to enforce the signature verification
   */
  verifySignatures: boolean;
}

/**
 * Contains the raw file contents required for proving
 */
export type ProvingKeyMaterial = {
  proverKey: Uint8Array,
  verifierKey: Uint8Array,
  ir: Uint8Array,
};

/**
 * A modelled cost of a transaction or block.
 */
export type SyntheticCost = {
  /**
   * The amount of (modelled) time spent reading from disk, measured in picoseconds.
   */
  readTime: bigint,
  /**
   * The amount of (modelled) time spent in single-threaded compute, measured in picoseconds.
   */
  computeTime: bigint,
  /**
   * The number of bytes of blockspace used
   */
  blockUsage: bigint,
  /**
   * The net number of (modelled) bytes written, i.e. max(0, absolute written bytes less deleted bytes).
   */
  bytesWritten: bigint,
  /**
   * The number of (modelled) bytes written temporarily or overwritten.
   */
  bytesChurned: bigint,
};

/**
 * An event emitted by the ledger
 */
export class Event {
  private constructor();
  serialize(): Uint8Array;
  static deserialize(raw: Uint8Array): Event;
  toString(compact?: boolean): string;
}

// TODO: Doc comments
export type DustSecretKey = bigint;
export type DustPublicKey = bigint;
export type DustInitialNonce = string;
export type DustNonce = bigint;
export type DustCommitment = bigint;
export type DustNullifier = bigint;

export function sampleDustSecretKey(): DustSecretKey;

export function dustPublicKeyFromSecret(sk: DustSecretKey): DustPublicKey;

export function updatedValue(ctime: Date, initialValue: bigint, genInfo: DustGenerationInfo, now: Date, params: DustParameters): bigint;

export type DustOutput = {
  initialValue: bigint,
  owner: DustPublicKey,
  nonce: DustNonce,
  seq: number,
  ctime: Date,
  backingNight: DustInitialNonce,
};

export type QualifiedDustOutput = {
  initialValue: bigint,
  owner: DustPublicKey,
  nonce: DustNonce,
  seq: number,
  ctime: Date,
  backingNight: DustInitialNonce,
  mtIndex: bigint,
};

export type DustGenerationInfo = {
  value: bigint,
  owner: DustPublicKey,
  nonce: DustInitialNonce,
  dtime: Date | undefined,
};

export type DustGenerationUniquenessInfo = {
  value: bigint,
  owner: DustPublicKey,
  nonce: DustInitialNonce,
};

export class DustSpend<P extends Proofish> {
  private constructor();
  serialize(): Uint8Array;
  static deserialize<P extends Proofish>(markerP: P['instance'], raw: Uint8Array): DustSpend<P>;
  toString(compact?: boolean): string;
  readonly vFee: bigint;
  readonly oldNullifier: DustNullifier;
  readonly newCommitment: DustCommitment;
  readonly proof: P;
}

export class DustRegistration<S extends Signaturish> {
  constructor(markerS: S['instance'], nightKey: SignatureVerifyingKey, dustAddress: DustPublicKey | undefined, allowFeePayment: bigint, signature?: S);
  serialize(): Uint8Array;
  static deserialize<S extends Signaturish>(markerS: S['instance'], raw: Uint8Array): DustRegistration<S>;
  toString(compact?: boolean): string;
  nightKey: SignatureVerifyingKey;
  dustAddress: DustPublicKey | undefined;
  allowFeePayment: bigint;
  signature: S;
}

export class DustActions<S extends Signaturish, P extends Proofish> {
  constructor(markerS: S['instance'], markerP: P['instance'], ctime: Date, spends?: DustSpend<P>[], registrations?: DustRegistration<S>[]);
  serialize(): Uint8Array;
  static deserialize<S extends Signaturish, P extends Proofish>(markerS: S['instance'], markerP: P['instance'], raw: Uint8Array): DustActions<S, P>;
  toString(compact?: boolean): string;
  spends: DustSpend<P>[];
  registrations: DustRegistration<S>[];
  ctime: Date;
}

export class DustParameters {
  constructor(nightDustRatio: bigint, generationDecayRate: bigint, dustGracePeriodSeconds: bigint);
  serialize(): Uint8Array;
  static deserialize(raw: Uint8Array): DustParameters;
  toString(compact?: boolean): string;
  nightDustRatio: bigint;
  generationDecayRate: bigint;
  dustGracePeriodSeconds: bigint;
  readonly timeToCapSeconds: bigint;
}

export class DustUtxoState {
  constructor();
  serialize(): Uint8Array;
  static deserialize(raw: Uint8Array): DustUtxoState;
  toString(compact?: boolean): string;
}

export class DustGenerationState {
  constructor();
  serialize(): Uint8Array;
  static deserialize(raw: Uint8Array): DustGenerationState;
  toString(compact?: boolean): string;
}

export class DustState {
  constructor();
  serialize(): Uint8Array;
  static deserialize(raw: Uint8Array): DustState;
  toString(compact?: boolean): string;
  readonly utxo: DustUtxoState;
  readonly generation: DustGenerationState;
}

export class DustLocalState {
  constructor(params: DustParameters);
  walletBalance(time: Date): bigint;
  generationInfo(qdo: QualifiedDustOutput): DustGenerationInfo | undefined;
  spend(sk: DustSecretKey, utxo: QualifiedDustOutput, vFee: bigint, ctime: Date): [DustLocalState, DustSpend<PreProof>];
  processTtls(time: Date): DustLocalState;
  replayEvents(sk: DustSecretKey, events: Event[]): DustLocalState;
  serialize(): Uint8Array;
  static deserialize(raw: Uint8Array): DustLocalState;
  toString(compact?: boolean): string;
  readonly utxos: QualifiedDustOutput[];
}

/**
 * Creates a payload for proving a specific transaction through the proof server
 * @deprecated Use `Transaction.prove` instead.
 */
export function createProvingTransactionPayload(
  transaction: UnprovenTransaction,
  proving_data: Map<string, ProvingKeyMaterial>,
): Uint8Array;

/**
 * Creates a payload for proving a specific proof through the proof server
 */
export function createProvingPayload(
  serializedPreimage: Uint8Array,
  overwriteBindingInput: bigint | undefined,
  keyMaterial?: ProvingKeyMaterial,
): Uint8Array;

/**
 * Creates a payload for checking a specific proof through the proof server
 */
export function createCheckPayload(
  serializedPreimage: Uint8Array,
  ir?: Uint8Array,
): Uint8Array;

/**
 * Parses the result of a proof-server check call
 */
export function parseCheckResult(result: Uint8Array): (bigint | undefined)[]

/**
 * The state of the Midnight ledger
 */
export class LedgerState {
  /**
   * Intializes from a Zswap state, with an empty contract set
   */
  constructor(network_id: string, zswap: ZswapChainState);

  /**
   * A fully blank state
   */
  static blank(network_id: string): LedgerState;

  /**
   * Applies a {@link Transaction}
   */
  apply(
    transaction: VerifiedTransaction,
    context: TransactionContext
  ): [LedgerState, TransactionResult];

  /**
   * Applies a system transaction to this ledger state.
   */
  applySystemTx(transaction: SystemTransaction, tblock: Date): [LedgerState, Event[]];

  /**
   * Indexes into the contract state map with a given contract address
   */
  index(address: ContractAddress): ContractState | undefined;

  /**
   * Sets the state of a given contract address from a {@link ChargedState}
   */
  updateIndex(address: ContractAddress, state: ChargedState, balance: Map<TokenType, bigint>): LedgerState;

  serialize(): Uint8Array;

  static deserialize(raw: Uint8Array): LedgerState;

  toString(compact?: boolean): string;

  /**
   * Carries out a post-block update, which does amortized bookkeeping that
   * only needs to be done once per state change.
   *
   * Typically, `postBlockUpdate` should be run after any (sequence of)
   * (system)-transaction application(s).
   */
  postBlockUpdate(tblock: Date, blockFullness?: SyntheticCost): LedgerState;

  /**
   * Retrieves the balance of the treasury for a specific token type.
   */
  treasuryBalance(token_type: TokenType): bigint;

  /**
  * How much in block rewards a recipient is owed and can claim.
  */
  unclaimedBlockRewards(recipient: UserAddress): bigint;

  /**
   * How much in bridged night a recipient is owed and can claim.
   */
  bridgeReceiving(recipient: UserAddress): bigint;

  /**
   * Allows distributing the specified amount of Night to the recipient's address.
   * Use is for testing purposes only.
   */
  testingDistributeNight(recipient: UserAddress, amount: bigint, tblock: Date): LedgerState;

  /**
   * The remaining size of the locked Night pool.
   */
  readonly lockedPool: bigint;

  /**
   * The size of the reserve Night pool
   */
  readonly reservePool: bigint;

  /**
   * How much in bridged night a recipient is owed and can claim.
   */
  bridgeReceiving(recipient: UserAddress): bigint;

  /**
   * The remaining unrewarded supply of native tokens.
   */
  readonly blockRewardPool: bigint;
  /**
   * The Zswap part of the ledger state
   */
  readonly zswap: ZswapChainState;
  /**
   * The unshielded utxos present
   */
  readonly utxo: UtxoState;
  /**
   * The dust subsystem state
   */
  readonly dust: DustState;
}

/**
 * An unspent transaction output
 */
export type Utxo = {
  /**
   * The amount of tokens this UTXO represents
   */
  value: bigint,
  /**
   * The address owning these tokens.
   */
  owner: UserAddress,
  /**
   * The token type of this UTXO
   */
  type: RawTokenType,
  /**
   * The hash of the intent outputting this UTXO
   */
  intentHash: IntentHash,
  /**
   * The output number of this UTXO in its parent {@link Intent}.
   */
  outputNo: number,
};

/**
 * An output appearing in an {@link Intent}.
 */
export type UtxoOutput = {
  /**
   * The amount of tokens this UTXO represents
   */
  value: bigint,
  /**
   * The address owning these tokens.
   */
  owner: UserAddress,
  /**
   * The token type of this UTXO
   */
  type: RawTokenType,
};

/**
 * Converts a bare signature public key to its corresponding address.
 */
export function addressFromKey(key: SignatureVerifyingKey): UserAddress;

/**
 * An input appearing in an {@link Intent}, or a user's local book-keeping.
 */
export type UtxoSpend = {
  /**
   * The amount of tokens this UTXO represents
   */
  value: bigint,
  /**
   * The signing key owning these tokens.
   */
  owner: SignatureVerifyingKey,
  /**
   * The token type of this UTXO
   */
  type: RawTokenType,
  /**
   * The hash of the intent outputting this UTXO
   */
  intentHash: IntentHash,
  /**
   * The output number of this UTXO in its parent {@link Intent}.
   */
  outputNo: number,
};

/**
 * Metadata about a specific UTXO
 */
export class UtxoMeta {
  constructor(ctime: Date);
  /**
   * The creation time of the UTXO, that is, when it was inserted into the state.
   */
  ctime: Date;
}
/**
 * The sub-state for unshielded UTXOs
 */
export class UtxoState {
  static new(utxos: Map<Utxo, UtxoMeta>): UtxoState;
  /**
   * Lookup the metadata for a specific UTXO.
   */
  lookupMeta(utxo: Utxo): UtxoMeta | undefined;

  /**
   * The set of valid UTXOs
   */
  readonly utxos: Set<Utxo>;

  /**
   * Filters out the UTXOs owned by a specific user address
   */
  filter(addr: UserAddress): Set<Utxo>;

  /**
   * Given a prior UTXO state, produce the set differences `this \ prior`, and
   * `prior \ this`, optionally filtered by a further condition.
   *
   * Note that this should be more efficient than iterating or manifesting the
   * {@link utxos} value, as the low-level implementation can avoid traversing
   * shared sub-structures.
   */
  delta(prior: UtxoState, filterBy?: (utxo: Utxo) => boolean): [Set<Utxo>, Set<Utxo>];
}

/**
 * A single contract call segment
 */
export class ContractCall<P extends Proofish> {
  private constructor();

  toString(compact?: boolean): string;

  /**
   * The address being called
   */
  readonly address: ContractAddress;
  /**
   * The communication commitment of this call
   */
  readonly communicationCommitment: CommunicationCommitment;
  /**
   * The entry point being called
   */
  readonly entryPoint: Uint8Array | string;
  /**
   * The fallible execution stage transcript
   */
  readonly fallibleTranscript: Transcript<AlignedValue> | undefined;
  /**
   * The guaranteed execution stage transcript
   */
  readonly guaranteedTranscript: Transcript<AlignedValue> | undefined;
  /**
   * The proof attached to this call
   */
  readonly proof: P;
}

/**
 * A {@link ContractCall} still being assembled
 */
export class ContractCallPrototype {
  /**
   * @param address - The address being called
   * @param entry_point - The entry point being called
   * @param op - The operation expected at this entry point
   * @param guaranteed_public_transcript - The guaranteed transcript computed
   * for this call
   * @param fallible_public_transcript - The fallible transcript computed for
   * this call
   * @param private_transcript_outputs - The private transcript recorded for
   * this call
   * @param input - The input(s) provided to this call
   * @param output - The output(s) computed from this call
   * @param communication_commitment_rand - The communication randomness used
   * for this call
   * @param key_location - An identifier for how the key for this call may be
   * looked up
   */
  constructor(
    address: ContractAddress,
    entry_point: Uint8Array | string,
    op: ContractOperation,
    guaranteed_public_transcript: Transcript<AlignedValue> | undefined,
    fallible_public_transcript: Transcript<AlignedValue> | undefined,
    private_transcript_outputs: AlignedValue[],
    input: AlignedValue,
    output: AlignedValue,
    communication_commitment_rand: CommunicationCommitmentRand,
    key_location: string
  );

  toString(compact?: boolean): string;

  intoCall(parentBinding: PreBinding): ContractCall<PreProof>;
}

/**
 * An intent is a potentially unbalanced partial transaction, that may be
 * combined with other intents to form a whole.
 */
export class Intent<S extends Signaturish, P extends Proofish, B extends Bindingish> {
  private constructor();

  static new(ttl: Date): UnprovenIntent;

  serialize(): Uint8Array;

  static deserialize<S extends Signaturish, P extends Proofish, B extends Bindingish>(
    markerS: S['instance'],
    markerP: P['instance'],
    markerB: B['instance'],
    raw: Uint8Array,

  ): Intent<S, P, B>;

  toString(compact?: boolean): string;

  /**
   * Returns the hash of this intent, for it's given segment ID.
   */
  intentHash(segmentId: number): IntentHash;

  /**
   * Adds a contract call to this intent.
   */
  addCall(call: ContractCallPrototype): Intent<S, PreProof, PreBinding>;

  /**
   * Adds a contract deploy to this intent.
   */
  addDeploy(deploy: ContractDeploy): Intent<S, PreProof, PreBinding>;

  /**
   * Adds a maintenance update to this intent.
   */
  addMaintenanceUpdate(update: MaintenanceUpdate): Intent<S, PreProof, PreBinding>;

  /**
   * Enforces binding for this intent. This is irreversible.
   * @throws If `segmentId` is not a valid segment ID.
   */
  bind(segmentId: number): Intent<S, P, Binding>;

  /**
   * Removes proofs from this intent.
   */
  eraseProofs(): Intent<S, NoProof, NoBinding>;

  /**
   * Removes signatures from this intent.
   */
  eraseSignatures(): Intent<SignatureErased, P, B>;

  /**
   * The raw data that is signed for unshielded inputs in this intent.
   */
  signatureData(segmentId: number): Uint8Array;

  /**
   * The UTXO inputs and outputs in the guaranteed section of this intent.
   * @throws Writing throws if `B` is {@link Binding}, unless the only change
   * is in the signature set.
   */
  guaranteedUnshieldedOffer: UnshieldedOffer<S> | undefined;
  /**
   * The UTXO inputs and outputs in the fallible section of this intent.
   * @throws Writing throws if `B` is {@link Binding}, unless the only change
   * is in the signature set.
   */
  fallibleUnshieldedOffer: UnshieldedOffer<S> | undefined;
  /**
   * The action sequence of this intent.
   * @throws Writing throws if `B` is {@link Binding}.
   */
  actions: ContractAction<P>[];
  /**
   * The DUST interactions made by this intent
   * @throws Writing throws if `B` is {@link Binding}.
   */
  dustActions: DustActions<S, P> | undefined;
  /**
   * The time this intent expires.
   * @throws Writing throws if `B` is {@link Binding}.
   */
  ttl: Date;
  readonly binding: B;
}

/**
 * An unshielded offer consists of inputs, outputs, and signatures that
 * authorize the inputs. The data the signatures sign is provided by {@link
 * Intent.signatureData}.
 */
export class UnshieldedOffer<S extends Signaturish> {
  private constructor();

  static new(inputs: UtxoSpend[], outputs: UtxoOutput[], signatures: Signature[]): UnshieldedOffer<SignatureEnabled>;

  addSignatures(signatures: Signature[]): UnshieldedOffer<S>;

  eraseSignatures(): UnshieldedOffer<SignatureErased>;

  toString(compact?: boolean): string;

  readonly inputs: UtxoSpend[];
  readonly outputs: UtxoOutput[];
  readonly signatures: Signature[];
}

/**
 * The context against which a transaction is run.
 */
export class TransactionContext {
  /**
   * @param ref_state - A past ledger state that is used as a reference point
   * for 'static' data.
   * @param block_context - Information about the block this transaction is, or
   * will be, contained in.
   * @param whitelist - A list of contracts that are being tracked, or
   * `undefined` to track all contracts.
   */
  constructor(ref_state: LedgerState, block_context: BlockContext, whitelist?: Set<ContractAddress>);

  toString(compact?: boolean): string;
}

/**
 * The result status of applying a transaction.
 * Includes an error message if the transaction failed, or partially failed.
 */
export class TransactionResult {
  private constructor();

  readonly type: 'success' | 'partialSuccess' | 'failure';
  readonly successfulSegments?: Map<number, boolean>;
  readonly error?: string;
  readonly events: Event[];

  toString(compact?: boolean): string;
}

/**
 * The result status of applying a transaction, without error message
 */
export type ErasedTransactionResult = {
  type: 'success' | 'partialSuccess' | 'failure',
  successfulSegments?: Map<number, boolean>,
};

/**
 * A single update instruction in a {@link MaintenanceUpdate}.
 */
export type SingleUpdate = ReplaceAuthority | VerifierKeyRemove | VerifierKeyInsert;

/**
 * The version associated with a {@link ContractOperation}
 */
export class ContractOperationVersion {
  constructor(version: 'v2');

  readonly version: 'v2';

  toString(compact?: boolean): string;
}

/**
 * A versioned verifier key to be associated with a {@link ContractOperation}.
 */
export class ContractOperationVersionedVerifierKey {
  constructor(version: 'v2', rawVk: Uint8Array);

  readonly version: 'v2';
  readonly rawVk: Uint8Array;

  toString(compact?: boolean): string;
}

/**
 * An update instruction to replace the current contract maintenance authority
 * with a new one.
 */
export class ReplaceAuthority {
  constructor(authority: ContractMaintenanceAuthority);

  readonly authority: ContractMaintenanceAuthority;

  toString(compact?: boolean): string;
}

/**
 * An update instruction to remove a verifier key of a specific operation and
 * version.
 */
export class VerifierKeyRemove {
  constructor(operation: string | Uint8Array, version: ContractOperationVersion);

  readonly operation: string | Uint8Array;
  readonly version: ContractOperationVersion;

  toString(compact?: boolean): string;
}

/**
 * An update instruction to insert a verifier key at a specific operation and
 * version.
 */
export class VerifierKeyInsert {
  constructor(operation: string | Uint8Array, vk: ContractOperationVersionedVerifierKey);

  readonly operation: string | Uint8Array;
  readonly vk: ContractOperationVersionedVerifierKey;

  toString(compact?: boolean): string;
}

/**
 * A contract maintenance update, updating associated operations, or
 * changing the maintenance authority.
 */
export class MaintenanceUpdate {
  constructor(address: ContractAddress, updates: SingleUpdate[], counter: bigint);

  /**
   * Adds a new signature to this update
   */
  addSignature(idx: bigint, signature: Signature): MaintenanceUpdate;

  toString(compact?: boolean): string;

  /**
   * The raw data any valid signature must be over to approve this update.
   */
  readonly dataToSign: Uint8Array;
  /**
   * The address this deployment will attempt to create
   */
  readonly address: ContractAddress;
  /**
   * The updates to carry out
   */
  readonly updates: SingleUpdate[];
  /**
   * The counter this update is valid against
   */
  readonly counter: bigint;
  /**
   * The signatures on this update
   */
  readonly signatures: [bigint, Signature][];
}

/**
 * A contract deployment segment, instructing the creation of a new contract
 * address, if not already present
 */
export class ContractDeploy {
  /**
   * Creates a deployment for an arbitrary contract state
   *
   * The deployment and its address are randomised.
   */
  constructor(initial_state: ContractState);

  toString(compact?: boolean): string;

  /**
   * The address this deployment will attempt to create
   */
  readonly address: ContractAddress;
  readonly initialState: ContractState;
}

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

/**
 * A transaction that has been validated with `wellFormed`.
 **/
export class VerifiedTransaction {
  private constructor();

  /**
   * The actual underlying transaction
   **/
  readonly transaction: Transaction<SignatureErased, NoProof, NoBinding>;
}

/**
 * A Midnight transaction, consisting a section of {@link
 * ContractAction}s, and a guaranteed and fallible {@link ZswapOffer}.
 *
 * The guaranteed section are run first, and fee payment is taken during this
 * part. If it succeeds, the fallible section is also run, and atomically
 * rolled back if it fails.
 */
export class Transaction<S extends Signaturish, P extends Proofish, B extends Bindingish> {
  private constructor();

  /**
   * Creates a transaction from its parts.
   */
  static fromParts(network_id: string, guaranteed?: UnprovenOffer, fallible?: UnprovenOffer, intent?: UnprovenIntent): UnprovenTransaction;

  /**
   * Creates a transaction from its parts, randomizing the segment ID to better
   * allow merging.
   */
  static fromPartsRandomized(network_id: string, guaranteed?: UnprovenOffer, fallible?: UnprovenOffer, intent?: UnprovenIntent): UnprovenTransaction;

  /**
   * Creates a rewards claim transaction, the funds claimed must have been
   * legitimately rewarded previously.
   */
  static fromRewards<S extends Signaturish>(rewards: ClaimRewardsTransaction<S>): Transaction<S, PreProof, Binding>;

  /**
   * Mocks proving, producing a 'proven' transaction that, while it will
   * *not* verify, is accurate for fee computation purposes.
   *
   * Due to the variability in proof sizes, this *only* works for transactions
   * that do not contain unproven contract calls.
   *
   * @throws If called on bound, proven, or proof-erased transactions, or if the
   * transaction contains unproven contract calls.
   */
  mockProve(): Transaction<S, Proof, B>;
  /**
   * Proves the transaction, with access to a low-level proving provider.
   * This may *only* be called for `P = PreProof`.
   *
   * @throws If called on bound, proven, or proof-erased transactions.
   */
  prove(provider: ProvingProvider, cost_model: CostModel): Promise<Transaction<S, Proof, B>>;

  /**
   * Erases the proofs contained in this transaction
   */
  eraseProofs(): Transaction<S, NoProof, NoBinding>;

  /**
   * Removes signatures from this transaction.
   */
  eraseSignatures(): Transaction<SignatureErased, P, B>;

  /**
   * Enforces binding for this transaction. This is irreversible.
   */
  bind(): Transaction<S, P, Binding>;

  /**
   * Tests well-formedness criteria, optionally including transaction balancing
   *
   * @throws If the transaction is not well-formed for any reason
   */
  wellFormed(ref_state: LedgerState, strictness: WellFormedStrictness, tblock: Date): VerifiedTransaction;

  /**
   * Returns the hash associated with this transaction. Due to the ability to
   * merge transactions, this should not be used to watch for a specific
   * transaction.
   */
  transactionHash(): TransactionHash;

  /**
   * Returns the set of identifiers contained within this transaction. Any of
   * these *may* be used to watch for a specific transaction.
   */
  identifiers(): TransactionId[];

  /**
   * Merges this transaction with another
   *
   * @throws If both transactions have contract interactions, or they spend the
   * same coins
   */
  merge(other: Transaction<S, P, B>): Transaction<S, P, B>;

  serialize(): Uint8Array;

  static deserialize<S extends Signaturish, P extends Proofish, B extends Bindingish>(
    markerS: S['instance'],
    markerP: P['instance'],
    markerB: B['instance'],
    raw: Uint8Array,

  ): Transaction<S, P, B>;

  /**
   * For given fees, and a given section (guaranteed/fallible), what the
   * surplus or deficit of this transaction in any token type is.
   *
   * @throws If `segment` is not a valid segment ID
   */
  imbalances(segment: number, fees?: bigint): Map<TokenType, bigint>;

  /**
   * The underlying resource cost of this transaction.
   */
  cost(params: LedgerParameters): SyntheticCost;

  /**
   * The cost of this transaction, in SPECKs.
   *
   * Note that this is *only* accurate when called with proven transactions.
   */
  fees(params: LedgerParameters): bigint;

  toString(compact?: boolean): string;

  /**
   * The rewards this transaction represents, if applicable
   */
  readonly rewards: ClaimRewardsTransaction<S> | undefined;
  /**
   * The intents contained in this transaction
   * @throws On writing if `B` is {@link Binding} or this is not a standard
   * transaction
   */
  intents: Map<number, Intent<S, P, B>> | undefined;
  /**
   * The fallible Zswap offer
   * @throws On writing if `B` is {@link Binding} or this is not a standard
   * transaction
   */
  fallibleOffer: Map<number, ZswapOffer<P>> | undefined;
  /**
   * The guaranteed Zswap offer
   * @throws On writing if `B` is {@link Binding} or this is not a standard
   * transaction
   */
  guaranteedOffer: ZswapOffer<P> | undefined;
  /**
   * The binding randomness associated with this transaction
   */
  readonly bindingRandomness: bigint;
}

/**
 * A transcript prior to partitioning, consisting of the context to run it in, the program that
 * will make up the transcript, and optionally a communication commitment to bind calls together.
 */
export class PreTranscript {
  constructor(context: QueryContext, program: Op<AlignedValue>[], comm_comm?: CommunicationCommitment);

  toString(compact?: boolean): string;
}

/**
 * Computes the communication commitment corresponding to an input/output pair and randomness.
 */
export function communicationCommitment(input: AlignedValue, output: AlignedValue, rand: CommunicationCommitmentRand): CommunicationCommitment;

/**
 * Finalizes a set of programs against their initial contexts,
 * resulting in guaranteed and fallible {@link Transcript}s, optimally
 * allocated, and heuristically covered for gas fees.
 */
export function partitionTranscripts(calls: PreTranscript[], params: LedgerParameters): [Transcript<AlignedValue> | undefined, Transcript<AlignedValue> | undefined][];

/**
 * The hash of a transaction, as a hex-encoded 256-bit bytestring
 */
export type TransactionHash = string;
/**
 * The hash of an intent, as a hex-encoded 256-bit bytestring
 */
export type IntentHash = string;
/**
 * A transaction identifier, used to index merged transactions
 */
export type TransactionId = string;
/**
 * An encryption public key, used to inform users of new coins sent to them
 */
export type EncPublicKey = string;
/**
 * A coin public key, used to determine if a given coin is owned by a specific user
 */
export type CoinPublicKey = string;

/**
 * Samples a dummy user coin public key, for use in testing
 */
export function sampleCoinPublicKey(): CoinPublicKey;

/**
 * Samples a dummy user encryption public key, for use in testing
 */
export function sampleEncryptionPublicKey(): EncPublicKey;

/**
 * Samples a dummy user intent hash, for use in testing
 */
export function sampleIntentHash(): IntentHash;

/**
 * Creates a new {@link ShieldedCoinInfo}, sampling a uniform nonce
 */
export function createShieldedCoinInfo(type_: RawTokenType, value: bigint): ShieldedCoinInfo;

/**
 * The base/system token type
 */
export function nativeToken(): UnshieldedTokenType;

/**
 * The system token type for fees
 */
export function feeToken(): DustTokenType;

/**
 * Default shielded token type for testing
 */
export function shieldedToken(): ShieldedTokenType;

/**
 * Default unshielded token type for testing
 */
export function unshieldedToken(): UnshieldedTokenType;

/**
 * Calculate commitment of a coin owned by a user
 */
export function coinCommitment(coin: ShieldedCoinInfo, coinPublicKey: CoinPublicKey): CoinCommitment;

/**
 * Calculate nullifier of a coin owned by a user
 */
export function coinNullifier(coin: ShieldedCoinInfo, coinSecretKey: CoinSecretKey): Nullifier;

/**
 * Parameters used by the Midnight ledger, including transaction fees and
 * bounds
 */
export class LedgerParameters {
  private constructor();

  /**
   * The initial parameters of Midnight
   */
  static initialParameters(): LedgerParameters;

  /**
   * The cost model used for transaction fees contained in these parameters
   */
  readonly transactionCostModel: TransactionCostModel;
  /**
   * The parameters associated with DUST.
   */
  readonly dust: DustParameters;

  serialize(): Uint8Array;

  static deserialize(raw: Uint8Array): LedgerParameters;

  toString(compact?: boolean): string;
}

export class TransactionCostModel {
  private constructor();

  /**
   * The initial cost model of Midnight
   */
  static initialTransactionCostModel(): TransactionCostModel;

  /**
   * The increase in fees to expect from adding a new input to a transaction
   */
  readonly inputFeeOverhead: bigint;
  /**
   * The increase in fees to expect from adding a new output to a transaction
   */
  readonly outputFeeOverhead: bigint;

  serialize(): Uint8Array;

  static deserialize(raw: Uint8Array): TransactionCostModel;

  toString(compact?: boolean): string;
}


/**
 * A compact delta on the coin commitments Merkle tree, used to keep local
 * spending trees in sync with the global state without requiring receiving all
 * transactions.
 */
export class MerkleTreeCollapsedUpdate {
  /**
   * Create a new compact update from a non-compact state, and inclusive
   * `start` and `end` indices
   *
   * @throws If the indices are out-of-bounds for the state, or `end < start`
   */
  constructor(state: ZswapChainState, start: bigint, end: bigint);

  serialize(): Uint8Array;

  static deserialize(raw: Uint8Array): MerkleTreeCollapsedUpdate;

  toString(compact?: boolean): string;
}

/**
 * Holds the encryption secret key of a user, which may be used to determine if
 * a given offer contains outputs addressed to this user
 */
export class EncryptionSecretKey {
  private constructor();

  /**
   * Clears the encryption secret key, so that it is no longer usable nor held in memory
   */
  clear(): void;

  test<P extends Proofish>(offer: ZswapOffer<P>): boolean;

  yesIKnowTheSecurityImplicationsOfThis_serialize(): Uint8Array;

  static deserialize(raw: Uint8Array): EncryptionSecretKey
}

/**
 * Holds the coin secret key of a user
 */
export class CoinSecretKey {
  private constructor();

  /**
   * Clears the coin secret key, so that it is no longer usable nor held in memory
   */
  clear(): void;

  yesIKnowTheSecurityImplicationsOfThis_serialize(): Uint8Array;

  static deserialize(raw: Uint8Array): CoinSecretKey
}

export class ZswapSecretKeys {
  private constructor();

  /**
   * Derives secret keys from a 32-byte seed
   */
  static fromSeed(seed: Uint8Array): ZswapSecretKeys;

  /**
   * Derives secret keys from a 32-byte seed using deprecated implementation.
   * Use only for compatibility purposes
   */
  static fromSeedRng(seed: Uint8Array): ZswapSecretKeys;


  /**
   * Clears the secret keys, so that they are no longer usable nor held in memory
   * Note: it does not clear copies of the keys - which is particularly relevant for proof preimages
   * Note: this will cause all other operations to fail
   */
  clear(): void;

  readonly coinPublicKey: CoinPublicKey;
  readonly coinSecretKey: CoinSecretKey;
  readonly encryptionPublicKey: EncPublicKey;
  readonly encryptionSecretKey: EncryptionSecretKey;
}

/**
 * The on-chain state of Zswap, consisting of a Merkle tree of coin
 * commitments, a set of nullifiers, an index into the Merkle tree, and a set
 * of valid past Merkle tree roots
 */
export class ZswapChainState {
  constructor();

  serialize(): Uint8Array;

  /**
   * The first free index in the coin commitment tree
   */
  readonly firstFree: bigint;

  static deserialize(raw: Uint8Array): ZswapChainState;

  /**
   * Given a whole ledger serialized state, deserialize only the Zswap portion
   */
  static deserializeFromLedgerState(raw: Uint8Array): ZswapChainState;

  /**
   * Carries out a post-block update, which does amortized bookkeeping that
   * only needs to be done once per state change.
   *
   * Typically, `postBlockUpdate` should be run after any (sequence of)
   * (system)-transaction application(s).
   */
  postBlockUpdate(tblock: Date): ZswapChainState;

  /**
   * Try to apply an {@link ZswapOffer} to the state, returning the updated state
   * and a map on newly inserted coin commitments to their inserted indices.
   *
   * @param whitelist - A set of contract addresses that are of interest. If
   * set, *only* these addresses are tracked, and all other information is
   * discarded.
   */
  tryApply<P extends Proofish>(offer: ZswapOffer<P>, whitelist?: Set<ContractAddress>): [ZswapChainState, Map<CoinCommitment, bigint>];

  toString(compact?: boolean): string;

  /**
   * Filters the state to only include coins that are relevant to a given
   * contract address.
   *
   * @param contractAddress
   */
  filter(contractAddress: ContractAddress): ZswapChainState;
}

/**
 * The local state of a user/wallet, consisting of a set
 * of unspent coins
 *
 * It also keeps track of coins that are in-flight, either expecting to spend
 * or expecting to receive, and a local copy of the global coin commitment
 * Merkle tree to generate proofs against.
 *
 * It does not store keys internally, but accepts them as arguments to various operations.
 */
export class ZswapLocalState {
  /**
   * Creates a new, empty state
   */
  constructor();

  /**
   * Applies a collapsed Merkle tree update to the current local state, fast
   * forwarding through the indices included in it, if it is a correct update.
   *
   * The general flow for usage if Alice is in state A, and wants to ask Bob how to reach the new state B, is:
   *  - Find where she left off – what's her firstFree?
   *  - Find out where she's going – ask for Bob's firstFree.
   *  - Find what contents she does care about – ask Bob for the filtered
   *    entries she want to include proper in her tree.
   *  - In order, of Merkle tree indices:
   *    - Insert (with `apply` offers Alice cares about).
   *    - Skip (with this method) sections Alice does not care about, obtaining
   *      the collapsed update covering the gap from Bob.
   * Note that `firstFree` is not included in the tree itself, and both ends of
   * updates *are* included.
   */
  applyCollapsedUpdate(update: MerkleTreeCollapsedUpdate): ZswapLocalState;

  /**
   * Replays observed events against the current local state. These *must* be replayed
   * in the same order as emitted by the chain being followed.
   */
  replayEvents(secretKeys: ZswapSecretKeys, events: Event[]): ZswapLocalState;
  /**
   * Locally applies an offer to the current state, returning the updated state
   */
  apply<P extends Proofish>(secretKeys: ZswapSecretKeys, offer: ZswapOffer<P>): ZswapLocalState;

  /**
   * Clears pending outputs / spends that have passed their TTL without being included in
   * a block.
   *
   * Note that as TTLs are *from a block perspective*, and there is some
   * latency between the block and the wallet, the time passed in here should
   * not be the current time, but incorporate a latency buffer.
   */
  clearPending(time: Date): ZswapLocalState;

  /**
   * Initiates a new spend of a specific coin, outputting the corresponding
   * {@link ZswapInput}, and the updated state marking this coin as
   * in-flight.
   */
  spend(secretKeys: ZswapSecretKeys, coin: QualifiedShieldedCoinInfo, segment: number, ttl?: Date): [ZswapLocalState, UnprovenInput];

  /**
   * Initiates a new spend of a new-yet-received output, outputting the
   * corresponding {@link ZswapTransient}, and the updated state marking
   * this coin as in-flight.
   */
  spendFromOutput(secretKeys: ZswapSecretKeys, coin: QualifiedShieldedCoinInfo, segment: number, output: UnprovenOutput, ttl?: Date): [ZswapLocalState, UnprovenTransient];

  /**
   * Adds a coin to the list of coins that are expected to be received
   *
   * This should be used if an output is creating a coin for this wallet, which
   * does not contain a ciphertext to detect it. In this case, the wallet must
   * know the commitment ahead of time to notice the receipt.
   */
  watchFor(coinPublicKey: CoinPublicKey, coin: ShieldedCoinInfo): ZswapLocalState;

  serialize(): Uint8Array;

  static deserialize(raw: Uint8Array): ZswapLocalState;

  toString(compact?: boolean): string;

  /**
   * The set of *spendable* coins of this wallet
   */
  readonly coins: Set<QualifiedShieldedCoinInfo>;
  /**
   * The first free index in the internal coin commitments Merkle tree.
   * This may be used to identify which merkle tree updates are necessary.
   */
  readonly firstFree: bigint;
  /**
   * The outputs that this wallet is expecting to receive in the future, with
   * an optional TTL attached.
   */
  readonly pendingOutputs: Map<CoinCommitment, [ShieldedCoinInfo, Date | undefined]>;
  /**
   * The spends that this wallet is expecting to be finalized on-chain in the
   * future. Each has an optional TTL attached.
   */
  readonly pendingSpends: Map<Nullifier, [QualifiedShieldedCoinInfo, Date | undefined]>;
}

/**
 * A shielded transaction input
 */
export class ZswapInput<P extends Proofish> {
  private constructor();

  static newContractOwned(coin: QualifiedShieldedCoinInfo, segment: number, contract: ContractAddress, state: ZswapChainState): UnprovenInput;

  serialize(): Uint8Array;

  static deserialize<P extends Proofish>(markerP: P['instance'], raw: Uint8Array): ZswapInput<P>;

  toString(compact?: boolean): string;

  /**
   * The contract address receiving the input, if the sender is a contract
   */
  readonly contractAddress: ContractAddress | undefined;
  /**
   * The nullifier of the input
   */
  readonly nullifier: Nullifier;
  /**
   * The proof of this input
   */
  readonly proof: P;
}

/**
 * A shielded transaction output
 */
export class ZswapOutput<P extends Proofish> {
  private constructor();

  /**
   * Creates a new output, targeted to a user's coin public key.
   *
   * Optionally the output contains a ciphertext encrypted to the user's
   * encryption public key, which may be omitted *only* if the {@link ShieldedCoinInfo}
   * is transferred to the recipient another way
   */
  static new(coin: ShieldedCoinInfo, segment: number, target_cpk: CoinPublicKey, target_epk: EncPublicKey): UnprovenOutput;

  /**
   * Creates a new output, targeted to a smart contract
   *
   * A contract must *also* explicitly receive a coin created in this way for
   * the output to be valid
   */
  static newContractOwned(coin: ShieldedCoinInfo, segment: number, contract: ContractAddress): UnprovenOutput;

  serialize(): Uint8Array;

  static deserialize<P extends Proofish>(markerP: P['instance'], raw: Uint8Array): ZswapOutput<P>;

  toString(compact?: boolean): string;

  /**
   * The commitment of the output
   */
  readonly commitment: CoinCommitment;
  /**
   * The contract address receiving the output, if the recipient is a contract
   */
  readonly contractAddress: ContractAddress | undefined;
  /**
   * The proof of this output
   */
  readonly proof: P;
}

/**
 * A shielded "transient"; an output that is immediately spent within the same
 * transaction
 */
export class ZswapTransient<P extends Proofish> {
  private constructor();

  /**
   * Creates a new contract-owned transient, from a given output and its coin.
   *
   * The {@link QualifiedShieldedCoinInfo} should have an `mt_index` of `0`
   */
  static newFromContractOwnedOutput(coin: QualifiedShieldedCoinInfo, segment: number, output: UnprovenOutput): UnprovenTransient;

  serialize(): Uint8Array;

  static deserialize<P extends Proofish>(markerP: P['instance'], raw: Uint8Array): ZswapTransient<P>;

  toString(compact?: boolean): string;

  /**
   * The commitment of the transient
   */
  readonly commitment: CoinCommitment;
  /**
   * The contract address creating the transient, if applicable
   */
  readonly contractAddress: ContractAddress | undefined;
  /**
   * The nullifier of the transient
   */
  readonly nullifier: Nullifier;
  /**
   * The input proof of this transient
   */
  readonly inputProof: P;
  /**
   * The output proof of this transient
   */
  readonly outputProof: P;
}

export type ClaimKind = "Reward" | "CardanoBridge";

/**
 * A request to allocate rewards, authorized by the reward's recipient
 */
export class ClaimRewardsTransaction<S extends Signaturish> {
  constructor(markerS: S['instance'], network_id: string, value: bigint, owner: SignatureVerifyingKey, nonce: Nonce, signature: S);

  eraseSignatures(): ClaimRewardsTransaction<SignatureErased>;

  serialize(): Uint8Array;

  static deserialize<S extends Signaturish>(markerS: S['instance'], raw: Uint8Array): ClaimRewardsTransaction<S>;

  toString(compact?: boolean): string;

  /**
   * The raw data any valid signature must be over to approve this transaction.
   */
  readonly dataToSign: Uint8Array;

  /**
   * The rewarded coin's value, in atomic units dependent on the currency
   *
   * Bounded to be a non-negative 64-bit integer
   */
  readonly value: bigint;

  /**
   * The signing key owning this coin.
   */
  readonly owner: SignatureVerifyingKey;

  /**
   * The rewarded coin's randomness, preventing it from colliding with other coins.
   */
  readonly nonce: Nonce;

  /**
   * The signature on this request.
   */
  readonly signature: S;

  /**
   * The kind of claim being made, either a `Reward` or a `CardanoBridge` claim.
   */
  readonly kind: ClaimKind
}

/**
 * A full Zswap offer; the zswap part of a transaction
 *
 * Consists of sets of {@link ZswapInput}s, {@link ZswapOutput}s, and {@link ZswapTransient}s,
 * as well as a {@link deltas} vector of the transaction value
 */
export class ZswapOffer<P extends Proofish> {
  private constructor();

  /**
   * Creates a singleton offer, from an {@link ZswapInput} and its value
   * vector
   */
  static fromInput<P extends Proofish>(input: ZswapInput<P>, type_: RawTokenType, value: bigint): ZswapOffer<P>;

  /**
   * Creates a singleton offer, from an {@link ZswapOutput} and its value
   * vector
   */
  static fromOutput<P extends Proofish>(output: ZswapOutput<P>, type_: RawTokenType, value: bigint): ZswapOffer<P>;

  /**
   * Creates a singleton offer, from a {@link ZswapTransient}
   */
  static fromTransient<P extends Proofish>(transient: ZswapTransient<P>): ZswapOffer<P>;

  /**
   * Combine this offer with another
   */
  merge(other: ZswapOffer<P>): ZswapOffer<P>;

  serialize(): Uint8Array;

  static deserialize<P extends Proofish>(markerP: P['instance'], raw: Uint8Array): ZswapOffer<P>;

  toString(compact?: boolean): string;

  /**
   * The value of this offer for each token type; note that this may be
   * negative
   *
   * This is input coin values - output coin values, for value vectors
   */
  readonly deltas: Map<RawTokenType, bigint>;
  /**
   * The inputs this offer is composed of
   */
  readonly inputs: ZswapInput<P>[];
  /**
   * The outputs this offer is composed of
   */
  readonly outputs: ZswapOutput<P>[];
  /**
   * The transients this offer is composed of
   */
  readonly transients: ZswapTransient<P>[];
}

/**
 * A privileged transaction issued by the system.
 */
export class SystemTransaction {
  private constructor();

  serialize(): Uint8Array;

  static deserialize(raw: Uint8Array): SystemTransaction;

  toString(compact?: boolean): string;
}

/**
 * A transaction that has not yet been proven.
 */
export type UnprovenTransaction = Transaction<SignatureEnabled, PreProof, PreBinding>;

/**
 * A transaction that has been proven and finalized.
 */
export type FinalizedTransaction = Transaction<SignatureEnabled, Proof, Binding>;

/**
 * A transaction with proofs erased.
 */
export type ProofErasedTransaction = Transaction<Signaturish, NoProof, NoBinding>;
