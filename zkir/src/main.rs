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

//! ZKIR compiler.
#![deny(unreachable_pub)]
//#![deny(warnings)]
use base_crypto::data_provider::{self, MidnightDataProvider};
use clap::{Parser, Subcommand};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use serialize::{tagged_deserialize, tagged_serialize};
use std::ffi::OsString;
use std::fs::File;
use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tracing::info;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::Registry;
use tracing_subscriber::filter::Targets;
use tracing_subscriber::prelude::*;
use transient_crypto::proofs::Zkir;
use zkir::IrSource;

#[derive(Parser)]
#[clap(version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Subcommands,
}

#[derive(Subcommand, Debug)]
enum Subcommands {
    /// Mock the compilation step
    MockCompile {
        /// Whether to output debugging information
        #[arg(short, long)]
        verbose: bool,
        /// The input IR file
        ir_file: PathBuf,
    },
    /// Mock the compilation step for all sources in a directory
    MockCompileMany {
        /// Whether to output debugging information
        #[arg(short, long)]
        verbose: bool,
        /// The input IR directory
        ir_dir: PathBuf,
    },
    /// Generate prover and verifier keys for all sources in a directory
    CompileMany {
        /// Whether to output debugging information
        #[arg(short, long)]
        verbose: bool,
        /// The input IR directory
        ir_dir: PathBuf,
        /// The output key directory
        key_dir: PathBuf,
    },
    /// Generate prover and verifier keys
    Compile {
        /// Whether to output debugging information
        #[arg(short, long)]
        verbose: bool,
        /// The input IR file
        ir_file: PathBuf,
        /// The output prover key file
        prover_key: PathBuf,
        /// The output verifier key file
        verifier_key: PathBuf,
    },
}

fn maybe_bzkir(path: impl AsRef<Path>) -> anyhow::Result<IrSource> {
    match path.as_ref().extension().map(|s| s.to_str()) {
        Some(Some("zkir")) => {
            let ir = IrSource::load(BufReader::new(File::open(&path)?))?;
            let mut bzkir = BufWriter::new(File::create(path.as_ref().with_extension("bzkir"))?);
            tagged_serialize(&ir, &mut bzkir)?;
            Ok(ir)
        }
        _ => Ok(tagged_deserialize(&mut BufReader::new(File::open(path)?))?),
    }
}

fn without_extension(path: impl AsRef<Path>) -> anyhow::Result<IrSource> {
    let zkir = path.as_ref().to_owned().with_extension("zkir");
    let bzkir = path.as_ref().to_owned().with_extension("bzkir");
    if std::fs::exists(&zkir)? {
        maybe_bzkir(zkir)
    } else {
        maybe_bzkir(bzkir)
    }
}

fn extract_files_from_dir(dir: impl AsRef<Path>) -> anyhow::Result<Vec<OsString>> {
    let mut files = std::fs::read_dir(dir)?
        .filter_map(|e| {
            let name = match e {
                Ok(e) => e,
                Err(err) => return Some(Err(err)),
            }
            .file_name();
            let stem = Path::file_stem(name.as_ref());
            let extension = Path::extension(name.as_ref());
            if extension == Some("zkir".as_ref()) || extension == Some("bzkir".as_ref()) {
                stem.map(|s| Ok::<_, std::io::Error>(s.to_owned()))
            } else {
                None
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    files.sort();
    files.dedup();
    Ok(files)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();
    let args = Cli::parse();

    match &args.command {
        Subcommands::MockCompile { verbose, .. }
        | Subcommands::CompileMany { verbose, .. }
        | Subcommands::Compile { verbose, .. }
        | Subcommands::MockCompileMany { verbose, .. } => {
            if *verbose {
                Registry::default()
                    .with(
                        tracing_subscriber::fmt::layer()
                            .with_filter(Targets::new().with_default(LevelFilter::TRACE)),
                    )
                    .try_init()
                    .ok();
            }
        }
    }

    match &args.command {
        Subcommands::MockCompile { ir_file, .. } => {
            let mut term = console::Term::stderr();
            write!(term, "Mock compiling circuit {ir_file:?}")?;
            term.flush()?;
            let ir = maybe_bzkir(ir_file)?;
            let model = ir.model(None);
            writeln!(term, " (k={}, rows={})", model.k(), model.rows())?;
            info!(?model, "full model");
        }
        Subcommands::MockCompileMany { ir_dir, .. } => {
            let files = extract_files_from_dir(ir_dir)?;
            let mut term = console::Term::stderr();
            writeln!(term, "Mock compiling {} circuits:", files.len())?;
            for file in files.iter() {
                write!(term, "  circuit {file:?}")?;
                term.flush()?;
                let path = ir_dir.join(file);
                let ir = without_extension(path)?;
                let model = ir.model(None);
                writeln!(term, " (k={}, rows={})", model.k(), model.rows())?;
                info!(?model, "full model for {file:?}");
            }
        }
        Subcommands::CompileMany {
            ir_dir, key_dir, ..
        } => {
            let files = extract_files_from_dir(ir_dir)?;
            std::fs::create_dir_all(key_dir)?;
            let mut term = console::Term::stderr();
            writeln!(term, "Compiling {} circuits:", files.len())?;
            let multi = MultiProgress::new();
            let pp = MidnightDataProvider::new(
                data_provider::FetchMode::OnDemand,
                data_provider::OutputMode::Cli(multi.clone()),
                vec![],
            )?;
            let mut data = vec![];
            for file in files.iter() {
                let path = ir_dir.join(file);
                let pb = ProgressBar::new_spinner().with_style(
                    ProgressStyle::with_template("{msg} {spinner:.green.bold}")
                        .expect("static style should be valid")
                        .tick_chars("|/-\\ "),
                );
                let pb = multi.add(pb);
                pb.set_message(format!("  circuit {file:?}"));
                let ir = without_extension(path)?;
                let k = ir.k();
                pb.set_message(format!("  circuit {file:?} (k={k})"));
                let model = ir.model(None);
                pb.set_message(format!("  circuit {file:?} (k={k}, rows={})", model.rows()));
                info!(?model, "full model for {file:?}");
                data.push((pb, ir, k));
            }
            let size = data.iter().map(|(_, _, k)| 1u64 << *k as u64).sum::<u64>();
            let overall = ProgressBar::new(size).with_style(
                ProgressStyle::with_template("{prefix} [{bar:20.green.bold}] {msg}")
                    .expect("Static style should parse")
                    .progress_chars("=> "),
            );
            let overall = multi.add(overall);
            overall.set_prefix("Overall progress");
            overall.set_message(format!("0/{}", data.len()));
            let mut prog = 0u64;
            let mut n = 0;
            for (file, (pb, ir, k)) in files.iter().zip(data.iter()) {
                prog += 1u64 << *k as u64;
                n += 1;
                let mut pk_file =
                    BufWriter::new(File::create(key_dir.join(file).with_extension("prover"))?);
                let mut vk_file =
                    BufWriter::new(File::create(key_dir.join(file).with_extension("verifier"))?);
                pb.enable_steady_tick(Duration::from_millis(100));
                let (pk, vk) = ir.keygen(&pp).await?;
                tagged_serialize(&pk, &mut pk_file)?;
                tagged_serialize(&vk, &mut vk_file)?;
                pb.finish();
                overall.set_message(format!("{n}/{}", data.len()));
                overall.set_position(prog);
            }
            overall.finish();
        }
        Subcommands::Compile {
            ir_file,
            prover_key,
            verifier_key,
            ..
        } => {
            let mut term = console::Term::stderr();
            write!(term, "Compiling circuit {ir_file:?}")?;
            term.flush()?;
            let ir = maybe_bzkir(ir_file)?;
            let mut pk_file = BufWriter::new(File::create(prover_key)?);
            let mut vk_file = BufWriter::new(File::create(verifier_key)?);
            let k = ir.k();
            let model = ir.model(None);
            write!(term, " (k={k}, rows={})", model.rows())?;
            info!(?model, "full model");
            term.flush()?;
            write!(term, "\r")?;
            let pb = ProgressBar::new_spinner().with_style(
                ProgressStyle::with_template("{msg} {spinner:.green.bold}")
                    .expect("static style should be valid")
                    .tick_chars("|/-\\"),
            );
            let multi = MultiProgress::new();
            let pb = multi.add(pb);
            let pp = MidnightDataProvider::new(
                data_provider::FetchMode::OnDemand,
                data_provider::OutputMode::Cli(multi),
                vec![],
            )?;
            pb.set_message(format!("Compiling circuit {ir_file:?} (k={k})"));
            pb.enable_steady_tick(Duration::from_millis(100));
            let (pk, vk) = ir.keygen(&pp).await?;
            tagged_serialize(&pk, &mut pk_file)?;
            tagged_serialize(&vk, &mut vk_file)?;
            pb.finish();
        }
    }
    Ok(())
}
