// This file is part of midnight-ledger.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License");
// You may not use this file except in compliance with the License.
// You may obtain a copy of the License at
// http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! # VM Cost Model
//!
//! A tool for analyzing VM operation benchmark results and generating cost
//! models using linear regression. See `../README.md`.
//!
//! ## Architecture
//!
//! The tool is organized into three main phases:
//!
//! 1. Parsing Phase: parse time measurements and input parameters from benchmark JSON files.
//! 2. Regression Phase: learn linear models of measured times using linear regression.
//! 3. Optional Plotting Phase: plot measured data along with learned predictions.

#[cfg(feature = "svg")]
use charming::ImageRenderer;
use charming::{
    Chart, HtmlRenderer,
    component::{Axis, Legend, Title},
    element::{ItemStyle, LineStyle, NameLocation, Symbol},
    series::{Line, Scatter},
};
use clap::Parser;
use indexmap::IndexMap;
use itertools::Itertools;
use linregress::{FormulaRegressionBuilder, RegressionDataBuilder};
use serde::Serialize;
use serde_json::{Value, from_reader};
use std::{
    collections::{HashMap, HashSet},
    error::Error,
    fs::{self, File},
    io::{BufReader, Write},
    path::{Path, PathBuf},
    sync::LazyLock,
};

// e.g. ["key_size", "log_container_size"]
type ModelParams = Vec<String>;

/// Description of benchmark parameters, to help with parsing benchmark JSON files.
struct BenchmarkSchema {
    /// Compute model parameters based on container type.
    ///
    /// The "all" container as input is a sort of default and is used in
    /// "combined" container contexts (combined modeling, combined plotting, and
    /// plot restriction parsing). The motivation for making this a function is
    /// to allow special casing for the array container, and we just treat the
    /// other container types uniformly in practice.
    model_params: Box<dyn Fn(&str) -> ModelParams + Sync + Send>,
}

/// Top-level map of operations to their benchmark schemas
static BENCHMARK_SCHEMAS: LazyLock<IndexMap<&'static str, BenchmarkSchema>> = LazyLock::new(|| {
    // Compute BenchmarkSchema from a list of parameter names, special casing
    // some params for array models.
    let schema = |params: &[&str]| {
        let strings: ModelParams = params.iter().map(|s| s.to_string()).collect();
        let strings_no_key_or_container_size: ModelParams = strings
            .iter()
            .filter(|&s| !["key_size", "container_log_size"].contains(&s.as_str()))
            .cloned()
            .collect();
        BenchmarkSchema {
            model_params: Box::new(move |container| match container {
                // For arrays, don't use key size or container size, since we end up with too many
                // model params and can learn degenerate models due to under determined params.
                "array" => strings_no_key_or_container_size.clone(),
                _ => strings.clone(),
            }),
        }
    };
    IndexMap::from([
        // Vm ops, ordered same as in benchmarking.rs and vm.rs
        ("noop", schema(&["arg"])),
        ("branch", schema(&["arg"])),
        ("jmp", schema(&["arg"])),
        ("ckpt", schema(&[])),
        ("lt", schema(&[])),
        ("eq", schema(&[])),
        ("type", schema(&[])),
        ("size", schema(&[])),
        ("new", schema(&[])),
        ("and", schema(&[])),
        ("or", schema(&[])),
        ("neg", schema(&[])),
        // We expect log op cost actually depend on value size, even tho it
        // doesn't at the moment
        ("log", schema(&["value_size"])),
        ("root", schema(&[])),
        ("pop", schema(&[])),
        ("popeq", schema(&["value_size"])),
        ("popeqc", schema(&["value_size"])),
        ("addi", schema(&[])),
        ("subi", schema(&[])),
        ("push", schema(&[])),
        ("pushs", schema(&[])),
        ("add", schema(&[])),
        ("sub", schema(&[])),
        ("concat", schema(&["total_size"])),
        ("concatc", schema(&["total_size"])),
        ("member", schema(&["key_size", "container_log_size"])),
        ("rem", schema(&["key_size", "container_log_size"])),
        ("remc", schema(&["key_size", "container_log_size"])),
        ("dup", schema(&["arg"])),
        ("swap", schema(&["arg"])),
        ("idx", schema(&["key_size", "container_log_size"])),
        ("idxc", schema(&["key_size", "container_log_size"])),
        ("idxp", schema(&["key_size", "container_log_size"])),
        ("idxpc", schema(&["key_size", "container_log_size"])),
        ("ins", schema(&["key_size", "container_log_size"])),
        ("insc", schema(&["key_size", "container_log_size"])),
        // Crypto benchmarks from transient-crypto/benches/benchmarking.rs
        ("transient_hash", schema(&[])),
        ("hash_to_curve", schema(&[])),
        ("ec_add", schema(&[])),
        ("ec_mul", schema(&[])),
        ("proof_verify", schema(&["size"])),
        ("verifier_key_load", schema(&[])),
        ("fr_add", schema(&[])),
        ("fr_mul", schema(&[])),
        ("pedersen_valid", schema(&[])),
        ("signature_verify", schema(&["size"])),
        // Storage delta tracking benchmarks from storage/benches/benchmarking.rs
        ("get_writes", schema(&["keys_added_size"])),
        ("update_rcmap", schema(&["keys_added_size"])),
        ("gc_rcmap", schema(&["keys_removed_size"])),
    ])
});

/// Combination of benchmark inputs and measured time
#[derive(Debug, Clone, Serialize)]
struct BenchmarkDataPoint {
    container_type: String,
    model_values: HashMap<String, usize>,
    measured_time: f64,
    /// Whether the operation crashed
    crashed: Option<bool>,
    /// Whether the key was present in the container (only applies to a few
    /// operations, including ins, rem, member)
    key_present: Option<bool>,
}

/// Results of cost modeling for a VM op.
#[derive(Debug, Clone, Serialize)]
struct OpModel {
    op: String,
    /// Regressions restricted to each container type
    per_container_type: HashMap<String, RegressionAndModeledData>,
    /// Regression for data combined across all container types
    combined: RegressionAndModeledData,
}

/// A cost model, learned by linear regression.
///
/// The learned model for `d: BenchmarkDataPoint` is
///
/// ```text
/// modeled_time = self.constant
///              + self.params_coeffs[p1] * d.model_values[p1] + ...
///              + self.params_coeffs[pn] * d.model_values[pn]
/// ```
///
/// where `p1` ... `pn` are the `BenchmarkSchema.model_params`.
#[derive(Debug, Clone, Serialize)]
struct Regression {
    /// Constant term of regression formula
    constant: f64,
    /// Linear coefficients: map from param-name to its linear coefficient in
    /// the regression formula.
    param_coeffs: HashMap<String, f64>,
    /// Standard quality metric
    r_squared: f64,
    /// Standard quality metric
    adjusted_r_squared: f64,
    /// Ad-hoc quality metric, measuring how good the mean is as a model
    mean_fit: f64,
}

/// Combined regression model and associated data points for validation and plotting.
#[derive(Debug, Clone, Serialize)]
struct RegressionAndModeledData {
    regression: Regression,
    /// Data points used to derive the regression, along with timing predicted
    /// by model, sorted by measured time.
    data: Vec<ModeledDataPoint>,
}

/// A benchmark data point along with its predicted time and residual (modeled
/// time - measured time).
#[derive(Debug, Clone, Serialize)]
struct ModeledDataPoint {
    benchmark: BenchmarkDataPoint,
    predicted_time: f64,
    residual: f64,
}

/// Define command line arguments using clap
#[derive(Parser, Debug)]
#[command(
    name = "vm-cost-model",
    about = "Analyze benchmark results and create cost models",
    version
)]
struct Args {
    /// Path to the criterion directory containing benchmark results
    #[arg(
        help = "Path to criterion directory containing benchmark results, e.g. target/criterion"
    )]
    criterion_dir: PathBuf,

    /// Operation name to analyze
    #[arg(help = "Operation name to analyze, or \"all\" to analyze all operations")]
    op: String,

    /// The output directory, created if it does not exist
    #[arg(default_value = "tmp/cost-model")]
    output_dir: PathBuf,

    /// Generate plots with optional parameter slices
    #[arg(long, value_name = "PARAM=VALUE,...", num_args = 0..=1, default_missing_value = "",
          help = "Generate plots. For 2D models: automatic min/mid/max slicing by default, or specify PARAM=VALUE,... for manual slices. Enable \"svg\" feature to get svg plots in addition to default html plots.")]
    plot: Option<String>,

    /// Enable verbose output for data point collection
    #[arg(short, long, help = "Enable verbose progress output")]
    verbose: bool,

    /// Output the cost model rust constant declaration file output
    #[arg(
        long,
        help = "Enable rust constant declaration file output (only applicable to 'all' op setting)"
    )]
    output_const: bool,
}

/// Parsed parameter restrictions for plotting
#[derive(Debug, Clone)]
enum PlotRestrictions {
    Auto,
    Manual(HashMap<String, Vec<usize>>),
}

/// Debug print `t` with indentation of `n` spaces.
///
/// Utility function for pretty-printing debug output with consistent indentation.
fn dbg_indent<T: std::fmt::Debug>(n: usize, t: &T) {
    let indent = " ".repeat(n);
    println!(
        "{}",
        format!("{indent}{t:#?}").replace('\n', &format!("\n{indent}"))
    );
}

/// Parse plot restrictions from command line argument.
///
/// Parses strings like `"key_size=32,key_size=64,container_log_size=10"` into `HashMap`ap.
/// Empty string triggers automatic min/mid/max slicing in 2D models.
fn parse_plot_restrictions(
    plot_arg: &str,
    model_params: &ModelParams,
) -> Result<PlotRestrictions, Box<dyn Error>> {
    let trimmed = plot_arg.trim();
    if trimmed.is_empty() {
        return Ok(PlotRestrictions::Auto);
    }

    let mut restrictions: HashMap<String, Vec<usize>> = HashMap::new();
    for pair in trimmed.split(',') {
        if pair.is_empty() {
            continue;
        }

        let parts: Vec<&str> = pair.split('=').collect();
        if parts.len() != 2 {
            return Err(format!(
                "Invalid plot restriction format: '{}'. Expected 'param=value'",
                pair
            )
            .into());
        }
        let param_name = parts[0];
        let value_str = parts[1];
        if !model_params.contains(&param_name.to_string()) {
            return Err(format!(
                "Unknown parameter '{}'. Valid parameters: [{}]",
                param_name,
                model_params.join(", ")
            )
            .into());
        }
        let value = value_str.parse::<usize>().map_err(|_| {
            format!(
                "Invalid value '{}' for parameter '{}'. Expected usize",
                value_str, param_name
            )
        })?;
        restrictions
            .entry(param_name.to_string())
            .or_default()
            .push(value);
    }

    Ok(PlotRestrictions::Manual(restrictions))
}

fn main() -> Result<(), Box<dyn Error>> {
    let result = run_main();
    match result {
        Ok(()) => Ok(()),
        Err(e) => {
            // Interpret escapes in error message :P
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}

fn run_main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();

    let json_dir = args.output_dir.join("json");
    std::fs::create_dir_all(&json_dir)?;

    if args.op == "all" {
        println!("Analyzing benchmarks for all operations");
        let mut all_models = HashMap::new();
        let mut failed_ops = Vec::new();
        let mut successful_results = Vec::new();

        let plot = match args.plot {
            Some(ref plot_arg) => {
                if plot_arg.is_empty() {
                    true
                } else {
                    return Err("Manual plot restrictions not allowed in 'auto' mode.".into());
                }
            }
            None => false,
        };

        // Learn models
        for op_name in BENCHMARK_SCHEMAS.keys() {
            println!("Processing: {}", op_name);
            match learn_model(op_name, &args.criterion_dir, &json_dir, args.verbose) {
                Ok(op_model) => {
                    all_models.insert(op_name.to_string(), op_model.clone());
                    successful_results.push((op_name.to_string(), op_model.clone()));
                    if plot {
                        let schema = BENCHMARK_SCHEMAS.get(op_name).unwrap();
                        generate_plots(
                            op_name,
                            &op_model,
                            schema,
                            &PlotRestrictions::Auto,
                            &args.output_dir,
                            args.verbose,
                        )?;
                    }
                }
                Err(e) => {
                    println!("- FAILED: {}", e);
                    failed_ops.push(op_name.to_string());
                }
            }
        }
        if plot {
            println!("Plots written to tmp/cost-model/plot/*.{{svg,html}}");
        }

        // Write combined results. Individual operation files are written by
        // `learn_model`.
        let combined_output_file = json_dir.join("all_operations_cost_model.json");
        let json = serde_json::to_string_pretty(&all_models)?;
        fs::write(&combined_output_file, json)?;
        println!(
            "Combined model for all ops written to: {}",
            combined_output_file.display()
        );

        if args.output_const {
            let mut f = File::create(args.output_dir.join("const_declaration.rs"))?;
            writeln!(
                f,
                "pub const INITIAL_COST_MODEL: CostModel = CostModel {{\n    read_time_batched_4k: BATCHED_4K_READ_TIME,\n    read_time_synchronous_4k: SYNCHRONOUS_4K_READ_TIME,"
            )?;
            let ps = |ns: f64| (ns * 1000f64).ceil().max(0f64) as u64;
            for (op_name, op_model) in all_models.iter() {
                if ["fr_mul", "fr_add"].contains(&op_name.as_str()) {
                    continue;
                }
                let use_combined = ["pop", "dup", "swap"].contains(&op_name.as_str());
                let containers: Box<dyn Iterator<Item = _>> = if use_combined {
                    Box::new(std::iter::once(("".to_owned(), op_model.combined.clone())))
                } else {
                    Box::new(
                        op_model
                            .per_container_type
                            .iter()
                            .map(|(a, b)| (a.clone(), b.clone())),
                    )
                };
                for (container_name, container_regression) in containers {
                    let container_fragment =
                        if op_model.per_container_type.len() == 1 || use_combined {
                            "".to_owned()
                        } else {
                            format!("_{container_name}")
                        };
                    writeln!(
                        f,
                        "    {op_name}{container_fragment}{}: CostDuration::from_picoseconds({}),",
                        if container_regression.regression.param_coeffs.is_empty() {
                            ""
                        } else {
                            "_constant"
                        },
                        ps(container_regression.regression.constant)
                    )?;
                    for (coeff_name, coeff_value) in
                        container_regression.regression.param_coeffs.iter()
                    {
                        writeln!(
                            f,
                            "    {op_name}{container_fragment}_coeff_{coeff_name}: CostDuration::from_picoseconds({}),",
                            ps(*coeff_value)
                        )?;
                    }
                }
            }
            writeln!(f, "}};")?;
        }

        // Print summary
        print_quality_table(&successful_results);
        let failed_ops: &[String] = &failed_ops;
        if !failed_ops.is_empty() {
            println!("\n=== FAILED OPERATIONS ===");
            println!("Failed to process {} operations:", failed_ops.len());
            for op in failed_ops {
                println!("- {op}");
            }
        }
    } else {
        // Single operation mode
        let op_model = learn_model(&args.op, &args.criterion_dir, &json_dir, args.verbose)?;
        if let Some(plot_arg) = args.plot {
            let schema = BENCHMARK_SCHEMAS.get(args.op.as_str()).unwrap();
            let restrictions = parse_plot_restrictions(&plot_arg, &(schema.model_params)("all"))?;
            generate_plots(
                &args.op,
                &op_model,
                schema,
                &restrictions,
                &args.output_dir,
                args.verbose,
            )?;
            println!("Plots written to tmp/cost-model/plot/*.{{svg,html}}");
        }
        print_quality_table(&[(args.op, op_model.clone())]);
    }

    Ok(())
}

/// Learn cost model for a single operation.
///
/// Writes JSON file for op and returns learned model.
fn learn_model(
    op_name: &str,
    criterion_dir: &Path,
    output_dir: &Path,
    verbose: bool,
) -> Result<OpModel, Box<dyn Error>> {
    println!("Analyzing benchmarks for operation: {}", op_name);
    let schema = BENCHMARK_SCHEMAS.get(op_name).ok_or(format!(
        "No schema defined for op '{}'. Please add it to BENCHMARK_SCHEMAS.",
        op_name
    ))?;

    let criterion_dir = criterion_dir.join(op_name);
    if !criterion_dir.exists() {
        return Err(format!("Criterion directory not found: {:?}", criterion_dir).into());
    }

    let data_points = collect_data_points(&criterion_dir, schema, verbose)?;
    println!("Collected {} data points", data_points.len());
    if data_points.is_empty() {
        return Err("No data points found for regression".into());
    }

    let op_model = compute_op_model(op_name, &data_points, schema, verbose)?;

    // Write individual JSON file
    let output_file = output_dir.join(format!("{}_cost_model.json", op_name));
    let json = serde_json::to_string_pretty(&op_model)?;
    fs::write(&output_file, json)?;
    println!(
        "Model for {op_name} op written to: {}",
        output_file.display()
    );

    Ok(op_model)
}

/// Format (adj) `R^2` value, showing "N/A" for zero-parameter models.
///
/// See discussion at `run_zero_dimensional_regression`.
fn format_r_squared(value: f64, has_model_params: bool) -> String {
    if has_model_params {
        format!("{:>5.2}", value)
    } else {
        "  N/A".to_string()
    }
}

/// Check if a model has poor quality fit
fn bad_fit_flag(regression: &Regression, _has_model_params: bool) -> &'static str {
    if regression.r_squared < 0.5
        && regression.adjusted_r_squared < 0.5
        && regression.mean_fit < 0.9
    {
        " ?BAD FIT?"
    } else {
        ""
    }
}

fn print_quality_table(successful_results: &[(String, OpModel)]) {
    println!("\n=== MODEL QUALITY SUMMARY ===");

    // Header
    println!("Operation     Data Points    R²    Adj R²  Mean Fit");
    println!("===================================================");

    for (op_name, op_model) in successful_results {
        // Check if this operation has model parameters
        let schema = BENCHMARK_SCHEMAS.get(op_name.as_str()).unwrap();
        let has_model_params = !(schema.model_params)("all").is_empty();

        // Print combined model first
        let combined = &op_model.combined.regression;
        let combined_data_count = &op_model.combined.data.len();

        println!(
            "{:<19} {:>5} {:>5} {:>9} {:>9.2}",
            op_name,
            combined_data_count,
            format_r_squared(combined.r_squared, has_model_params),
            format_r_squared(combined.adjusted_r_squared, has_model_params),
            combined.mean_fit,
        );

        // Print per-container models (sorted by container name)
        let mut container_entries: Vec<_> = op_model.per_container_type.iter().collect();
        container_entries.sort_by_key(|(container_type, _)| *container_type);

        for (container_type, container_model) in container_entries {
            let container_data_count = container_model.data.len();
            let container_regression = &container_model.regression;

            println!(
                "- {:<17} {:>5} {:>5} {:>9} {:>9.2}{}",
                container_type,
                container_data_count,
                format_r_squared(container_regression.r_squared, has_model_params),
                format_r_squared(container_regression.adjusted_r_squared, has_model_params),
                container_regression.mean_fit,
                bad_fit_flag(container_regression, has_model_params)
            );
        }

        println!("---------------------------------------------------");
    }
}

/// Collect benchmark data points from Criterion output files.
///
/// This function recursively searches the criterion directory for benchmark results,
/// parsing both `benchmark.json` (for parameters) and `estimates.json` (for timing data).
///
/// # Returns
/// Vector of parsed benchmark data points, each containing parameters and measured time
///
/// # File Structure Expected
/// ```text
/// criterion_dir/
///   └── <benchmark_variant>/
///       └── new/
///           ├── benchmark.json     # Contains operation parameters
///           └── estimates.json     # Contains timing measurements
/// ```
fn collect_data_points(
    criterion_dir: &Path,
    schema: &BenchmarkSchema,
    verbose: bool,
) -> Result<Vec<BenchmarkDataPoint>, Box<dyn Error>> {
    let mut data_points = Vec::new();
    // Recursively find all benchmark.json and estimates.json files
    for entry in fs::read_dir(criterion_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if verbose {
            println!("Checking directory: {:?}", path);
        }
        let new_dir = path.join("new");
        if !new_dir.exists() || !new_dir.is_dir() {
            continue;
        }
        let benchmark_file = new_dir.join("benchmark.json");
        if !benchmark_file.exists() {
            if verbose {
                println!("  benchmark.json not found, skipping");
            }
            continue;
        }
        let estimates_file = new_dir.join("estimates.json");
        if !estimates_file.exists() {
            if verbose {
                println!("  estimates.json not found, skipping");
            }
            continue;
        }
        if verbose {
            println!("  Found benchmark files, parsing...");
        }
        let measured_time = parse_time_estimate(&estimates_file)?;
        let data_point = parse_benchmark(&benchmark_file, schema, measured_time, verbose)?;
        if verbose {
            println!("  Successfully parsed benchmark: {data_point:?}");
        }
        data_points.push(data_point);
    }
    Ok(data_points)
}

/// Parse benchmark parameters from a Criterion `benchmark.json` file.
///
/// Extracts operation parameters and container type from the JSON-encoded `function_id` field.
/// The `function_id` contains a JSON string with the benchmark configuration.
///
/// # Arguments
/// * `measured_time` - Pre-parsed timing measurement to include in the result
///
/// # Example expected JSON Format
/// ```json
/// {
///   "function_id": "{\"container_type\":\"bmt\",\"key_size\":32,\"container_log_size\":10}"
/// }
/// ```
fn parse_benchmark(
    file_path: &Path,
    schema: &BenchmarkSchema,
    measured_time: f64,
    verbose: bool,
) -> Result<BenchmarkDataPoint, Box<dyn Error>> {
    let file = File::open(file_path)?;
    let reader = BufReader::new(file);
    let json: Value = from_reader(reader)?;
    // The function_id field contains the JSON string with parameters.
    let function_id = json["function_id"]
        .as_str()
        .ok_or("function_id not found or not a string")?;
    if verbose {
        println!("  Raw function_id: {}", function_id);
    }
    let params_json: Value = serde_json::from_str(function_id)?;
    let mut model_values = HashMap::new();
    for key in &(schema.model_params)("all") {
        let val = params_json[key]
            .as_u64()
            .ok_or(format!("model param '{}' missing or not a u64", key))?
            as usize;
        model_values.insert(key.clone(), val);
    }
    let container_type = params_json["container_type"]
        .as_str()
        .ok_or("container_type field missing or not a string")?
        .to_string();

    // Extract decoration parameters if present; these are not used for model learning
    let crashed = params_json
        .get("crashed")
        .and_then(|v| v.as_u64())
        .map(|v| v != 0);
    let key_present = params_json
        .get("key_present")
        .and_then(|v| v.as_u64())
        .map(|v| v != 0);

    Ok(BenchmarkDataPoint {
        container_type,
        model_values,
        measured_time,
        crashed,
        key_present,
    })
}

/// Parse timing estimate from a Criterion `estimates.json` file.
///
/// # Expected JSON Format
/// ```json
/// {
///   "mean": {
///     "point_estimate": 1234.5 # nanoseconds
///   }
/// }
/// ```
fn parse_time_estimate(file_path: &Path) -> Result<f64, Box<dyn Error>> {
    let file = File::open(file_path)?;
    let reader = BufReader::new(file);
    let json: Value = from_reader(reader)?;
    let time = json["mean"]["point_estimate"]
        .as_f64()
        .ok_or("mean.point_estimate not found or not a number")?;
    Ok(time)
}

/// Perform linear regression analysis on collected benchmark data.
fn compute_op_model(
    op: &str,
    data_points: &[BenchmarkDataPoint],
    schema: &BenchmarkSchema,
    verbose: bool,
) -> Result<OpModel, Box<dyn Error>> {
    if verbose {
        println!("Preparing data for regression:");
    }
    let container_types: HashSet<String> = data_points
        .iter()
        .map(|p| p.container_type.clone())
        .collect();
    if verbose {
        println!("Found {} unique container types", container_types.len());
    }
    let mut per_container_type = HashMap::new();
    // Sort container types for deterministic iteration order
    let mut sorted_container_types: Vec<String> = container_types.into_iter().collect();
    sorted_container_types.sort();
    for container_type in &sorted_container_types {
        if verbose {
            println!(
                "Running regression for container type: {:?}",
                container_type
            );
        }
        let filtered_points: Vec<_> = data_points
            .iter()
            .filter(|p| &p.container_type == container_type)
            .collect();
        if verbose {
            println!(
                "  {} data points for this container type",
                filtered_points.len()
            );
        }
        let modeled_data = run_regression(
            &filtered_points,
            &(schema.model_params)(container_type),
            verbose,
        )?;
        per_container_type.insert(container_type.clone(), modeled_data);
    }
    if verbose {
        println!("Running combined regression across all container types");
    }
    let all_points: Vec<_> = data_points.iter().collect();
    let combined = run_regression(&all_points, &(schema.model_params)("all"), verbose)?;
    let result = OpModel {
        op: op.to_string(),
        per_container_type,
        combined,
    };
    Ok(result)
}

/// Compute regression for a filtered data set.
///
/// Handles both multi-parameter operations (using linear regression) and
/// parameter-less operations (using mean-based modeling). Returns both the
/// regression model and the modeled data points sorted by measured time.
fn run_regression(
    data: &[&BenchmarkDataPoint],
    model_params: &ModelParams,
    verbose: bool,
) -> Result<RegressionAndModeledData, Box<dyn Error>> {
    // Learn regression, with special handling for degenerate cases
    let zero_dim_reg = run_zero_dimensional_regression(data, model_params)?;
    let reg = if !model_params.is_empty() && data.len() > 1 {
        run_linear_regression(data, model_params, zero_dim_reg.mean_fit, verbose)?
    } else {
        zero_dim_reg
    };
    if verbose {
        dbg_indent(2, &reg);
    }

    // Model data using the regression.
    let mut modeled_data: Vec<ModeledDataPoint> = vec![];
    for &p in data {
        let mut prediction = reg.constant;
        for param in model_params {
            let v = *p.model_values.get(param).unwrap() as f64;
            prediction += reg.param_coeffs.get(param).unwrap() * v;
        }
        let residual = p.measured_time - prediction;
        modeled_data.push(ModeledDataPoint {
            benchmark: (*p).clone(),
            predicted_time: prediction,
            residual,
        });
    }

    // Sort modeled data by measured time.
    modeled_data.sort_by(|a, b| {
        a.benchmark
            .measured_time
            .total_cmp(&b.benchmark.measured_time)
    });

    Ok(RegressionAndModeledData {
        regression: reg,
        data: modeled_data,
    })
}

/// Run a linear regression on the data, with regression variable for each model parameter.
///
/// This function performs multi-parameter linear regression using the specified benchmark
/// schema to map parameter names to regression variables. It uses the `linregress` crate
/// to build and fit a linear model of the form:
///
/// ```text
/// measured_time = constant + β₁×param₁ + β₂×param₂ + ... + βₙ×paramₙ
/// ```
fn run_linear_regression(
    data_points: &[&BenchmarkDataPoint],
    model_params: &ModelParams,
    mean_fit: f64,
    verbose: bool,
) -> Result<Regression, Box<dyn Error>> {
    let non_crashing_data_points: Vec<&BenchmarkDataPoint> = data_points
        .iter()
        .filter(|p| p.crashed != Some(true))
        .copied()
        .collect();
    // Gather the inputs and outputs.
    let mut table = std::collections::HashMap::new();
    // The outputs
    let mut measured_times = Vec::new();
    let n_params = model_params.len();
    // The inputs
    let mut param_columns: Vec<Vec<f64>> = vec![vec![]; n_params];
    for p in non_crashing_data_points {
        measured_times.push(p.measured_time);
        for (i, param) in model_params.iter().enumerate() {
            let v = *p.model_values.get(param).unwrap() as f64;
            param_columns[i].push(v);
        }
    }

    // Use regression to find best linear model that maps inputs to outputs.
    table.insert("Y", measured_times.clone());
    for (i, param) in model_params.iter().enumerate() {
        table.insert(param.as_str(), param_columns[i].clone());
    }
    // E.g. for a schema with `["key_size", "container_log_size"]`, the regression formula is
    // `Y ~ key_size + container_log_size`, where the constant term is implicit.
    let formula = format!("Y ~ {}", model_params.join(" + "));
    if verbose {
        println!("  Fitting regression model with formula: {}", formula);
    }
    let data = RegressionDataBuilder::new().build_from(table.clone())?;
    let model = FormulaRegressionBuilder::new()
        .data(&data)
        .formula(&formula)
        .fit()?;

    // Extract the model.
    let params = model.parameters();
    let constant = params[0];
    let mut param_coeffs = HashMap::new();
    for (i, param) in model_params.iter().enumerate() {
        param_coeffs.insert(param.clone(), params[i + 1]);
    }
    let regression = Regression {
        constant,
        param_coeffs,
        r_squared: model.rsquared(),
        adjusted_r_squared: model.rsquared_adj(),
        mean_fit,
    };
    Ok(regression)
}

/// Handle regression for zero-dimensional (parameter-less) operations.
///
/// This function provides a specialized regression approach for operations that have no
/// model parameters or insufficient data points for standard linear regression. It uses
/// the mean of measured times as the model and calculates an alternative fit metric.
///
/// # Explanation
///
/// The linear regression library won't compute regressions for a single data point, and for
/// zero-dimensional data with multiple data points (i.e., opcodes without model parameters but
/// with multiple container types), the standard `R²` is always 0 by definition, since the
/// least squares model is just the mean:
///
/// ```text
/// R² = 1 - SS_res / SS_tot = 1 - 1 = 0
/// ```
///
/// Where `SS_res = Σ(yᵢ - model(yᵢ))²` and `SS_tot = Σ(yᵢ - μ)²` are sums of
/// squared errors between data and model, and data and mean, resp. See
/// <https://en.wikipedia.org/wiki/Coefficient_of_determination> for details.
///
/// This function computes an alternative fit measure using the mean as the
/// model, and dividing by the sum of squares of the measurements:
///
/// ```text
/// fit = 1 - Σ(yᵢ - μ)² / Σ(yᵢ)²
/// ```
///
/// This equals 1 when all `yᵢ = μ`, and is small when data points are close to `μ`
/// relative to their magnitude.  WARNING: Nathan made this alternative fit
/// measure up, and there might be a better one ...
fn run_zero_dimensional_regression(
    data_points: &[&BenchmarkDataPoint],
    model_params: &ModelParams,
) -> Result<Regression, Box<dyn Error>> {
    let non_crashing_data_points: Vec<&BenchmarkDataPoint> = data_points
        .iter()
        .filter(|p| p.crashed != Some(true))
        .copied()
        .collect();

    let times: Vec<f64> = non_crashing_data_points
        .iter()
        .map(|d| d.measured_time)
        .collect();
    let n = non_crashing_data_points.len();
    let mean = times.iter().sum::<f64>() / (n as f64);
    let sum_of_squares = times.iter().map(|t| t.powi(2)).sum::<f64>();
    let sum_of_square_errors = times.iter().map(|&t| (t - mean).powi(2)).sum::<f64>();
    let fit = if sum_of_squares == 0.0 {
        1.0
    } else {
        1f64 - sum_of_square_errors / sum_of_squares
    };
    // We still need param coefficients, so we set them all to zero, since the
    // mean is the whole model. This is used in the degenerate case of
    // per-container benchmarks which only have a single data point.
    let param_coeffs: HashMap<_, _> = model_params
        .iter()
        .map(|param| (param.clone(), 0f64))
        .collect();
    let r = Regression {
        constant: mean,
        param_coeffs,
        r_squared: 0.0,
        adjusted_r_squared: 0.0,
        mean_fit: fit,
    };
    Ok(r)
}

/// A data point formatted for scatter plot visualization.
#[derive(Debug, Clone)]
struct ScatterDataPoint {
    container_type: String,
    /// X-axis value (parameter value or dummy coordinate)
    x: usize,
    /// Y-axis value: actual measured execution time in nanoseconds
    measured_time: f64,
    /// Model's predicted execution time in nanoseconds (for regression line)
    predicted_time: f64,
    /// Whether the benchmark operation crashed (for visual decoration)
    crashed: Option<bool>,
    /// Whether the key was present in the container (for visual decoration)
    key_present: Option<bool>,
}

/// Function type for extracting scatter plot data from modeled data points.
type ExtractFn = Box<dyn Fn(&ModeledDataPoint) -> Option<ScatterDataPoint>>;

/// Flexible data extraction system for generating different types of plots.
///
/// The `Extractor` provides a configurable way to transform regression data into
/// scatter plot points, supporting different visualization scenarios:
/// - **0D plots**: Constant operations (dummy x-axis)
/// - **1D plots**: Single parameter vs. time
/// - **2D slices**: One parameter vs. time with another parameter fixed
///
/// Each extractor encapsulates:
/// - **Data source**: Reference to regression results
/// - **Extraction logic**: Function to filter/transform data points
/// - **Plot metadata**: Axis labels, file-name fragments, titles
struct Extractor<'a> {
    reg_and_data: &'a RegressionAndModeledData,
    /// Fragment used in output file-name generation
    filename_fragment: String,
    /// Label for the plot's x-axis
    x_axis_name: String,
    /// Function that transforms `ModeledDataPoint` to `ScatterDataPoint`
    extract_fn: ExtractFn,
    /// Optional additional text for plot title (e.g., `"key_size=32"`)
    title_suffix: Option<String>,
}

impl<'a> Extractor<'a> {
    /// Extract scatter data points using the configured extraction function.
    fn extract(&self) -> Vec<ScatterDataPoint> {
        self.reg_and_data
            .data
            .iter()
            .filter_map(|p| (self.extract_fn)(p))
            .collect()
    }

    /// Create an extractor for zero-parameter operations.
    ///
    /// For operations with no model parameters, creates a dummy x-axis
    /// where all points have x=0. This allows visualization of timing
    /// variation across container types.
    fn new_zero_param(reg_and_data: &'a RegressionAndModeledData) -> Self {
        Extractor {
            reg_and_data,
            filename_fragment: "noparams".to_string(),
            x_axis_name: "dummy (no params)".to_string(),
            extract_fn: Box::new(|p| {
                Some(ScatterDataPoint {
                    container_type: p.benchmark.container_type.clone(),
                    x: 0,
                    measured_time: p.benchmark.measured_time,
                    predicted_time: p.predicted_time,
                    crashed: p.benchmark.crashed,
                    key_present: p.benchmark.key_present,
                })
            }),
            title_suffix: None,
        }
    }

    /// Create an extractor for single-parameter operations.
    ///
    /// Sets up extraction to plot the specified parameter on the x-axis
    /// against measured/predicted times on the y-axis.
    fn new_one_param(reg_and_data: &'a RegressionAndModeledData, param_name: &str) -> Self {
        let param_name_copy = param_name.to_string();
        Extractor {
            reg_and_data,
            filename_fragment: param_name.to_string(),
            x_axis_name: param_name.to_string(),
            extract_fn: Box::new(move |p| {
                let x = *p.benchmark.model_values.get(&param_name_copy).unwrap();
                Some(ScatterDataPoint {
                    container_type: p.benchmark.container_type.clone(),
                    x,
                    measured_time: p.benchmark.measured_time,
                    predicted_time: p.predicted_time,
                    crashed: p.benchmark.crashed,
                    key_present: p.benchmark.key_present,
                })
            }),
            title_suffix: None,
        }
    }

    /// Create an extractor for two-parameter operations with one parameter fixed.
    ///
    /// This creates a 2D slice by fixing one parameter at a specific value and
    /// plotting the other parameter against time. This allows visualization of
    /// how one parameter affects performance while the other is held constant.
    ///
    /// # Arguments
    /// * `var_param` - Parameter to vary on the x-axis
    /// * `fixed_param` - Parameter to hold constant
    /// * `fixed_value` - Value to fix the constant parameter at
    ///
    /// # Example
    /// For `new_two_param(data, "key_size", "container_log_size", 10)`:
    /// - x-axis: `key_size` values
    /// - Only includes points where `container_log_size` = 10
    /// - Title includes `"container_log_size=10"`
    fn new_two_param(
        reg_and_data: &'a RegressionAndModeledData,
        var_param: &str,
        fixed_param: &str,
        fixed_value: usize,
    ) -> Self {
        let var_param_copy = var_param.to_string();
        let fixed_param_copy = fixed_param.to_string();
        let var_param_owned = var_param.to_string();
        let fixed_param_owned = fixed_param.to_string();

        Extractor {
            reg_and_data,
            filename_fragment: format!(
                "{}_{}_fixed_{}",
                var_param_owned, fixed_param_owned, fixed_value
            ),
            x_axis_name: var_param_owned,
            extract_fn: Box::new(move |p| {
                // Only include points where the fixed parameter has the correct value
                if *p.benchmark.model_values.get(&fixed_param_copy).unwrap() == fixed_value {
                    let x = *p.benchmark.model_values.get(&var_param_copy).unwrap();
                    Some(ScatterDataPoint {
                        container_type: p.benchmark.container_type.clone(),
                        x,
                        measured_time: p.benchmark.measured_time,
                        predicted_time: p.predicted_time,
                        crashed: p.benchmark.crashed,
                        key_present: p.benchmark.key_present,
                    })
                } else {
                    None
                }
            }),
            title_suffix: Some(format!("2D slice: {}={}", fixed_param_owned, fixed_value)),
        }
    }
}

/// Plot measured data and regression line for a specific parameter configuration.
///
/// Creates a scatter plot showing measured vs. predicted execution times with:
/// - Scatter points grouped by container type (different colors/symbols)
/// - Regression line showing the model's predictions
/// - `R²` values and optional context in the title
///
/// # Arguments
/// * `plot_file` - Output path for the HTML plot file
/// * `op` - The operation name to include in the plot title
fn plot_regression(
    plot_file: &Path,
    container_type: &str,
    extractor: &Extractor,
    op: &str,
) -> Result<(), Box<dyn Error>> {
    let scatter_points: Vec<ScatterDataPoint> = extractor.extract();
    if scatter_points.is_empty() {
        // Do not plot empty data sets
        return Ok(());
    }

    // Create title with R-squared values, optional suffix, and shape/size
    // legend
    let reg = &extractor.reg_and_data.regression;
    let mut title_text = format!(
        "{}@{}: R² = {:.2}, Adjusted R² = {:.2}, Fit of Mean = {:.2}",
        op, container_type, reg.r_squared, reg.adjusted_r_squared, reg.mean_fit,
    );
    if let Some(suffix) = &extractor.title_suffix {
        title_text.push_str(&format!("\n{suffix}"));
    }
    title_text.push_str(
        "\nShape: ● = no-crash, ▲ = crashed | Size: small = key-absent, large = key-present",
    );

    let mut chart = Chart::new()
        .background_color("white")
        .title(Title::new().text(title_text.clone()))
        .x_axis(
            Axis::new()
                .name(extractor.x_axis_name.clone())
                .name_location(NameLocation::Middle)
                .name_gap(25),
        )
        .y_axis(
            Axis::new()
                .name("time (ns)")
                .name_location(NameLocation::Middle)
                .name_gap(80),
        );
    let mut legends = vec![];

    // Consistently color the container types and regression.
    fn item_style_for_legend(legend: &str) -> ItemStyle {
        let color_map = HashMap::from([
            ("null", "blue"),
            ("cell", "green"),
            ("array", "orange"),
            ("map", "red"),
            ("bmt", "purple"),
            ("regression", "black"),
        ]);
        if let Some(color) = color_map.get(legend) {
            ItemStyle::new().color(*color)
        } else {
            // Fall back on implicit color cycling.
            ItemStyle::new()
        }
    }

    // Plot scatter points grouped by (container_type, symbol_type,
    // size_category) combinations.  This allows us to decorate points based on
    // crashed/key_present parameters.

    // Use stringified Symbol key since Symbol doesn't implement Hash/Eq :P
    let mut scatter_groups: HashMap<
        (
            String, /* container */
            &str,   /* symbol */
            i32,    /* size */
        ),
        Vec<Vec<f64>>,
    > = HashMap::new();

    for p in &scatter_points {
        let symbol = match p.crashed {
            Some(false) => "circle",
            Some(true) => "triangle",
            None => "circle",
        };
        let size = match p.key_present {
            Some(false) => 5,
            Some(true) => 15,
            None => 10,
        };
        let key = (p.container_type.clone(), symbol, size);
        scatter_groups
            .entry(key)
            .or_default()
            .push(vec![p.x as f64, p.measured_time]);
    }

    // Create scatter series for each group
    fn string_to_symbol(s: &str) -> Symbol {
        match s {
            "circle" => Symbol::Circle,
            "triangle" => Symbol::Triangle,
            _ => unreachable!(),
        }
    }
    for ((container_type, symbol, size), scatter_data) in scatter_groups.iter() {
        legends.push(container_type.clone());
        chart = chart.series(
            Scatter::new()
                .data(scatter_data.clone())
                .symbol(string_to_symbol(symbol))
                .symbol_size(*size as f64)
                .name(container_type)
                .item_style(item_style_for_legend(container_type)),
        );
    }

    // Plot regression line
    let mut regression_points: Vec<(f64, f64)> = scatter_points
        .iter()
        .map(|p| (p.x as f64, p.predicted_time))
        .collect();
    // Sort by x coord to avoid zig-zagging line
    regression_points.sort_by(|a, b| a.0.total_cmp(&b.0));
    let line_data: Vec<Vec<f64>> = regression_points
        .iter()
        .map(|(x, y)| vec![*x, *y])
        .collect();
    chart = chart.series(
        Line::new()
            .data(line_data)
            .line_style(LineStyle::new().width(2))
            .name("regression")
            .item_style(item_style_for_legend("regression")),
    );
    legends.push("regression".into());

    // Tweak canvas size and positioning, and add padding, to prevent label
    // cutoff and title overlap
    chart = chart.legend(Legend::new().data(legends).top("8%").left("center"));
    let plot_file = plot_file.with_extension("html");
    HtmlRenderer::new(title_text, 1000, 800).save(&chart, &plot_file)?;
    #[cfg(feature = "svg")]
    {
        let plot_file = plot_file.with_extension("svg");
        ImageRenderer::new(1000, 800).save(&chart, &plot_file)?;
    }
    Ok(())
}

/// Helper function to extract unique values for a parameter across data points.
fn get_unique_param_values(data_points: &[&ModeledDataPoint], param: &str) -> Vec<usize> {
    data_points
        .iter()
        .map(|p| *p.benchmark.model_values.get(param).unwrap_or(&0))
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect()
}

/// Compute automatic plot restrictions by selecting min, mid, and max values
/// for each parameter.
///
/// For 2D models in auto mode, this function finds the minimum, middle, and
/// maximum values for each parameter in the provided data points, creating
/// slice plots at these points.
fn compute_auto_restrictions(
    data_points: &[&ModeledDataPoint],
    param_names: &[String],
) -> HashMap<String, Vec<usize>> {
    let mut restrictions = HashMap::new();
    for param in param_names {
        let mut values = get_unique_param_values(data_points, param);
        if !values.is_empty() {
            values.sort();
            let min_val = values[0];
            let mid_val = values[values.len() / 2];
            let max_val = values[values.len() - 1];
            // Use a set in case there are fewer than 3 values
            let selected_values = HashSet::from([min_val, mid_val, max_val]);
            restrictions.insert(param.clone(), selected_values.into_iter().collect());
        }
    }
    restrictions
}

/// Generate plots for a specific container type combination.
///
/// This function creates visualization plots based on the number of model parameters:
/// - **0 parameters**: Single dummy plot showing constant behavior
/// - **1 parameter**: Single scatter plot with parameter on x-axis
/// - **2 parameters**: Multiple 2D slice plots, filtering based on `param=value` restrictions
fn plot_for_container_types(
    reg_and_data: &RegressionAndModeledData,
    types_str: String,
    output_dir: &Path,
    op: &str,
    model_params: &ModelParams,
    restrictions: &PlotRestrictions,
) -> Result<(), Box<dyn Error>> {
    let data_points: Vec<&ModeledDataPoint> = reg_and_data.data.iter().collect();
    let param_names = model_params;
    let actual_restrictions = match restrictions {
        PlotRestrictions::Auto => {
            if param_names.len() == 2 {
                compute_auto_restrictions(&data_points, param_names)
            } else {
                HashMap::new()
            }
        }
        PlotRestrictions::Manual(manual_restrictions) => manual_restrictions.clone(),
    };
    let res = &actual_restrictions;

    if param_names.len() < 2 && !res.is_empty() {
        return Err(
            "Parameter restrictions with --plot only supported for ops with 2 params".into(),
        );
    }

    // Generate extractors for different parameter dimensions
    let extractors = match param_names.len() {
        0 => {
            vec![Extractor::new_zero_param(reg_and_data)]
        }
        1 => {
            let param = &param_names[0];
            vec![Extractor::new_one_param(reg_and_data, param)]
        }
        2 => {
            let param_x = &param_names[0];
            let param_y = &param_names[1];

            // Show all available slice values for this container type
            let unique_x = get_unique_param_values(&data_points, param_x);
            let unique_y = get_unique_param_values(&data_points, param_y);
            println!(
                "\
Possible plot slice params and values for '{op}@{types_str}':
- {}: {}
- {}: {}",
                param_x,
                unique_x.iter().sorted().join(", "),
                param_y,
                unique_y.iter().sorted().join(", "),
            );

            // Compute data extractors
            let mut extractors = Vec::new();
            for (variable_param, fixed_param) in [(param_y, param_x), (param_x, param_y)] {
                for &fixed_value in res.get(fixed_param).unwrap_or(&vec![]) {
                    extractors.push(Extractor::new_two_param(
                        reg_and_data,
                        variable_param,
                        fixed_param,
                        fixed_value,
                    ));
                }
            }
            extractors
        }
        _ => {
            println!("  Plotting for >2 model params is not supported. Skipping plot.");
            return Ok(());
        }
    };

    // Generate all plots using the extractor-based plotting function
    for extractor in &extractors {
        {
            let plot_file = output_dir.join(format!(
                "{}_{}_{}_plot",
                op, types_str, extractor.filename_fragment
            ));
            plot_regression(&plot_file, &types_str, extractor, op)
        }?;
    }

    Ok(())
}

/// Generate all visualization plots for the cost model analysis.
///
/// This function orchestrates the complete plotting phase by generating plots for:
/// 1. **Individual container types** (if multiple types exist)
/// 2. **Combined analysis** across all container types
///
/// All plots are saved as HTML and SVG files in the `/plot` sub-directory of the output directory argument.
fn generate_plots(
    op: &str,
    results: &OpModel,
    schema: &BenchmarkSchema,
    restrictions: &PlotRestrictions,
    output_dir: &Path,
    verbose: bool,
) -> Result<(), Box<dyn Error>> {
    let output_dir = output_dir.join("plot");
    std::fs::create_dir_all(&output_dir)?;
    let mut all_container_types: Vec<String> = results.per_container_type.keys().cloned().collect();
    all_container_types.sort();

    // Plot for each individual container type
    for ctype in &all_container_types {
        if verbose {
            println!("Generating plots for {op}@{ctype}");
        }
        plot_for_container_types(
            results.per_container_type.get(ctype).unwrap(),
            ctype.clone(),
            &output_dir,
            op,
            &(schema.model_params)(ctype),
            restrictions,
        )?;
    }

    // Plot for all container types combined
    if verbose && all_container_types.len() > 1 {
        println!("Generating plots for {op}@all ...");
        plot_for_container_types(
            &results.combined,
            "all".into(),
            &output_dir,
            op,
            &(schema.model_params)("all"),
            restrictions,
        )?;
    }
    Ok(())
}
