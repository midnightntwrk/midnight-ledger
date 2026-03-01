// This file is part of midnight-ledger.
// Copyright (C) Midnight Foundation
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

//! Translates the parameters from the format provided in
//! https://srs.midnight.network/current_srs/powers_of_tau (as at the end of the ceremony)

use group::prime::PrimeCurveAffine;
use midnight_curves::{Bls12, G1Affine, G2Affine, serde::SerdeObject};
use midnight_proofs::poly::kzg::params::ParamsKZG;
use rayon::prelude::*;

pub fn main() {
    let bytes = std::fs::read("powers_of_tau").unwrap();
    let g1_size = G1Affine::uncompressed_size();
    let g2_size = G2Affine::uncompressed_size();
    let read_g1_point = |bytes| G1Affine::from_raw_bytes(bytes).unwrap();
    let read_g2_point = |bytes| G2Affine::from_raw_bytes(bytes).unwrap();
    let offset = bytes.len() - 2 * g2_size;
    let g1s = bytes[..offset]
        .par_chunks(g1_size)
        .map(read_g1_point)
        .map(Into::into)
        .collect::<Vec<_>>();
    let mut g2s = [G2Affine::generator(); 2];
    g2s[0] = read_g2_point(&bytes[offset..offset + g2_size]);
    g2s[1] = read_g2_point(&bytes[offset + g2_size..offset + 2 * g2_size]);
    let mut params = ParamsKZG::<Bls12>::from_parts(25, g1s, None, g2s[0].into(), g2s[1].into());
    for k in (0..=25).rev() {
        eprintln!("k={k}");
        let mut f = std::fs::File::create(format!("bls_midnight_2p{k}")).unwrap();
        params.downsize(k);
        params
            .write_custom(&mut f, midnight_proofs::utils::SerdeFormat::RawBytes)
            .unwrap();
    }
}
