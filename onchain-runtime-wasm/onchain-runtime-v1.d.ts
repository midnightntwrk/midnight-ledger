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
