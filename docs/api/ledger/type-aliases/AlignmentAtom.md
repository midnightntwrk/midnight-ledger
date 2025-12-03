[**@midnight/ledger v6.1.0-rc.1**](../README.md)

***

[@midnight/ledger](../globals.md) / AlignmentAtom

# Type Alias: AlignmentAtom

```ts
type AlignmentAtom = 
  | {
  tag: "compress";
}
  | {
  tag: "field";
}
  | {
  length: number;
  tag: "bytes";
};
```

A atom in a larger [Alignment](Alignment.md).
