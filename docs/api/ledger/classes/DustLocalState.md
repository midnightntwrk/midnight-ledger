[**@midnight/ledger v1.0.0-rc.4**](../README.md)

***

[@midnight/ledger](../globals.md) / DustLocalState

# Class: DustLocalState

## Constructors

### Constructor

```ts
new DustLocalState(params): DustLocalState;
```

#### Parameters

##### params

[`DustParameters`](DustParameters.md)

#### Returns

`DustLocalState`

## Properties

### commitmentTreeFirstFree

```ts
readonly commitmentTreeFirstFree: bigint;
```

***

### generatingTreeFirstFree

```ts
readonly generatingTreeFirstFree: bigint;
```

***

### nullifiers

```ts
readonly nullifiers: Map<bigint, QualifiedDustOutput>;
```

***

### params

```ts
readonly params: DustParameters;
```

***

### syncTime

```ts
syncTime: Date;
```

***

### utxos

```ts
readonly utxos: QualifiedDustOutput[];
```

## Methods

### addUtxo()

```ts
addUtxo(
   nullifier, 
   utxo, 
   pendingUntil?): DustLocalState;
```

#### Parameters

##### nullifier

`bigint`

##### utxo

[`QualifiedDustOutput`](../type-aliases/QualifiedDustOutput.md)

##### pendingUntil?

`Date`

#### Returns

`DustLocalState`

***

### applyCommitmentCollapsedUpdate()

```ts
applyCommitmentCollapsedUpdate(update): DustLocalState;
```

#### Parameters

##### update

[`DustStateMerkleTreeCollapsedUpdate`](DustStateMerkleTreeCollapsedUpdate.md)

#### Returns

`DustLocalState`

***

### applyGenerationCollapsedUpdate()

```ts
applyGenerationCollapsedUpdate(update): DustLocalState;
```

#### Parameters

##### update

[`DustStateMerkleTreeCollapsedUpdate`](DustStateMerkleTreeCollapsedUpdate.md)

#### Returns

`DustLocalState`

***

### collapseCommitmentTree()

```ts
collapseCommitmentTree(commitmentIndexStart, commitmentIndexEnd): DustLocalState;
```

#### Parameters

##### commitmentIndexStart

`bigint`

##### commitmentIndexEnd

`bigint`

#### Returns

`DustLocalState`

***

### collapseGenerationTree()

```ts
collapseGenerationTree(generationIndexStart, generationIndexEnd): DustLocalState;
```

#### Parameters

##### generationIndexStart

`bigint`

##### generationIndexEnd

`bigint`

#### Returns

`DustLocalState`

***

### commitmentTreeRoot()

```ts
commitmentTreeRoot(): bigint | undefined;
```

#### Returns

`bigint` \| `undefined`

***

### findUtxoByNullifier()

```ts
findUtxoByNullifier(nullifier): 
  | QualifiedDustOutput
  | undefined;
```

#### Parameters

##### nullifier

`bigint`

#### Returns

  \| [`QualifiedDustOutput`](../type-aliases/QualifiedDustOutput.md)
  \| `undefined`

***

### generatingTreeRoot()

```ts
generatingTreeRoot(): bigint | undefined;
```

#### Returns

`bigint` \| `undefined`

***

### generationInfo()

```ts
generationInfo(qdo): 
  | DustGenerationInfo
  | undefined;
```

#### Parameters

##### qdo

[`QualifiedDustOutput`](../type-aliases/QualifiedDustOutput.md)

#### Returns

  \| [`DustGenerationInfo`](../type-aliases/DustGenerationInfo.md)
  \| `undefined`

***

### insertCommitment()

```ts
insertCommitment(
   commitmentIndex, 
   qdo, 
   own_qdo): DustLocalState;
```

#### Parameters

##### commitmentIndex

`bigint`

##### qdo

[`QualifiedDustOutput`](../type-aliases/QualifiedDustOutput.md)

##### own\_qdo

`boolean`

#### Returns

`DustLocalState`

***

### insertGenerationInfo()

```ts
insertGenerationInfo(
   generationIndex, 
   generation, 
   initialNonce?): DustLocalState;
```

#### Parameters

##### generationIndex

`bigint`

##### generation

[`DustGenerationInfo`](../type-aliases/DustGenerationInfo.md)

##### initialNonce?

`string`

#### Returns

`DustLocalState`

***

### processTtls()

```ts
processTtls(time): DustLocalState;
```

#### Parameters

##### time

`Date`

#### Returns

`DustLocalState`

***

### removeCommitment()

```ts
removeCommitment(commitmentIndex): DustLocalState;
```

#### Parameters

##### commitmentIndex

`bigint`

#### Returns

`DustLocalState`

***

### removeGenerationInfo()

```ts
removeGenerationInfo(generationIndex, generation): DustLocalState;
```

#### Parameters

##### generationIndex

`bigint`

##### generation

[`DustGenerationInfo`](../type-aliases/DustGenerationInfo.md)

#### Returns

`DustLocalState`

***

### removeUtxo()

```ts
removeUtxo(nullifier): DustLocalState;
```

#### Parameters

##### nullifier

`bigint`

#### Returns

`DustLocalState`

***

### replayEvents()

```ts
replayEvents(sk, events): DustLocalState;
```

#### Parameters

##### sk

[`DustSecretKey`](DustSecretKey.md)

##### events

[`Event`](Event.md)[]

#### Returns

`DustLocalState`

***

### replayEventsWithChanges()

```ts
replayEventsWithChanges(sk, events): DustLocalStateWithChanges;
```

#### Parameters

##### sk

[`DustSecretKey`](DustSecretKey.md)

##### events

[`Event`](Event.md)[]

#### Returns

[`DustLocalStateWithChanges`](DustLocalStateWithChanges.md)

***

### replayRawEvents()

```ts
replayRawEvents(sk, rawEvents): DustLocalStateWithChanges;
```

Replays a direct concatenation of serialized ledger events. Otherwise, acts as `replayEventsWithChanges`.

#### Parameters

##### sk

[`DustSecretKey`](DustSecretKey.md)

##### rawEvents

`Uint8Array`

#### Returns

[`DustLocalStateWithChanges`](DustLocalStateWithChanges.md)

***

### serialize()

```ts
serialize(): Uint8Array;
```

#### Returns

`Uint8Array`

***

### spend()

```ts
spend(
   sk, 
   utxo, 
   vFee, 
   ctime): [DustLocalState, DustSpend<PreProof>];
```

#### Parameters

##### sk

[`DustSecretKey`](DustSecretKey.md)

##### utxo

[`QualifiedDustOutput`](../type-aliases/QualifiedDustOutput.md)

##### vFee

`bigint`

##### ctime

`Date`

#### Returns

\[`DustLocalState`, [`DustSpend`](DustSpend.md)\<[`PreProof`](PreProof.md)\>\]

***

### toString()

```ts
toString(compact?): string;
```

#### Parameters

##### compact?

`boolean`

#### Returns

`string`

***

### updateGenerationTreeFromEvidence()

```ts
updateGenerationTreeFromEvidence(evidence): DustLocalState;
```

#### Parameters

##### evidence

[`DustGenerationTreeInsertionPath`](DustGenerationTreeInsertionPath.md)

#### Returns

`DustLocalState`

***

### walletBalance()

```ts
walletBalance(time): bigint;
```

#### Parameters

##### time

`Date`

#### Returns

`bigint`

***

### deserialize()

```ts
static deserialize(raw): DustLocalState;
```

#### Parameters

##### raw

`Uint8Array`

#### Returns

`DustLocalState`
