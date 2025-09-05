[**@midnight/zswap v4.0.0-rc**](../README.md)

***

[@midnight/zswap](../globals.md) / MerkleTreeCollapsedUpdate

# Class: MerkleTreeCollapsedUpdate

A compact delta on the coin commitments Merkle tree, used to keep local
spending trees in sync with the global state without requiring receiving all
transactions.

## Constructors

### new MerkleTreeCollapsedUpdate()

```ts
new MerkleTreeCollapsedUpdate(
   state, 
   start, 
   end): MerkleTreeCollapsedUpdate
```

Create a new compact update from a non-compact state, and inclusive
`start` and `end` indices

#### Parameters

##### state

[`ZswapChainState`](ZswapChainState.md)

##### start

`bigint`

##### end

`bigint`

#### Returns

[`MerkleTreeCollapsedUpdate`](MerkleTreeCollapsedUpdate.md)

#### Throws

If the indices are out-of-bounds for the state, or `end < start`

## Methods

### serialize()

```ts
serialize(netid): Uint8Array<ArrayBufferLike>
```

#### Parameters

##### netid

[`NetworkId`](../enumerations/NetworkId.md)

#### Returns

`Uint8Array`\<`ArrayBufferLike`\>

***

### toString()

```ts
toString(compact?): string
```

#### Parameters

##### compact?

`boolean`

#### Returns

`string`

***

### deserialize()

```ts
static deserialize(raw, netid): MerkleTreeCollapsedUpdate
```

#### Parameters

##### raw

`Uint8Array`\<`ArrayBufferLike`\>

##### netid

[`NetworkId`](../enumerations/NetworkId.md)

#### Returns

[`MerkleTreeCollapsedUpdate`](MerkleTreeCollapsedUpdate.md)
