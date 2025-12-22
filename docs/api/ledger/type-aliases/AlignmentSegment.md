[**@midnight/ledger v6.2.0-rc.3**](../README.md)

***

[@midnight/ledger](../globals.md) / AlignmentSegment

# Type Alias: AlignmentSegment

```ts
type AlignmentSegment = 
  | {
  tag: "option";
  value: Alignment[];
}
  | {
  tag: "atom";
  value: AlignmentAtom;
};
```

A segment in a larger [Alignment](Alignment.md).
