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

use regex::Regex;
use std::env;
use std::fs;
use std::path::Path;

fn update_tag_versions(content: &str) -> (String, bool) {
    // Match #[tag = "..."] only at the start of a line (with optional leading whitespace)
    let re = Regex::new(r#"(?m)^(\s*)#\[tag = "(.+?)(?:\[v(\d+)\])?"?\]"#).unwrap();
    let mut changed = false;

    let updated = re.replace_all(content, |caps: &regex::Captures| {
        let leading_whitespace = &caps[1];
        let base_tag = &caps[2];

        let new_version = if let Some(version_str) = caps.get(3) {
            // Version exists, increment it
            let current_version: u32 = version_str.as_str().parse().unwrap_or(1);
            current_version + 1
        } else {
            // No version, start with v1
            1
        };

        changed = true;
        format!(r#"{}#[tag = "{}[v{}]"]"#, leading_whitespace, base_tag, new_version)
    });

    (updated.to_string(), changed)
}

fn process_file<P: AsRef<Path>>(path: P) -> std::io::Result<()> {
    let path = path.as_ref();
    if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("rs") {
        let content = fs::read_to_string(&path)?;
        let (updated_content, changed) = update_tag_versions(&content);

        if changed {
            println!("Updating file: {:?}", path);
            fs::write(path, updated_content)?;
        }
    }
    Ok(())
}

fn process_dir(path: impl AsRef<Path>) -> std::io::Result<()> {
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            process_dir(path)?;
        } else {
            process_file(path)?;
        }
    }
    Ok(())
}

fn main() -> std::io::Result<()> {
    let args: Vec<String> = env::args().collect();
    let path = if args.len() > 1 { &args[1] } else { "." };
    let path = Path::new(path);
    if fs::metadata(&path)?.is_dir() {
        process_dir(path)?
    } else {
        process_file(path)?
    }
    Ok(())
}
