# Ledger API integration tests


`integration-tests` module contains Ledger API integration tests.
Tests are written in Typescript and use [Vitest](https://vitest.dev/) framework.
They are testing the Ledger/ZSwap WASM libraries generated in other modules of this project.

## Requirements

Besides Nodejs 22+ and Yarn, which are automatically provided by nix dev shell, the other requirements may require manual setup. Details follow.

### Proof server

Tests use a Proof Server (binary or docker image). By default tests use the Proof Server located at `result/bin/midnight-proof-server`, which is provided by nix (run `nix build`).

Env vars affecting proof server:

- `MN_AXIOS` - use Axios to call Proof Server, default is using `midnight-js` fetch
- `MN_PROOF_SERVER_DOCKER` - use docker image defined in `proof-server.yml` of proof server instead of binary

### Compiled `ledger` and `zswap` libraries

Build the `@midnight-ntwrk/ledger`, `@midnight-ntwrk/zswap` libraries using Nix (_from the project root_):

```console
$ nix build
```

As a result `result/lib/node_modules/@midnight-ntwrk` directory contains built library that is tested by this module.

## Run tests

```console
$ cd integration-tests
$ yarn clean && yarn install && yarn build && yarn test
```

or you can use the `./run_it.sh` script in the project root directory.

## Run tests and generate coverage report

```shell
cd integration-tests
yarn clean && yarn install && yarn build && yarn test:coverage:prepare && yarn test:coverage
```

## Explanation

As a workaround to gather the metrics libraries source code is being copied to `lib-sources` directory by
`test:coverage:prepare` script defined in [package.json](./integration-tests/package.json).
The `yarn test:coverage` then runs the tests with code coverage.

## Results

After test execution all reports available are:

- console
- [reports](./integration-tests/reports) directory:
  - [junit](./integration-tests/reports/test-report.xml)
  - [html](./integration-tests/reports/test-report.html)
- [coverage](./integration-tests/coverage) metrics
