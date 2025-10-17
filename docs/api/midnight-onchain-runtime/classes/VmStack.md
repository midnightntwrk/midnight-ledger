[**@midnight-ntwrk/onchain-runtime v1.0.0-alpha.4**](../README.md)

***

[@midnight-ntwrk/onchain-runtime](../globals.md) / VmStack

# Class: VmStack

Represents the state of the VM's stack at a specific point. The stack is an
array of [StateValue](StateValue.md)s, each of which is also annotated with whether
it is "strong" or "weak"; that is, whether it is permitted to be stored
on-chain or not.

## Constructors

### new VmStack()

```ts
new VmStack(): VmStack
```

#### Returns

[`VmStack`](VmStack.md)

## Methods

### get()

```ts
get(idx): undefined | StateValue
```

#### Parameters

##### idx

`number`

#### Returns

`undefined` \| [`StateValue`](StateValue.md)

***

### isStrong()

```ts
isStrong(idx): undefined | boolean
```

#### Parameters

##### idx

`number`

#### Returns

`undefined` \| `boolean`

***

### length()

```ts
length(): number
```

#### Returns

`number`

***

### push()

```ts
push(value, is_strong): void
```

#### Parameters

##### value

[`StateValue`](StateValue.md)

##### is\_strong

`boolean`

#### Returns

`void`

***

### removeLast()

```ts
removeLast(): void
```

#### Returns

`void`

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
