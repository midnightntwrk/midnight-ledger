# Cost Model

Crate for cost-model related tools. This is primarily a rust tool for
interpreting criterion results, and a docker script for running the benchmarks
and gathering these results.

# Gathering benchmarks through docker

Use the following script to perform a new benchmark run:

1. In a fresh checkout, at the tag you want to benchmark: `generate-cost-model/build-docker.sh` (locally)
2. Create the raw docker image: `docker image save ghcr.io/midnight-ntwrk/generate-ledger-cost-model:latest -o cost-model.tar.gz`
3. Transfer `cost-model.tar.gz` to benchmarking machine. Prerequisites as of time of writing:
   1. Connected to AWS VPN
   2. StrongDM access and locally connected
   3. SRE have spun up a dedicated machine for access
   4. Once done, access should be possible via `ssh` and `scp` as normal.
4. On remote: `sudo yum install tmux docker && sudo service docker start && sudo usermod -a -G docker $USER`
5. Disconnect and reconnect from remote to trigger new user group permissions
6. On remote: `mkdir ledger-cost-model-output && chmod ugo+rwx ledger-cost-model-output`
7. On remote: import `cost-model.tar.gz`: `docker load -i cost-model.tar.gz`
8. On remote: remove `cost-model.tar.gz`
9. On remote: Open `tmux` shell
10. On remote \[`tmux`\]: `docker run --mount type=bind,src=$PWD/ledger-cost-model-output,dst=/cost-model/output ghcr.io/midnight-ntwrk/generate-ledger-cost-model:latest`
11. Disconnect from `tmux` (`Ctrl-B Ctrl-D`)
12. On remote, monitor until completion via `tmux attach -r` (read-only to avoid killing). Note that during run AWS VPN is likely to boot you out at some point.
13. If the above fails, it's likely that the benchmarks need adjusting. This isn't too surprising, as the benchmarks are heavy, rarely run, and usually major system changes occur between benchmark runs.
14. Copy back results to local (`scp -r` on `ledger-cost-model-output`)
15. Rename the results and compress them to a `.tar.gz` in `ledger/params`.
16. To generate `.json` and `.bin` files for the resulting parameters, copy the contents of `ledger-cost-model-output/const_declaration.rs` into the corresponding spot in `ledger/examples/output-params.rs`, and run `cargo run --example output-params <identifier>`. Open a PR to include this, and the result `.tar.gz` in-repo for reference.

# `vm-cost-model` commandline tool

The `vm-cost-model` binary analyzes Criterion benchmark results for VM operations and uses linear regression to generate cost models that predict execution time based on operation parameters. The regression learning supports zero or more model parameters. The tool optionally generates plots, but this only supports 0, 1, or 2 parameters (the 2 param case is handled by slicing the data by fixing values of one of the parameters).

Here the "parameters" correspond to the parameters the VM will use to calculate the gas cost during execution. For example, the `rem` operation has two parameters: `key_size` and `container_log_size`, and the model we learn is
`(c_0, c_1, c_2)` s.t.

```
predicted_time = c_0 + c_1 * key_size + c_2 * container_log_size
```

is the best fit to the observed data. The VM op benchmarks save the parameter values along with the measured run time.

Features:
- **Multi-parameter operations**: Uses linear regression to model time as a function of parameters
- **Parameter-less operations**: Uses mean-based modeling with custom fit metrics
- **Mixed container types**: Generates both per-container and combined models, in case of operations that support multiple container types (e.g. the `new` opcode can create a container of any type)
- **Quality assessment**: Provides R² and adjusted R² metrics for model validation

## Usage

### Basic Analysis

First, you need to run the benchmarks to generate the data. See `:/onchain-runtime/benches/benchmarking.rs` for details.

To analyze benchmark results for a specific operation:

```bash
cargo run --bin vm-cost-model <criterion_dir> <operation>
```

For example, to cost-model the `member` operation, when the benchmark results are in the default location `target/criterion/`:
```bash
cargo run --bin vm-cost-model target/criterion member
```

### With Plotting

To generate visualization plots along with the analysis:

```bash
cargo run --bin vm-cost-model <criterion_dir> <operation> --plot
```

## Output

### JSON Results

Results are written to `tmp/cost-model/json/<operation>_cost_model.json` containing:

- **Regression models**: Linear cost models for each container type and combined. The format of these models is e.g.
  ```json
  {
    "constant": 1234.5,
    "param_coeffs": {
      "key_size": 12.3,
      "container_log_size": 456.7
    },
    "r_squared": 0.95,
    "adjusted_r_squared": 0.94,
    "mean_fit": 0.01
  }
  ```
  Where the regression formula is: `predicted_time = constant + (key_size * 12.3) + (container_log_size * 456.7)`

- **Model quality metrics**: R², adjusted R², and mean fit (1 - variance/sum-of-value-squares)
- **Data points**: Original benchmark data with predicted times and residuals

### Plots (with --plot flag)

HTML plots are generated in `tmp/cost-model/plot/` showing:

- **0 parameters**: 1D plot of measured times vs predicted time (mean)
- **1 parameter**: 2D plot of value vs execution time with regression line of predicted time
- **2 parameters**: Multiple 2D slices fixing one parameter at different values, with the regression line of predicted time

## Input Format

The tool expects Criterion benchmark results in the standard format:

```
<criterion_dir>/<operation>/
  └── <benchmark_variant>/
      └── new/
          ├── benchmark.json     # Contains operation parameters
          └── estimates.json     # Contains timing measurements
```

The `benchmark.json` should have a `function_id` field containing JSON with:
- `container_type`: String identifying the container type ("none" if no container)
- Parameter values matching the operation's schema (e.g., `key_size`, `container_log_size`)

The `BENCHMARK_SCHEMAS` map in `src/vm-cost-model.rs` defines the expected parameters for each VM operation.

Note: these .json files are [not part of the public API](https://bheisler.github.io/criterion.rs/book/user_guide/csv_output.html) of Criterion, but I assume their format rarely changes, since Criterion is pretty mature.
