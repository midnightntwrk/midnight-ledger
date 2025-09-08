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

//! Provides mechanisms to fetch Midnight proof-related parameters and keys.

use futures::StreamExt;
#[cfg(feature = "cli")]
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use lazy_static::lazy_static;
use reqwest::Url;
use sha2::Digest;
use sha2::Sha256;
use std::env;
use std::fs::File;
use std::io;
use std::io::BufReader;
use std::io::Read;
use std::io::Seek;
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;
use std::time::Instant;
use tracing::{info, warn};

/// Retrieves various static cryptographic artifacts from a data server.
/// This keeps a local file system cache of the parameters, prover keys, verifier keys, and IR, that
/// can also be fetched remotely.
///
/// The local cache is located at: `$MIDNIGHT_PP` / `$XDG_CACHE_HOME/midnight/zk-params` /
/// `$HOME/.cache/midnight/zk-params` in that order of fall-backs.
///
/// The provider knows which data is in scope, and the SHA-256 hashes of each of these. When
/// reading one of the values from cache, or fetching them, the SHA-256 hash is verified for
/// integrity.
///
/// The data provider can operate in an on-demand, or synchronous mode. In the former, if a datum
/// is *not* locally available, it is fetched when requested. In the latter, the datum is only
/// fetched when explicitly requested via a [`MidnightDataProvider::fetch`] call. Key material is
/// *always* synchronous.
#[derive(Clone)]
pub struct MidnightDataProvider {
    /// How to handle requests to fetch data
    pub fetch_mode: FetchMode,
    /// The base URL of the data store to use.
    ///
    /// Fetching an item with `name` will request it from `{base_url}/{name}`.
    pub base_url: Url,
    /// How to report status of fetching to the user
    pub output_mode: OutputMode,
    /// Additional (non-parameter) files allows to be fetched
    /// Triple of `file_path`, SHA-256 `hash`, and `description`
    pub expected_data: Vec<(&'static str, [u8; 32], &'static str)>,
    /// The path to the directory where Midnight key material is stored
    pub dir: PathBuf,
}

lazy_static! {
    /// The default base URL to use for the Midnight data provider.
    pub static ref BASE_URL: Url = Url::parse(&std::env::var("MIDNIGHT_PARAM_SOURCE").unwrap_or("https://midnight-s3-fileshare-dev-eu-west-1.s3.eu-west-1.amazonaws.com/".to_owned())).expect("$MIDNIGHT_PARAM_SOURCE should be a valid URL");
}

/// Parse a 256-bit hex hash at const time.
pub const fn hexhash(hex: &[u8]) -> [u8; 32] {
    match const_hex::const_decode_to_array(hex) {
        Ok(hash) => hash,
        Err(_) => panic!("hash should be correct format"),
    }
}

const EXPECTED_DATA: &[(&str, [u8; 32], &str)] = &[
    (
        "bls_filecoin_2p10",
        hexhash(b"d1a3403c1f8669e82ed28d9391e13011aea76801b28fe14b42bf76d141b4efa2"),
        "public parameters for k=10",
    ),
    (
        "bls_filecoin_2p11",
        hexhash(b"b5047f05800dbd84fd1ea43b96a8850e128b7a595ed132cd72588cc2cb146b29"),
        "public parameters for k=11",
    ),
    (
        "bls_filecoin_2p12",
        hexhash(b"b32791775af5fff1ae5ead682c3d8832917ebb0652b43cf810a1e3956eb27a71"),
        "public parameters for k=12",
    ),
    (
        "bls_filecoin_2p13",
        hexhash(b"b9af43892c3cb90321fa00a36e5e59051f356df145d7f58368531f28d212937b"),
        "public parameters for k=13",
    ),
    (
        "bls_filecoin_2p14",
        hexhash(b"4923e5a7fbb715d81cdb5c03b9c0e211768d35ccc52d82f49c3d93bcf8d36a56"),
        "public parameters for k=14",
    ),
    (
        "bls_filecoin_2p15",
        hexhash(b"162fac0cf70b9b02e02195ec37013c04997b39dc1831a97d5a83f47a9ce39c97"),
        "public parameters for k=15",
    ),
    (
        "bls_filecoin_2p16",
        hexhash(b"4ebc0d077fe6645e9b7ca6563217be2176f00dfe39cc97b3f60ecbad3573f973"),
        "public parameters for k=16",
    ),
    (
        "bls_filecoin_2p17",
        hexhash(b"7228c4519e96ece2c54bf2f537d9f26b0ed042819733726623fab5e17eac4360"),
        "public parameters for k=17",
    ),
    (
        "bls_filecoin_2p18",
        hexhash(b"4f023825c14cc0a88070c70588a932519186d646094eddbff93c87a46060fd28"),
        "public parameters for k=18",
    ),
    (
        "bls_filecoin_2p19",
        hexhash(b"0574a536c128142e89c0f28198d048145e2bb2bf645c8b81c8697cba445a1fb1"),
        "public parameters for k=19",
    ),
    (
        "bls_filecoin_2p20",
        hexhash(b"75a1774fdf0848f4ff82790202e5c1401598bafea27321b77180d96c56e62228"),
        "public parameters for k=20",
    ),
    (
        "bls_filecoin_2p21",
        hexhash(b"e05fcbe4f7692800431cfc32e972be629c641fca891017be09a8384d0b5f8d3c"),
        "public parameters for k=21",
    ),
    (
        "bls_filecoin_2p22",
        hexhash(b"277d9c8140c02a1d4472d5da65a823fc883bc4596e69734fb16ca463d193186b"),
        "public parameters for k=22",
    ),
    (
        "bls_filecoin_2p23",
        hexhash(b"7b8dc4b2e809ef24ed459cabaf9286774cf63f2e6e2086f0d9fb014814bdfc97"),
        "public parameters for k=23",
    ),
    (
        "bls_filecoin_2p24",
        hexhash(b"e6b02dccf381a5fc7a79ba4d87612015eba904241f81521e2dea39a60ab6b812"),
        "public parameters for k=24",
    ),
];

impl MidnightDataProvider {
    /// Creates a new data provider with the default base URL.
    pub fn new(
        fetch_mode: FetchMode,
        output_mode: OutputMode,
        expected_data: Vec<(&'static str, [u8; 32], &'static str)>,
    ) -> io::Result<Self> {
        Ok(Self {
            fetch_mode,
            base_url: BASE_URL.clone(),
            output_mode,
            expected_data,
            dir: env::var_os("MIDNIGHT_PP")
                .map(PathBuf::from)
                .or_else(|| {
                    env::var_os("XDG_CACHE_HOME")
                        .map(|p| PathBuf::from(p).join("midnight").join("zk-params"))
                })
                .or_else(|| {
                    env::var_os("HOME").map(|p| {
                        PathBuf::from(p)
                            .join(".cache")
                            .join("midnight")
                            .join("zk-params")
                    })
                })
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::NotFound,
                        "Could not determine $HOME, $XDG_CACHE_HOME, or $MIDNIGHT_PP",
                    )
                })?,
        })
    }

    fn expected_hash(&self, name: &str) -> io::Result<[u8; 32]> {
        Ok(EXPECTED_DATA
            .iter()
            .chain(self.expected_data.iter())
            .find(|(n, ..)| *n == name)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "artifact '{name}' is not a known managed artifact by the proof data cache."
                    ),
                )
            })?
            .1)
    }

    fn description(&self, name: &str) -> io::Result<&'static str> {
        Ok(EXPECTED_DATA
            .iter()
            .chain(self.expected_data.iter())
            .find(|(n, ..)| *n == name)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "artifact '{name}' is not a known managed artifact by the proof data cache."
                    ),
                )
            })?
            .2)
    }

    fn get_local(&self, name: &str) -> io::Result<Option<BufReader<File>>> {
        let path = self.dir.join(name);
        let expected_hash = self.expected_hash(name)?;
        if !std::fs::exists(&path)? {
            return Ok(None);
        }
        let mut file = BufReader::new(File::open(&path)?);
        let mut hasher = Sha256::new();
        let mut buf = [0u8; 1 << 20];
        loop {
            let read = file.read(&mut buf)?;
            if read == 0 {
                break;
            }
            hasher.update(&buf[..read]);
        }
        let actual_hash = <[u8; 32]>::from(hasher.finalize());
        if actual_hash != expected_hash {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "Hash mismatch in data stored at {}. Found hash {}, but expected {}. Please try removing this file to force a re-fetch. If that does not work, you may be subject to an attack.",
                    path.display(),
                    const_hex::encode(actual_hash),
                    const_hex::encode(expected_hash)
                ),
            ));
        }
        file.seek(io::SeekFrom::Start(0))?;
        Ok(Some(file))
    }

    async fn get_or_fetch(&self, name: &str) -> io::Result<BufReader<File>> {
        if let Some(data) = self.get_local(name)? {
            return Ok(data);
        };
        let expected_hash = self.expected_hash(name)?;
        let path = self.dir.join(name);
        let parent = path.parent().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("parent of path file {name} should exist."),
            )
        })?;
        std::fs::create_dir_all(parent)?;
        let mut file = atomic_write_file::OpenOptions::new()
            .read(true)
            .open(&path)?;
        self.fetch_data_to(name, expected_hash, &mut file).await?;
        let mut rfile = file.as_file().try_clone()?;
        file.commit()?;
        rfile.seek(io::SeekFrom::Start(0))?;
        Ok(BufReader::new(rfile))
    }

    /// Fetches a given item.
    pub async fn fetch(&self, name: &str) -> io::Result<()> {
        self.get_or_fetch(name).await?;
        Ok(())
    }

    /// The name of the public parameters for the given `k` value.
    pub fn name_k(k: u8) -> String {
        format!("bls_filecoin_2p{k}")
    }

    /// Fetches the public parameters for a give `k`.
    pub async fn fetch_k(&self, k: u8) -> io::Result<()> {
        self.fetch(&Self::name_k(k)).await
    }

    // Only arise due to feature gates.
    #[allow(irrefutable_let_patterns)]
    async fn fetch_data_to(
        &self,
        name: &str,
        expected_hash: [u8; 32],
        f: &mut File,
    ) -> io::Result<()> {
        const RETRIES: usize = 3;
        let desc = self.description(name)?;
        if let OutputMode::Log = &self.output_mode {
            info!(
                "Missing {desc}. Attempting to download from the host {} - this is not a trusted service, the data will be verified.",
                self.base_url
            );
        }
        #[cfg(feature = "cli")]
        if let OutputMode::Cli(pb) = &self.output_mode {
            pb.println(format!("Missing {desc}. Attempting to download from the host {} - this is not a trusted service, the data will be verified.", self.base_url))?;
        }
        let mut url = self.base_url.clone();
        url.path_segments_mut()
            .map_err(|()| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "Base URL '{}' for proving data provider invalid",
                        &self.base_url
                    ),
                )
            })?
            .push(name);
        for i in 0..RETRIES {
            let retry_msg = if i == RETRIES - 1 {
                "Giving up."
            } else {
                "Retrying..."
            };
            f.seek(io::SeekFrom::Start(0))?;
            f.set_len(0)?;
            let mut hasher = Sha256::new();
            let res = match reqwest::Client::new().get(url.clone()).send().await {
                Ok(res) => res,
                Err(e) => {
                    #[cfg(feature = "cli")]
                    if let OutputMode::Cli(pb) = &self.output_mode {
                        pb.println(format!("{e}. {retry_msg}"))?;
                    }
                    warn!("{e}. {retry_msg}");
                    continue;
                }
            };
            let total_size = res.content_length();
            #[cfg(feature = "cli")]
            let pb = if let OutputMode::Cli(multi) = &self.output_mode {
                let pb = match total_size {
                    Some(size) => ProgressBar::new(size).with_style(
                        ProgressStyle::with_template(
                            "{msg} [{bar:.green.bold}] {bytes:.bold} / {total_bytes:.bold}",
                        )
                        .expect("Static style should parse")
                        .progress_chars("=> "),
                    ),
                    None => ProgressBar::no_length().with_style(
                        ProgressStyle::with_template("{msg} {spinner:.green.bold} {bytes:.bold}")
                            .expect("Static style should parse"),
                    ),
                };
                let pb = multi.insert(0, pb);
                pb.set_message(format!("Fetching {desc}"));
                Some(pb)
            } else {
                None
            };
            let mut downloaded: u64 = 0;
            let mut t_last = Instant::now();
            const LOG_UPDATE_FREQ: Duration = Duration::from_secs(5);
            let mut stream = res.bytes_stream();

            while let Some(resp) = stream.next().await {
                let data = match resp {
                    Ok(res) => res,
                    Err(e) => {
                        #[cfg(feature = "cli")]
                        if let OutputMode::Cli(pb) = &self.output_mode {
                            pb.println(format!("{e}. {retry_msg}"))?;
                        }
                        warn!("{e}. {retry_msg}");
                        continue;
                    }
                };
                f.write_all(&data)?;
                hasher.update(&data);
                downloaded += data.len() as u64;
                #[cfg(feature = "cli")]
                if let Some(pb) = &pb {
                    pb.set_position(downloaded);
                }
                let t = Instant::now();
                if matches!(self.output_mode, OutputMode::Log) && t - t_last > LOG_UPDATE_FREQ {
                    t_last = t;
                    match total_size {
                        Some(size) => {
                            info!("Fetching '{name}' - {downloaded} / {size} bytes downloaded")
                        }
                        None => info!("Fetching '{name}' - {downloaded} bytes downloaded"),
                    }
                }
            }
            info!("Fetching {desc} - finished.");
            #[cfg(feature = "cli")]
            if let Some(pb) = pb {
                pb.finish();
            }
            let hash = <[u8; 32]>::from(hasher.finalize());
            if hash == expected_hash {
                if let OutputMode::Log = self.output_mode {
                    info!("Fetching {desc} - verified correct.");
                }
                return Ok(());
            }
            warn!(
                ?hash,
                ?expected_hash,
                "Fetching {desc} - hash mismatch. {retry_msg}"
            );
        }
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Failed to fetch data from {url} after {RETRIES} attempts. Giving up."),
        ))
    }

    /// Retrieves a file from the data provider according to the fetch mode, giving a specific
    /// error message on failure.
    pub async fn get_file(&self, name: &str, desc: &str) -> io::Result<BufReader<File>> {
        Ok(match self.fetch_mode {
            FetchMode::OnDemand => self.get_or_fetch(name).await?,
            FetchMode::Synchronous => self
                .get_local(name)?
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, desc))?,
        })
    }
}

/// How to behave when fetching data
#[derive(Debug, Copy, Clone)]
pub enum FetchMode {
    /// Fetch on demand, whenever it gets accessed
    OnDemand,
    /// Fetch data only when explicitly requested
    Synchronous,
}

#[derive(Debug, Clone)]
/// How to output updates to the user
pub enum OutputMode {
    #[cfg(feature = "cli")]
    /// Assume an interactive CLI
    Cli(MultiProgress),
    /// Assume logging output only
    Log,
}
