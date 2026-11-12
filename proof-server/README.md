# Proof server

`midnight-proof-server` is an HTTP server that generates zero-knowledge proofs
for Midnight transactions. Clients send it proof preimages or unproven
transactions, together with the key material for their circuits, and receive
proofs or proven transactions back.

## Running

### Docker

Release images are published as `midnightntwrk/proof-server:<version>` on
Docker Hub and `ghcr.io/midnight-ntwrk/proof-server:<version>`, for `amd64`
and `arm64`.

```bash
docker run -p 127.0.0.1:6300:6300 \
  -v midnight-zk-params:/params -e MIDNIGHT_PP=/params \
  midnightntwrk/proof-server:<version>
```

The image starts the server with `--port $PORT` (default `6300`), so set
`PORT` to change the port; the other options below are passed as their
environment variables with `-e`. Mounting a volume at `MIDNIGHT_PP` keeps
downloaded parameters across restarts.

### Nix

```bash
nix run .#proof-server -- --port 6300
```

`nix build .#proof-server` builds the binary into `result/bin/`, and
`nix build .#proof-server-oci` builds the Docker image on a Linux host.

### Cargo

```bash
cargo run --release -p midnight-proof-server -- --port 6300
```

## Options

Every option can be set on the command line or through its environment
variable.

| Option              | Environment variable                    | Default   | Meaning                                                                                    |
|---------------------|-----------------------------------------|-----------|--------------------------------------------------------------------------------------------|
| `-p`, `--port`      | `MIDNIGHT_PROOF_SERVER_PORT`            | `6300`    | Port to listen on, on all interfaces.                                                      |
| `-v`, `--verbose`   | `MIDNIGHT_PROOF_SERVER_VERBOSE`         | off       | Log at debug level, including request bodies.                                              |
| `--num-workers`     | `MIDNIGHT_PROOF_SERVER_NUM_WORKERS`     | `2`       | Jobs proven concurrently.                                                                  |
| `--job-capacity`    | `MIDNIGHT_PROOF_SERVER_JOB_CAPACITY`    | `10`      | Jobs that may wait in the queue; further requests get HTTP 429. `0` means unlimited.       |
| `--job-timeout`     | `MIDNIGHT_PROOF_SERVER_JOB_TIMEOUT`     | `600`     | Seconds a job is kept; a request whose job outlives this fails.                            |
| `--no-fetch-params` | `MIDNIGHT_PROOF_SERVER_NO_FETCH_PARAMS` | off       | Skip downloading parameters and built-in keys at startup, and disable `/fetch-params/{k}`. |
| `--max-k`           | `MIDNIGHT_PROOF_SERVER_MAX_K`           | unlimited | Reject circuits larger than this `k` (1–25). See [Memory](#memory).                        |

## Public parameters and keys

Proving a circuit of size `k` needs the public parameters for `2^k` rows. At
startup the server downloads the parameters for `k` 10 to 15 and the built-in
Zswap and Dust keys, unless `--no-fetch-params` is set. Anything else is
downloaded the first time a request needs it.

Files are stored in `$MIDNIGHT_PP` if set, otherwise in
`$XDG_CACHE_HOME/midnight/zk-params` or `~/.cache/midnight/zk-params`, and are
downloaded from `https://srs.midnight.network/` unless `MIDNIGHT_PARAM_SOURCE`
names another URL. Downloads are checked against known hashes.

## Memory

Proving memory grows with `2^k`, so the largest circuit a server is asked to
prove determines how much memory it needs. As a rough guide, proving a single
job peaked at about 5 GiB for a `k` = 18 circuit and 12 GiB for `k` = 20, and
concurrent jobs add up. A server that runs out of memory mid-proof is killed
by the operating system, failing every job it is running.

Set `--max-k` to the largest `k` the host can prove within its memory, given
`--num-workers`. Requests for larger circuits are then rejected with HTTP 400
before they are queued. In Docker, the container's memory limit and, on macOS
and Windows, Docker Desktop's VM memory both bound what is available.

## Endpoints

Request and response bodies are binary, serialized with `midnight-serialize`.
The `@midnight-ntwrk/ledger-v9` package builds them: see
`createProvingPayload`, `createProvingTransactionPayload`,
`createCheckPayload` and `parseCheckResult` in [the API docs](../docs/api/ledger/README.md).

| Method and path         | Purpose                                                                                                                                   |
|-------------------------|-------------------------------------------------------------------------------------------------------------------------------------------|
| `POST /prove`           | Prove one proof preimage, with optional key material and binding input. Returns the proof.                                                |
| `POST /prove-tx`        | Prove every proof in an unproven transaction, given key material by circuit name. Returns the proven transaction. Superseded by `/prove`. |
| `POST /check`           | Run a preimage through its circuit without proving, and return which public inputs it skips.                                              |
| `POST /k`               | Return the `k` of a ZKIR source.                                                                                                          |
| `GET /fetch-params/{k}` | Download the parameters for `k` ahead of use. Not available with `--no-fetch-params`.                                                     |
| `GET /health`, `GET /`  | Liveness check.                                                                                                                           |
| `GET /ready`            | Queue status as JSON; HTTP 503 when the job queue is full.                                                                                |
| `GET /version`          | Server version.                                                                                                                           |
| `GET /proof-versions`   | Proof versions the server can produce.                                                                                                    |

Malformed requests and circuits above `--max-k` return HTTP 400; a full queue
returns HTTP 429.
