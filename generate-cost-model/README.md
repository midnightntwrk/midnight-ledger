# Cost Model

Crate for cost-model related tools. Currently only includes `vm-cost-model` binary. Remaining work includes consuming the learned cost models to create a `onchain_vm::cost_model::CostModel` struct.

## `vm-cost-model` commandline tool

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
