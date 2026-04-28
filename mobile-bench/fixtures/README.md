# zkir fixtures

These three files are the smallest checked-in zkir example used by the
mobile-bench prover. They are a copy of the `fallible/count` artifacts
produced by `nix build .#test-artifacts`.

To regenerate:

```bash
TA="$(nix build .#test-artifacts --no-link --print-out-paths)"
cp "$TA/fallible/zkir/count.bzkir"     fallible.bzkir
cp "$TA/fallible/keys/count.prover"    fallible.prover
cp "$TA/fallible/keys/count.verifier"  fallible.verifier
```

These are checked in (small enough — ~24 KB total) so the prover-core
crate works on any host without invoking Nix at runtime, and so the
Dioxus app can bundle them as Android assets.
