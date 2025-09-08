[**@midnight/zswap v4.0.0-rc**](../README.md)

***

[@midnight/zswap](../globals.md) / Offer

# Class: Offer

A full Zswap offer; the zswap part of a transaction

Consists of sets of [Input](Input.md)s, [Output](Output.md)s, and [Transient](Transient.md)s,
as well as a [deltas](Offer.md#deltas) vector of the transaction value

## Properties

### deltas

```ts
readonly deltas: Map<string, bigint>;
```

The value of this offer for each token type; note that this may be
negative

This is input coin values - output coin values, for value vectors

***

### inputs

```ts
readonly inputs: Input[];
```

The inputs this offer is composed of

***

### outputs

```ts
readonly outputs: Output[];
```

The outputs this offer is composed of

***

### transient

```ts
readonly transient: Transient[];
```

The transients this offer is composed of

## Methods

### merge()

```ts
merge(other): Offer
```

Combine this offer with another

#### Parameters

##### other

[`Offer`](Offer.md)

#### Returns

[`Offer`](Offer.md)

***

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
static deserialize(raw, netid): Offer
```

#### Parameters

##### raw

`Uint8Array`\<`ArrayBufferLike`\>

##### netid

[`NetworkId`](../enumerations/NetworkId.md)

#### Returns

[`Offer`](Offer.md)
