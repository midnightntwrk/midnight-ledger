# Agda development — toolchain and type-checking

This directory holds the Agda code ([zkir-v3/](zkir-v3/)) and the Nix flake
providing the toolchain (the same one CI uses, via
[.github/workflows/agda.yml](../../.github/workflows/agda.yml)).

Everything runs from this directory via the flake:

```sh
cd formal/src
nix run .#agda -- zkir-v3/Main.agda    # type-checks the entire zkir-v3 development
```

Inside a nix shell (or with a suitable Agda + standard-library setup) the
bare invocation works from the repository root:

```sh
agda --safe -i formal/src formal/src/zkir-v3/Main.agda
```

`zkir-v3/Main.agda` imports every module, so checking it verifies the whole
development. `CircuitProof.agda` is large — a cold check can take several
minutes. Dependencies are declared in
[zkir-formal-spec.agda-lib](zkir-formal-spec.agda-lib)
(`standard-library`, `standard-library-classes`, `standard-library-meta`).
