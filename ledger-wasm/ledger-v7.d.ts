/**
 * An onchain data value, in field-aligned binary format.
 */
export type Value = Array<Uint8Array>;
/**
 * The alignment of an onchain field-aligned binary data value.
 */
export type Alignment = AlignmentSegment[];
/**
 * A segment in a larger {@link Alignment}.
 */
export type AlignmentSegment = { tag: 'option', value: Alignment[] } | { tag: 'atom', value: AlignmentAtom };
/**
 * A atom in a larger {@link Alignment}.
 */
export type AlignmentAtom = { tag: 'compress' } | { tag: 'field' } | { tag: 'bytes', length: number };
/**
 * An onchain data value, in field-aligned binary format, annotated with its
 * alignment.
 */
export type AlignedValue = { value: Value, alignment: Alignment };
/**
 * A Zswap nullifier, as a hex-encoded 256-bit bitstring
 */
export type Nullifier = string;
/**
 * A Zswap coin commitment, as a hex-encoded 256-bit bitstring
 */
export type CoinCommitment = string;
/**
 * A contract address, as a hex-encoded 35-byte string
 */
export type ContractAddress = string;
/**
 * A user public key address, as a hex-encoded 35-byte string
 */
export type UserAddress = string;
/**
 * The internal identifier attached to a {@link TokenType}, as a hex-encoded string.
 */
export type RawTokenType = string;

/**
 * Unshielded token type (or color), as a hex-encoded 35-byte string
 */
export type UnshieldedTokenType = { tag: 'unshielded', raw: RawTokenType };
/**
 * Shielded token type (or color), as a hex-encoded 35-byte string
 */
export type ShieldedTokenType = { tag: 'shielded', raw: RawTokenType };
/**
 * Dust token type
 */
export type DustTokenType = { tag: 'dust' };
/**
 * A token type (or color), as a hex-encoded 35-byte string, shielded, unshielded, or Dust
 */
export type TokenType = UnshieldedTokenType | ShieldedTokenType | DustTokenType;
/**
 * A token domain seperator, the pre-stage of `TokenType`, as 32-byte bytearray
 */
export type DomainSeparator = Uint8Array;
/**
 * A user public key capable of receiving Zswap coins, as a hex-encoded 35-byte
 * string
 */
export type CoinPublicKey = string;
/**
 * A running tally of synthetic resource costs.
 */
export type RunningCost = {
  /**
   * The amount of (modelled) time spent reading from disk, measured in picoseconds.
   */
  readTime: bigint,
  /**
   * The amount of (modelled) time spent in single-threaded compute, measured in picoseconds.
   */
  computeTime: bigint,
  /**
   * The number of (modelled) bytes written.
   */
  bytesWritten: bigint,
  /**
   * The number of (modelled) bytes deleted.
   */
  bytesDeleted: bigint,
};

/**
 * The fee prices for transaction
 */
export type FeePrices = {
  /**
   * The overall price of a full block in an average cost dimension.
   */
  overallPrice: number,
  /**
   * The price factor of time spent reading from disk.
   */
  readFactor: number,
  /**
   * The price factor of time spent in single-threaded compute.
   */
  computeFactor: number,
  /**
   * The price factor of block usage.
   */
  blockUsageFactor: number,
  /**
   * The price factor of time spent writing to disk.
   */
  writeFactor: number,
}

/**
 * Holds the coin secret key of a user, serialized as a hex-encoded 32-byte string
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

/**
 * A Zswap nonce, as a hex-encoded 256-bit string
 */
export type Nonce = string;
/**
 * A hex-encoded signature BIP-340 verifying key, with a 3-byte version prefix
 */
export type SignatureVerifyingKey = string;
/**
 * A hex-encoded signature BIP-340 signing key, with a 3-byte version prefix
 */
export type SigningKey = string;
/**
 * A hex-encoded signature BIP-340 signature, with a 3-byte version prefix
 */
export type Signature = string;
/**
 * An internal encoding of a value of the proof systems scalar field
 */
export type Fr = Uint8Array;
/**
 * Information required to create a new coin, alongside details about the
 * recipient
 */
export type ShieldedCoinInfo = {
  /**
   * The coin's type, identifying the currency it represents
   */
  type: RawTokenType,
  /**
   * The coin's randomness, preventing it from colliding with other coins
   */
  nonce: Nonce,
  /**
   * The coin's value, in atomic units dependent on the currency
   *
   * Bounded to be a non-negative 64-bit integer
   */
  value: bigint,
};
/**
 * Information required to spend an existing coin, alongside authorization of
 * the owner
 */
export type QualifiedShieldedCoinInfo = {
  /**
   * The coin's type, identifying the currency it represents
   */
  type: RawTokenType,
  /**
   * The coin's randomness, preventing it from colliding with other coins
   */
  nonce: Nonce,
  /**
   * The coin's value, in atomic units dependent on the currency
   *
   * Bounded to be a non-negative 64-bit integer
   */
  value: bigint,
  /**
   * The coin's location in the chain's Merkle tree of coin commitments
   *
   * Bounded to be a non-negative 64-bit integer
   */
  mt_index: bigint,
};

/**
 * A key used to index into an array or map in the onchain VM
 */
export type Key = { tag: 'value', value: AlignedValue } | { tag: 'stack' };
/**
 * An individual operation in the onchain VM
 *
 * @typeParam R - `null` or {@link AlignedValue}, for gathering and verifying
 * mode respectively
 */
export type Op<R> = { noop: { n: number } } |
  'lt' |
  'eq' |
  'type' |
  'size' |
  'new' |
  'and' |
  'or' |
  'neg' |
  'log' |
  'root' |
  'pop' |
  { popeq: { cached: boolean, result: R } } |
  { addi: { immediate: number } } |
  { subi: { immediate: number } } |
  { push: { storage: boolean, value: EncodedStateValue } } |
  { branch: { skip: number } } |
  { jmp: { skip: number } } |
  'add' |
  'sub' |
  { concat: { cached: boolean, n: number } } |
  'member' |
  { rem: { cached: boolean } } |
  { dup: { n: number } } |
  { swap: { n: number } } |
  { idx: { cached: boolean, pushPath: boolean, path: Key[] } } |
  { ins: { cached: boolean, n: number } } |
  'ckpt';
/**
 * An individual result of observing the results of a non-verifying VM program
 * execution
 */
export type GatherResult = { tag: 'read', content: AlignedValue } |
  { tag: 'log', content: EncodedStateValue };
/**
 * An alternative encoding of {@link StateValue} for use in {@link Op} for
 * technical reasons
 */
export type EncodedStateValue = { tag: 'null' } |
  { tag: 'cell', content: AlignedValue } |
  { tag: 'map', content: Map<AlignedValue, EncodedStateValue> } |
  { tag: 'array', content: EncodedStateValue[] } |
  { tag: 'boundedMerkleTree', content: [number, Map<bigint, [Uint8Array, undefined]>] };
/**
 * A transcript of operations, to be recorded in a transaction
 */
export type Transcript<R> = {
  /**
   * The execution budget for this transcript, which {@link program} must not
   * exceed
   */
  gas: RunningCost,
  /**
   * The effects of the transcript, which are checked before execution, and
   * must match those constructed by {@link program}
   */
  effects: Effects,
  /**
   * The sequence of operations that this transcript captured
   */
  program: Op<R>[],
};
/**
 * A public address that an entity can be identified by
 */
export type PublicAddress = { tag: 'user', address: UserAddress } | { tag: 'contract', address: ContractAddress }
/**
 * The context information of a call provided to the VM.
 */
export type CallContext = {
  ownAddress: ContractAddress,
  /**
   * The commitment indices map accessible to the contract.
   */
  comIndices: Map<CoinCommitment, number>
  /**
   * The seconds since the UNIX epoch that have elapsed
   */
  secondsSinceEpoch: bigint,
  /**
   * The maximum error on {@link secondsSinceEpoch} that should occur, as a
   * positive seconds value
   */
  secondsSinceEpochErr: number,
  /**
   * The hash of the block prior to this transaction, as a hex-encoded string
   */
  parentBlockHash: string,
  /**
   * The balances held by the called contract at the time it was called.
   */
  balance: Map<TokenType, bigint>,
  /**
   * A public address identifying an entity.
   */
  caller?: PublicAddress,
};
/**
 * Context information about the block forwarded to {@link CallContext}.
 */
export type BlockContext = {
  /**
   * The seconds since the UNIX epoch that have elapsed
   */
  secondsSinceEpoch: bigint,
  /**
   * The maximum error on {@link secondsSinceEpoch} that should occur, as a
   * positive seconds value
   */
  secondsSinceEpochErr: number,
  /**
   * The hash of the block prior to this transaction, as a hex-encoded string
   */
  parentBlockHash: string,
};
/**
 * The contract-external effects of a transcript.
 */
export type Effects = {
  /**
   * The nullifiers (spends) this contract call requires
   */
  claimedNullifiers: Nullifier[],
  /**
   * The coin commitments (outputs) this contract call requires, as coins
   * received
   */
  claimedShieldedReceives: CoinCommitment[],
  /**
   * The coin commitments (outputs) this contract call requires, as coins
   * sent
   */
  claimedShieldedSpends: CoinCommitment[],
  /**
   * The contracts called from this contract. The values are, in order:
   *
   * - The sequence number of this call
   * - The contract being called
   * - The entry point being called
   * - The communications commitment
   */
  claimedContractCalls: Array<[bigint, ContractAddress, string, Fr]>,
  /**
   * The shielded tokens minted in this call, as a map from hex-encoded 256-bit domain
   * separators to unsigned 64-bit integers.
   */
  shieldedMints: Map<string, bigint>,
  /**
   * The unshielded tokens minted in this call, as a map from hex-encoded 256-bit domain
   * separators to unsigned 64-bit integers.
   */
  unshieldedMints: Map<string, bigint>,
  /**
   * The unshielded inputs this contract expects.
   */
  unshieldedInputs: Map<TokenType, bigint>,
  /**
   * The unshielded outputs this contract authorizes.
   */
  unshieldedOutputs: Map<TokenType, bigint>,
  /**
   * The unshielded UTXO outputs this contract expects to be present.
   */
  claimedUnshieldedSpends: Map<[TokenType, PublicAddress], bigint>,
};

/**
 * A hex-encoded commitment of data shared between two contracts in a call
 */
export type CommunicationCommitment = string;
/**
 * The hex-encoded randomness to {@link CommunicationCommitment}
 */
export type CommunicationCommitmentRand = string;

/**
 * Samples a new {@link CommunicationCommitmentRand} uniformly
 */
export function communicationCommitmentRandomness(): CommunicationCommitmentRand;

/**
 * Computes the communication commitment corresponding to an input/output pair and randomness.
 */
export function communicationCommitment(input: AlignedValue, output: AlignedValue, rand: CommunicationCommitmentRand): CommunicationCommitment;

/**
 * Computes the (hex-encoded) hash of a given contract entry point. Used in
 * composable contracts to reference the called contract's entry point ID
 * in-circuit.
 */
export function entryPointHash(entryPoint: string | Uint8Array): string;

/**
 * Randomly samples a {@link SigningKey}.
 */
export function sampleSigningKey(): SigningKey;

/**
 * Creates a {@link SigningKey} from provided Bip340 private key.
 */
export function signingKeyFromBip340(data: Uint8Array): SigningKey;

/**
 * Signs arbitrary data with the given signing key.
 *
 * WARNING: Do not expose access to this function for valuable keys for data
 * that is not strictly controlled!
 */
export function signData(key: SigningKey, data: Uint8Array): Signature;

/**
 * Returns the verifying key for a given signing key
 */
export function signatureVerifyingKey(sk: SigningKey): SignatureVerifyingKey;

/**
 * Verifies if a signature is correct
 */
export function verifySignature(vk: SignatureVerifyingKey, data: Uint8Array, signature: Signature): boolean;

/**
 * Encode a raw {@link RawTokenType} into a `Uint8Array` for use in Compact's
 * `RawTokenType` type
 */
export function encodeRawTokenType(tt: RawTokenType): Uint8Array;

/**
 * Decode a raw {@link RawTokenType} from a `Uint8Array` originating from Compact's
 * `RawTokenType` type
 */
export function decodeRawTokenType(tt: Uint8Array): RawTokenType;

/**
 * Encode a {@link ContractAddress} into a `Uint8Array` for use in Compact's
 * `ContractAddress` type
 */
export function encodeContractAddress(addr: ContractAddress): Uint8Array;

/**
 * Decode a {@link ContractAddress} from a `Uint8Array` originating from
 * Compact's `ContractAddress` type
 */
export function decodeContractAddress(addr: Uint8Array): ContractAddress;

/**
 * Encode a {@link UserAddress} into a `Uint8Array` for use in Compact's
 * `UserAddress` type
 */
export function encodeUserAddress(addr: UserAddress): Uint8Array;

/**
 * Decode a {@link UserAddress} from a `Uint8Array` originating from
 * Compact's `UserAddress` type
 */
export function decodeUserAddress(addr: Uint8Array): UserAddress;

/**
 * Encode a {@link CoinPublicKey} into a `Uint8Array` for use in Compact's
 * `CoinPublicKey` type
 */
export function encodeCoinPublicKey(pk: CoinPublicKey): Uint8Array;

/**
 * Decode a {@link CoinPublicKey} from a `Uint8Array` originating from Compact's
 * `CoinPublicKey` type
 */
export function decodeCoinPublicKey(pk: Uint8Array): CoinPublicKey;

/**
 * Encode a {@link ShieldedCoinInfo} into a Compact's `ShieldedCoinInfo` TypeScript
 * representation
 */
export function encodeShieldedCoinInfo(coin: ShieldedCoinInfo): { color: Uint8Array, nonce: Uint8Array, value: bigint };

/**
 * Encode a {@link QualifiedShieldedCoinInfo} into a Compact's `QualifiedShieldedCoinInfo`
 * TypeScript representation
 */
export function encodeQualifiedShieldedCoinInfo(coin: QualifiedShieldedCoinInfo): {
  color: Uint8Array,
  nonce: Uint8Array,
  value: bigint,
  mt_index: bigint
};

/**
 * Decode a {@link ShieldedCoinInfo} from Compact's `ShieldedCoinInfo` TypeScript representation
 */
export function decodeShieldedCoinInfo(coin: { color: Uint8Array, nonce: Uint8Array, value: bigint }): ShieldedCoinInfo;

/**
 * Decode a {@link QualifiedShieldedCoinInfo} from Compact's `QualifiedShieldedCoinInfo`
 * TypeScript representation
 */
export function decodeQualifiedShieldedCoinInfo(coin: {
  color: Uint8Array,
  nonce: Uint8Array,
  value: bigint,
  mt_index: bigint
}): QualifiedShieldedCoinInfo;

/**
 * Derives the raw {@link RawTokenType} associated with a particular
 * {@link DomainSeparator} and contract.
 */
export function rawTokenType(domain_sep: DomainSeparator, contract: ContractAddress): RawTokenType;

/**
 * Samples a uniform contract address, for use in testing
 */
export function sampleContractAddress(): ContractAddress;

/**
 * Samples a uniform user address, for use in testing
 */
export function sampleUserAddress(): UserAddress;

/**
 * Samples a uniform raw token type, for use in testing to construct
 * both the shielded and unshielded token types.
 */
export function sampleRawTokenType(): RawTokenType;

/**
 * A sample contract address
 */
export function dummyContractAddress(): ContractAddress;

/**
 * A sample user address
 */
export function dummyUserAddress(): UserAddress;

/**
 * Internal implementation of the runtime's coin commitment primitive.
 * @internal
 */
export function runtimeCoinCommitment(coin: AlignedValue, recipient: AlignedValue): AlignedValue;

/**
 * Internal implementation of the runtime's coin nullifier primitive.
 * @internal
 */
export function runtimeCoinNullifier(coin: AlignedValue, sender_evidence: AlignedValue): AlignedValue;

/**
 * Internal implementation of the Merkle tree leaf hash primitive.
 * @internal
 */
export function leafHash(value: AlignedValue): AlignedValue;

/**
 * Internal implementation of the max aligned size primitive.
 * @internal
 */
export function maxAlignedSize(alignment: Alignment): bigint;

/**
 * Returns the maximum representable value in the proof systems scalar field
 * (that is, 1 less than the prime modulus)
 */
export function maxField(): bigint;

/**
 * Converts input, output, and transcript information into a proof preimage
 * suitable to pass to a `ProvingProvider`.
 *
 * The `key_location` parameter is a string used to identify the circuit by
 * proving machinery, for backwards-compatibility, if unset it defaults to
 * `'dummy'`.
 */
export function proofDataIntoSerializedPreimage(
  input: AlignedValue,
  output: AlignedValue,
  public_transcript: Op<AlignedValue>[],
  private_transcript_outputs: AlignedValue[],
  key_location?: string,
): Uint8Array;

/**
 * Takes a bigint modulus the proof systems scalar field
 */
export function bigIntModFr(x: bigint): bigint;

/**
 * Internal conversion between field-aligned binary values and bigints within
 * the scalar field
 * @internal
 * @throws If the value does not encode a field element
 */
export function valueToBigInt(x: Value): bigint;

/**
 * Internal conversion between bigints and their field-aligned binary
 * representation
 * @internal
 */
export function bigIntToValue(x: bigint): Value;

/**
 * Internal implementation of the transient hash primitive
 * @internal
 * @throws If {@link val} does not have alignment {@link align}
 */
export function transientHash(align: Alignment, val: Value): Value;

/**
 * Internal implementation of the transient commitment primitive
 * @internal
 * @throws If {@link val} does not have alignment {@link align}, or
 * {@link opening} does not encode a field element
 */
export function transientCommit(align: Alignment, val: Value, opening: Value): Value;

/**
 * Internal implementation of the persistent hash primitive
 * @internal
 * @throws If {@link val} does not have alignment {@link align}, or any
 * component has a compress alignment
 */
export function persistentHash(align: Alignment, val: Value): Value;

/**
 * Internal implementation of the persistent commitment primitive
 * @internal
 * @throws If {@link val} does not have alignment {@link align},
 * {@link opening} does not encode a 32-byte bytestring, or any component has a
 * compress alignment
 */
export function persistentCommit(align: Alignment, val: Value, opening: Value): Value;

/**
 * Internal implementation of the degrade to transient primitive
 * @internal
 * @throws If {@link persistent} does not encode a 32-byte bytestring
 */
export function degradeToTransient(persistent: Value): Value;

/**
 * Internal implementation of the upgrade from transient primitive
 * @internal
 * @throws If {@link transient} does not encode a field element
 */
export function upgradeFromTransient(transient: Value): Value;

/**
 * Internal implementation of the hash to curve primitive
 * @internal
 * @throws If {@link val} does not have alignment {@link align}
 */
export function hashToCurve(align: Alignment, val: Value): Value;

/**
 * Internal implementation of the elliptic curve addition primitive
 * @internal
 * @throws If either input does not encode an elliptic curve point
 */
export function ecAdd(a: Value, b: Value): Value;

/**
 * Internal implementation of the elliptic curve multiplication primitive
 * @internal
 * @throws If {@link a} does not encode an elliptic curve point or {@link b}
 * does not encode a field element
 */
export function ecMul(a: Value, b: Value): Value;

/**
 * Internal implementation of the elliptic curve generator multiplication
 * primitive
 * @internal
 * @throws if {@link val} does not encode a field element
 */
export function ecMulGenerator(val: Value): Value;

/**
 * Runs a VM program against an initial stack, with an optional gas limit
 */
export function runProgram(initial: VmStack, ops: Op<null>[], cost_model: CostModel, gas_limit?: RunningCost): VmResults;

/**
 * An individual operation, or entry point of a contract, consisting primarily
 * of a ZK verifier keys, potentially for different versions of the proving
 * system.
 *
 * Only the latest available version is exposed to this API.
 *
 * Note that the serialized form of the key is checked on initialization
 */
export class ContractOperation {
  constructor();

  verifierKey: Uint8Array;

  serialize(): Uint8Array;

  static deserialize(raw: Uint8Array): ContractOperation;

  toString(compact?: boolean): string;
}

/**
 * A committee permitted to make changes to this contract. If a threshold of
 * the public keys in this committee sign off, they can change the rules of
 * this contract, or recompile it for a new version.
 *
 * If the threshold is greater than the number of committee members, it is
 * impossible for them to sign anything.
 */
export class ContractMaintenanceAuthority {
  /**
   * Constructs a new authority from its components
   *
   * If not supplied, `counter` will default to `0n`. Values should be
   * non-negative, and at most 2^32 - 1.
   *
   * At deployment, `counter` must be `0n`, and any subsequent update should
   * set counter to exactly one greater than the current value.
   */
  constructor(committee: Array<SignatureVerifyingKey>, threshold: number, counter?: bigint);

  /**
   * The committee public keys
   */
  readonly committee: Array<SignatureVerifyingKey>;
  /**
   * How many keys must sign rule changes
   */
  readonly threshold: number;
  /**
   * The replay protection counter
   */
  readonly counter: bigint;

  serialize(): Uint8Array;

  static deserialize(raw: Uint8Array): ContractState;

  toString(compact?: boolean): string;
}

/**
 * The state of a contract, consisting primarily of the {@link data} accessible
 * directly to the contract, and the map of {@link ContractOperation}s that can
 * be called on it, the keys of which can be accessed with {@link operations},
 * and the individual operations can be read with {@link operation} and written
 * to with {@link setOperation}.
 */
export class ContractState {
  /**
   * Creates a blank contract state
   */
  constructor();

  /**
   * Return a list of the entry points currently registered on this contract
   */
  operations(): Array<string | Uint8Array>

  /**
   * Get the operation at a specific entry point name
   */
  operation(operation: string | Uint8Array): ContractOperation | undefined;

  /**
   * Set a specific entry point name to contain a given operation
   */
  setOperation(operation: string | Uint8Array, value: ContractOperation): void;

  /**
   * Runs a series of operations against the current state, and returns the
   * results
   */
  query(query: Op<null>[], cost_model: CostModel): GatherResult[];

  serialize(): Uint8Array;

  static deserialize(raw: Uint8Array): ContractState;

  toString(compact?: boolean): string;

  /**
   * The current value of the primary state of the contract
   */
  data: ChargedState;
  /**
   * The maintenance authority associated with this contract
   */
  maintenanceAuthority: ContractMaintenanceAuthority;
  /**
   * The public balances held by this contract
   */
  balance: Map<TokenType, bigint>;
}

/**
 * Provides the information needed to fully process a transaction, including
 * information about the rest of the transaction, and the state of the chain at
 * the time of execution.
 */
export class QueryContext {
  /**
   * Construct a basic context from a contract's address and current state
   * value
   */
  constructor(state: ChargedState, address: ContractAddress);

  /**
   * Register a given coin commitment as being accessible at a specific index,
   * for use when receiving coins in-contract, and needing to record their
   * index to later spend them
   */
  insertCommitment(comm: CoinCommitment, index: bigint): QueryContext;

  /**
   * Internal counterpart to {@link insertCommitment}; upgrades an encoded
   * {@link ShieldedCoinInfo} to an encoded {@link QualifiedShieldedCoinInfo} using the
   * inserted commitments
   * @internal
   */
  qualify(coin: Value): Value | undefined;

  /**
   * Runs a transcript in verifying mode against the current query context,
   * outputting a new query context, with the {@link state} and {@link effects}
   * from after the execution.
   */
  runTranscript(transcript: Transcript<AlignedValue>, cost_model: CostModel): QueryContext;

  /**
   * Runs a sequence of operations in gather mode, returning the results of the
   * gather.
   */
  query(ops: Op<null>[], cost_model: CostModel, gas_limit?: RunningCost): QueryResults;

  /**
   * Converts the QueryContext to {@link VmStack}.
   */
  toVmStack(): VmStack;

  toString(compact?: boolean): string;

  /**
   * The address of the contract
   */
  readonly address: ContractAddress;
  /**
   * The block-level information accessible to the contract
   */
  block: CallContext;
  /**
   * The commitment indices map accessible to the contract, primarily via
   * {@link qualify}
   */
  readonly comIndices: Map<CoinCommitment, bigint>;
  /**
   * The effects that occurred during execution against this context, should
   * match those declared in a {@link Transcript}
   */
  effects: Effects;
  /**
   * The current contract state retained in the context
   */
  readonly state: ChargedState;
}

/**
 * A cost model for calculating transaction fees
 */
export class CostModel {
  private constructor();

  /**
   * The initial cost model of Midnight
   */
  static initialCostModel(): CostModel;

  toString(compact?: boolean): string;
}

/**
 * The results of making a query against a specific state or context
 */
export class QueryResults {
  private constructor();

  toString(compact?: boolean): string;

  /**
   * The context state after executing the query. This can be used to execute
   * further queries
   */
  readonly context: QueryContext;
  /**
   * Any events/results that occurred during or from the query
   */
  readonly events: GatherResult[];
  /**
   * The measured cost of executing the query
   */
  readonly gasCost: RunningCost;
}

/**
 * Represents a fixed-depth Merkle tree storing hashed data, whose preimages
 * are unknown
 */
export class StateBoundedMerkleTree {
  /**
   * Create a blank tree with the given height
   */
  constructor(height: number);

  /**
   * Internal implementation of the merkle tree root primitive.
   * Returns undefined if the tree has not been fully hashed.
   * @internal
   */
  root(): AlignedValue | undefined;

  /**
   * Internal implementation of the finding path primitive.
   * Returns undefined if the leaf is not in the tree.
   * @internal
   */
  findPathForLeaf(leaf: AlignedValue): AlignedValue | undefined;

  /**
   * Internal implementation of the path construction primitive
   * @internal
   * @throws If the index is out-of-bounds for the tree
   */
  pathForLeaf(index: bigint, leaf: AlignedValue): AlignedValue;

  /**
   * Inserts a value into the Merkle tree, returning the updated tree
   * @throws If the index is out-of-bounds for the tree
   */
  update(index: bigint, leaf: AlignedValue): StateBoundedMerkleTree;

  /**
   * Rehashes the tree, updating all internal hashes and ensuring all
   * node hashes are present. Necessary because the onchain runtime does
   * not automatically rehash trees.
   */
  rehash(): StateBoundedMerkleTree;

  /**
   * Erases all but necessary hashes between, and inclusive of, `start` and
   * `end` inidices @internal
   * @throws If the indices are out-of-bounds for the tree, or `end < start`
   */
  collapse(start: bigint, end: bigint): StateBoundedMerkleTree;

  toString(compact?: boolean): string;

  readonly height: number;
}

/**
 * Represents a key-value map, where keys are {@link AlignedValue}s, and values
 * are {@link StateValue}s.
 */
export class StateMap {
  constructor();

  keys(): AlignedValue[];

  get(key: AlignedValue): StateValue | undefined;

  insert(key: AlignedValue, value: StateValue): StateMap;

  remove(key: AlignedValue): StateMap;

  toString(compact?: boolean): string;
}

/**
 * Represents a {@link StateValue} with storage annotations.
 *
 * These track the state usage that has been charged for so far.
 */
export class ChargedState {
  constructor(state: StateValue);
  readonly state: StateValue;
  toString(compact?: boolean): string;
}

/**
 * Represents the core of a contract's state, and recursively represents each
 * of its components.
 *
 * There are different *classes* of state values:
 * - `null`
 * - Cells of {@link AlignedValue}s
 * - Maps from {@link AlignedValue}s to state values
 * - Bounded Merkle trees containing {@link AlignedValue} leaves
 * - Short (\<= 15 element) arrays of state values
 *
 * State values are *immutable*, any operations that mutate states will return
 * a new state instead.
 */
export class StateValue {
  private constructor();

  type(): 'null' | 'cell' | 'map' | 'array' | 'boundedMerkleTree';

  static newNull(): StateValue;

  static newCell(value: AlignedValue): StateValue;

  static newMap(map: StateMap): StateValue;

  static newBoundedMerkleTree(tree: StateBoundedMerkleTree): StateValue;

  static newArray(): StateValue;

  arrayPush(value: StateValue): StateValue;

  asCell(): AlignedValue;

  asMap(): StateMap | undefined;

  asBoundedMerkleTree(): StateBoundedMerkleTree | undefined;

  asArray(): StateValue[] | undefined;

  logSize(): number;

  toString(compact?: boolean): string;

  /**
   * @internal
   */
  encode(): EncodedStateValue;

  /**
   * @internal
   */
  static decode(value: EncodedStateValue): StateValue;
}

/**
 * Represents the results of a VM call
 */
export class VmResults {
  private constructor();

  toString(compact?: boolean): string;

  /**
   * The events that got emitted by this VM invocation
   */
  readonly events: GatherResult[];
  /**
   * The computed gas cost of running this VM invocation
   */
  readonly gasCost: RunningCost;
  /**
   * The VM stack at the end of the VM invocation
   */
  readonly stack: VmStack;
}

/**
 * Represents the state of the VM's stack at a specific point. The stack is an
 * array of {@link StateValue}s, each of which is also annotated with whether
 * it is "strong" or "weak"; that is, whether it is permitted to be stored
 * on-chain or not.
 */
export class VmStack {
  constructor();

  push(value: StateValue, is_strong: boolean): void;

  removeLast(): void;

  length(): number;

  get(idx: number): StateValue | undefined;

  isStrong(idx: number): boolean | undefined;

  toString(compact?: boolean): string;
}


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
 * A normalized form of {@link SyntheticCost}.
 */
export type NormalizedCost = {
  /**
   * The amount of (modelled) time spent reading from disk, measured in picoseconds.
   */
  readTime: number,
  /**
   * The amount of (modelled) time spent in single-threaded compute, measured in picoseconds.
   */
  computeTime: number,
  /**
   * The number of bytes of blockspace used
   */
  blockUsage: number,
  /**
   * The net number of (modelled) bytes written, i.e. max(0, absolute written bytes less deleted bytes).
   */
  bytesWritten: number,
  /**
   * The number of (modelled) bytes written temporarily or overwritten.
   */
  bytesChurned: number,
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

/**
 * A secret key for the Dust, used to derive Dust UTxO nonces and prove credentials to spend Dust UTxOs
 */
export class DustSecretKey {
  private constructor();

  /**
   * Temporary method to create an instance of {@link DustSecretKey} from a bigint (its natural representation)
   * @param bigint
   */
  static fromBigint(bigint: bigint): DustSecretKey;

  /**
   * Create an instance of {@link DustSecretKey} from a seed.
   * @param seed
   */
  static fromSeed(seed: Uint8Array): DustSecretKey;

  publicKey: DustPublicKey;

  /**
   * Clears the dust secret key, so that it is no longer usable nor held in memory
   */
  clear(): void;
}

// TODO: Doc comments
export type DustPublicKey = bigint;
export type DustInitialNonce = string;
export type DustNonce = bigint;
export type DustCommitment = bigint;
export type DustNullifier = bigint;

export function sampleDustSecretKey(): DustSecretKey;

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

export class DustStateChanges {
  private constructor();
  /**
   * The source of the state change, as a hex-encoded string
   */
  readonly source: string;
  /**
   * The UTXOs that were received in this state change
   */
  readonly receivedUtxos: QualifiedDustOutput[];
  /**
   * The UTXOs that were spent in this state change
   */
  readonly spentUtxos: QualifiedDustOutput[];
}

export class DustLocalStateWithChanges {
  private constructor();
  /**
   * The updated local state after replaying events
   */
  readonly state: DustLocalState;
  /**
   * The state changes that occurred during the replay
   */
  readonly changes: DustStateChanges[];
}

export class DustLocalState {
  constructor(params: DustParameters);
  walletBalance(time: Date): bigint;
  generationInfo(qdo: QualifiedDustOutput): DustGenerationInfo | undefined;
  spend(sk: DustSecretKey, utxo: QualifiedDustOutput, vFee: bigint, ctime: Date): [DustLocalState, DustSpend<PreProof>];
  processTtls(time: Date): DustLocalState;
  replayEvents(sk: DustSecretKey, events: Event[]): DustLocalState;
  replayEventsWithChanges(sk: DustSecretKey, events: Event[]): DustLocalStateWithChanges;
  serialize(): Uint8Array;
  static deserialize(raw: Uint8Array): DustLocalState;
  toString(compact?: boolean): string;
  readonly utxos: QualifiedDustOutput[];
  readonly params: DustParameters;
  readonly syncTime: Date;
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
  postBlockUpdate(tblock: Date, detailedBlockFullness?: NormalizedCost, overallBlockFullness?: number): LedgerState;

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
  /**
   * The parameters of the ledger
   */
  parameters: LedgerParameters;
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
 * A {@link ContractCall} prior to being partitioned into guarnateed and
 * fallible parts, for use with {@link Transaction.addCalls}.
 *
 * Note that this is similar, but not the same as {@link ContractCall}, which
 * assumes {@link partitionTranscripts} was already used. {@link
 * Transaction.addCalls} is a replacement for this that also handles
 * Zswap components, and creates relevant intents when needed.
 */
export class PrePartitionContractCall {
  constructor(
    address: ContractAddress,
    entry_point: Uint8Array | string,
    op: ContractOperation,
    pre_transcript: PreTranscript,
    private_transcript_outputs: AlignedValue[],
    input: AlignedValue,
    output: AlignedValue,
    communication_commitment_rand: CommunicationCommitmentRand,
    key_location: string
  );
  toString(compact?: boolean): string;
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
  constructor(version: 'v3');

  readonly version: 'v3';

  toString(compact?: boolean): string;
}

/**
 * A versioned verifier key to be associated with a {@link ContractOperation}.
 */
export class ContractOperationVersionedVerifierKey {
  constructor(version: 'v3', rawVk: Uint8Array);

  readonly version: 'v3';
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
 * Specifies where something should execute in a transaction.
 *
 * Options are:
 * - As the first thing (alias for `{ tag: 'specific', value: 1 }`)
 * - In any physical segment, but only utilising the guaranteed logical segment
 * - In a random segment (ideal for merging with other intents)
 * - In a specific directly provided segment (in the range 1..65535)
 */
export type SegmentSpecifier = { tag: 'first' } | { tag: 'guaranteedOnly' } | { tag: 'random' } | { tag: 'specific', value: number };

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
  mockProve(): Transaction<S, Proof, Binding>;
  /**
   * Proves the transaction, with access to a low-level proving provider.
   * This may *only* be called for `P = PreProof`.
   *
   * @throws If called on bound, proven, or proof-erased transactions.
   */
  prove(provider: ProvingProvider, cost_model: CostModel): Promise<Transaction<S, Proof, B>>;

  /**
   * Adds a set of new calls to the transaction.
   *
   * In contrast to {@link Intent.addCall}, this takes calls *before*
   * transcript partitioning ({@link partitionTranscripts}), will create the
   * target intent where needed, and will ensure that relevant Zswap parts are
   * placed in the same section as contract interactions with them.
   *
   * @throws If called on bound, proven, or proof-erased transactions.
   */
  addCalls(
    segment: SegmentSpecifier,
    calls: PrePartitionContractCall[],
    params: LedgerParameters,
    ttl: Date,
    zswapInputs?: ZswapInput<PreProof>[],
    zswapOutputs?: ZswapOutput<PreProof>[],
    zswapTransient?: ZswapTransient<PreProof>[],
  ): Transaction<S, P, B>;

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
  cost(params: LedgerParameters, enforceTimeToDismiss?: boolean): SyntheticCost;

  /**
   * The cost of this transaction, in SPECKs.
   *
   * Note that this is *only* accurate when called with proven transactions.
   */
  fees(params: LedgerParameters, enforceTimeToDismiss?: boolean): bigint;

  /**
   * The cost of this transaction, in SPECKs, with a safety margin of `n` blocks applied.
   *
   * As with {@link fees}, this is only accurate for proven transactions.
   *
   * Warning: `n` must be a non-negative integer, and it is an exponent, it is
   * very easy to get a completely unreasonable margin here!
   */
  feesWithMargin(params: LedgerParameters, margin: number): bigint;

  toString(compact?: boolean): string;

  /**
   * The rewards this transaction represents, if applicable
   */
  readonly rewards: ClaimRewardsTransaction<S> | undefined;
  /**
   * The intents contained in this transaction
   *
   * Note that writing to this re-computes binding information if and only if
   * this transaction is unbound *and* unproven. If this is not the case,
   * creating or removing intents will lead to a binding error down the line,
   * but modifying existing intents will succeed.
   *
   * @throws On writing if `B` is {@link Binding} or this is not a standard
   * transaction
   */
  intents: Map<number, Intent<S, P, B>> | undefined;
  /**
   * The fallible Zswap offer
   *
   * Note that writing to this re-computes binding information if and only if
   * this transaction is unbound *and* unproven. If this is not the case,
   * creating or removing offer components will lead to a binding error down
   * the line.
   *
   * @throws On writing if `B` is {@link Binding} or this is not a standard
   * transaction
   */
  fallibleOffer: Map<number, ZswapOffer<P>> | undefined;
  /**
   * The guaranteed Zswap offer
   *
   * Note that writing to this re-computes binding information if and only if
   * this transaction is unbound *and* unproven. If this is not the case,
   * creating or removing offer components will lead to a binding error down
   * the line.
   *
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

export type PartitionedTranscript = [Transcript<AlignedValue> | undefined, Transcript<AlignedValue> | undefined];

/**
 * Computes the communication commitment corresponding to an input/output pair and randomness.
 */
export function communicationCommitment(input: AlignedValue, output: AlignedValue, rand: CommunicationCommitmentRand): CommunicationCommitment;

/**
 * Finalizes a set of programs against their initial contexts,
 * resulting in guaranteed and fallible {@link Transcript}s, optimally
 * allocated, and heuristically covered for gas fees.
 */
export function partitionTranscripts(calls: PreTranscript[], params: LedgerParameters): PartitionedTranscript[];

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

  /**
   * The maximum price adjustment per block with the current parameters, as a multiplicative
   * factor (that is: 1.1 would indicate a 10% adjustment). Will always return the positive (>1)
   * adjustment factor. Note that negative adjustments are the additive inverse (1.1 has a
   * corresponding 0.9 downward adjustment), *not* the multiplicative as might reasonably be
   * assumed.
   */
  maxPriceAdjustment(): number;

  serialize(): Uint8Array;

  static deserialize(raw: Uint8Array): LedgerParameters;

  toString(compact?: boolean): string;

  /**
   * Normalizes a detailed block fullness cost to the block limits.
   *
   * @throws if any of the block limits is exceeded
   */
  normalizeFullness(fullness: SyntheticCost): NormalizedCost;

  /**
   * The fee prices for transaction
   */
  readonly feePrices: FeePrices;
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

  /**
   * A cost model for calculating transaction fees
   */
  readonly runtimeCostModel: CostModel;

  /**
   * A baseline cost to begin with
   */
  readonly baselineCost: RunningCost;
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
  yesIKnowTheSecurityImplicationsOfThis_taggedSerialize(): Uint8Array;

  static deserialize(raw: Uint8Array): EncryptionSecretKey
  static taggedDeserialize(raw: Uint8Array): EncryptionSecretKey
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

export class ZswapStateChanges {
  private constructor();
  /**
   * The source of the state change, as a hex-encoded string
   */
  readonly source: string;
  /**
   * The coins that were received in this state change
   */
  readonly receivedCoins: QualifiedShieldedCoinInfo[];
  /**
   * The coins that were spent in this state change
   */
  readonly spentCoins: QualifiedShieldedCoinInfo[];
}

export class ZswapLocalStateWithChanges {
  private constructor();
  /**
   * The updated local state after replaying events
   */
  readonly state: ZswapLocalState;
  /**
   * The state changes that occurred during the replay
   */
  readonly changes: ZswapStateChanges[];
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
   * Replays observed events against the current local state, returning both the updated state
   * and the state changes. These *must* be replayed in the same order as emitted by the chain being followed.
   */
  replayEventsWithChanges(secretKeys: ZswapSecretKeys, events: Event[]): ZswapLocalStateWithChanges;
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
  spend(secretKeys: ZswapSecretKeys, coin: QualifiedShieldedCoinInfo, segment: number | undefined, ttl?: Date): [ZswapLocalState, UnprovenInput];

  /**
   * Initiates a new spend of a new-yet-received output, outputting the
   * corresponding {@link ZswapTransient}, and the updated state marking
   * this coin as in-flight.
   */
  spendFromOutput(secretKeys: ZswapSecretKeys, coin: QualifiedShieldedCoinInfo, segment: number | undefined, output: UnprovenOutput, ttl?: Date): [ZswapLocalState, UnprovenTransient];

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

  static newContractOwned(coin: QualifiedShieldedCoinInfo, segment: number | undefined, contract: ContractAddress, state: ZswapChainState): UnprovenInput;

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
  static new(coin: ShieldedCoinInfo, segment: number | undefined, target_cpk: CoinPublicKey, target_epk: EncPublicKey): UnprovenOutput;

  /**
   * Creates a new output, targeted to a smart contract
   *
   * A contract must *also* explicitly receive a coin created in this way for
   * the output to be valid
   */
  static newContractOwned(coin: ShieldedCoinInfo, segment: number | undefined, contract: ContractAddress): UnprovenOutput;

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
  static newFromContractOwnedOutput(coin: QualifiedShieldedCoinInfo, segment: number | undefined, output: UnprovenOutput): UnprovenTransient;

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
  constructor(markerS: S['instance'], network_id: string, value: bigint, owner: SignatureVerifyingKey, nonce: Nonce, signature: S, kind?: ClaimKind);

  static new(network_id: string, value: bigint, owner: SignatureVerifyingKey, nonce: Nonce, kind: ClaimKind): ClaimRewardsTransaction<SignatureErased>;

  addSignature(signature: Signature): ClaimRewardsTransaction<SignatureEnabled>;

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
   *
   * The `type_` and `value` parameters are deprecated and will be ignored.
   */
  static fromInput<P extends Proofish>(input: ZswapInput<P>, type_?: RawTokenType, value?: bigint): ZswapOffer<P>;

  /**
   * Creates a singleton offer, from an {@link ZswapOutput} and its value
   * vector
   *
   * The `type_` and `value` parameters are deprecated and will be ignored.
   */
  static fromOutput<P extends Proofish>(output: ZswapOutput<P>, type_?: RawTokenType, value?: bigint): ZswapOffer<P>;

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
