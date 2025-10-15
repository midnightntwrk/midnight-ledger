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

fn update_tag_versions(content: &str, tags_filter: &Option<Vec<String>>) -> (String, bool) {
    // Match #[tag = "..."] only at the start of a line (with optional leading whitespace)
    let re = Regex::new(r#"(?m)^(\s*)#\[tag = "(.+?)(?:\[v(\d+)\])?"?\]"#).unwrap();
    let mut changed = false;

    let updated = re.replace_all(content, |caps: &regex::Captures| {
        let leading_whitespace = &caps[1];
        let base_tag = &caps[2];

        // Check if we should update this tag
        let should_update = match tags_filter {
            Some(tags) => tags.iter().any(|t| t == base_tag),
            None => true, // No filter, update all tags
        };

        if !should_update {
            // Return the original match unchanged
            return caps[0].to_string();
        }

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

fn process_file<P: AsRef<Path>>(path: P, tags_filter: &Option<Vec<String>>) -> std::io::Result<()> {
    let path = path.as_ref();
    if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("rs") {
        let content = fs::read_to_string(&path)?;
        let (updated_content, changed) = update_tag_versions(&content, tags_filter);

        if changed {
            println!("Updating file: {:?}", path);
            fs::write(path, updated_content)?;
        }
    }
    Ok(())
}

fn process_dir(path: impl AsRef<Path>, tags_filter: &Option<Vec<String>>) -> std::io::Result<()> {
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            process_dir(path, tags_filter)?;
        } else {
            process_file(path, tags_filter)?;
        }
    }
    Ok(())
}

fn main() -> std::io::Result<()> {
    let args: Vec<String> = env::args().collect();

    let mut path = ".";
    let mut tags_filter: Option<Vec<String>> = None;

    // Find --tags position
    if let Some(tags_pos) = args.iter().position(|arg| arg == "--tags") {
        // Everything after --tags is a tag name
        if tags_pos + 1 < args.len() {
            tags_filter = Some(args[tags_pos + 1..].iter().map(|s| s.clone()).collect());
        }

        // Path is the first arg before --tags (if any)
        if tags_pos > 1 {
            path = &args[1];
        }
    } else if args.len() > 1 {
        // No --tags, just a path argument
        path = &args[1];
    }

    let path = Path::new(path);
    if fs::metadata(&path)?.is_dir() {
        process_dir(path, &tags_filter)?
    } else {
        process_file(path, &tags_filter)?
    }
    Ok(())
}
