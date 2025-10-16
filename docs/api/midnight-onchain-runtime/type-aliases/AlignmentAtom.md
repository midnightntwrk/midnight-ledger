[**@midnight-ntwrk/onchain-runtime v1.0.0-alpha.4**](../README.md)

***

[@midnight-ntwrk/onchain-runtime](../globals.md) / AlignmentAtom

# Type Alias: AlignmentAtom

```ts
type AlignmentAtom: {
  tag: "compress";
 } | {
  tag: "field";
 } | {
  length: number;
  tag: "bytes";
};
```

A atom in a larger [Alignment](Alignment.md).
