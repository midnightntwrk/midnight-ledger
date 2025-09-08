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

use quote::ToTokens;
use std::env;
use std::fs;
use std::path::Path;
use std::str::FromStr;
use syn::{parse_file, Expr, ImplItem, Item, Pat};
use syn::{ExprLit, ImplItemFn};
use syn::{File, ItemImpl};

fn next_version_from(version: String) -> Option<ExprLit> {
    u8::from_str(&version)
        .ok()
        .map(|current| current.saturating_add(100))
        .and_then(|next| syn::parse_str(next.to_string().as_str()).ok())
}

fn update_versioned_trait_impl(item_impl: &mut ItemImpl) -> bool {
    for impl_item in &mut item_impl.items {
        if let ImplItem::Const(const_item) = impl_item {
            if const_item.ident == "VERSION" {
                if let Expr::Call(expr_call) = &mut const_item.expr {
                    if let Some(Expr::Struct(expr_struct)) = expr_call.args.first_mut() {
                        let mut changed = false;
                        for field in &mut expr_struct.fields {
                            match field.member.to_token_stream().to_string().as_str() {
                                "major" | "minor" => {
                                    if let Some(next) =
                                        next_version_from(field.expr.to_token_stream().to_string())
                                    {
                                        field.expr = Expr::Lit(next);
                                        changed = true;
                                    }
                                }
                                _ => (),
                            }
                        }
                        return changed;
                    }
                }
            }
        }
    }

    return false;
}

fn update_deserialized_fn_impl(method: &mut ImplItemFn) -> bool {
    let mut changed = false;

    // Handle match expressions
    if let Some(expr_match) = method.block.stmts.iter_mut().find_map(|stmt| {
        if let syn::Stmt::Expr(Expr::Match(expr_match), _) = stmt {
            Some(expr_match)
        } else {
            None
        }
    }) {
        for arm in &mut expr_match.arms {
            if let Pat::TupleStruct(pat_ts) = &mut arm.pat {
                if let Some(Pat::Struct(pat_struct)) = &mut pat_ts.elems.first_mut() {
                    if pat_struct.path.segments.last().unwrap().ident == "Version" {
                        for field in &mut pat_struct.fields {
                            match field.member.to_token_stream().to_string().as_str() {
                                "major" | "minor" => {
                                    if let Some(next) =
                                        next_version_from(field.pat.to_token_stream().to_string())
                                    {
                                        field.pat = Box::new(Pat::Lit(next));
                                        changed = true;
                                    }
                                }
                                _ => (),
                            }
                        }
                    }
                }
            }
        }
    }

    // Handle const check_injected_version statements
    for stmt in &mut method.block.stmts {
        if let syn::Stmt::Item(Item::Const(item_const)) = stmt {
            if let Expr::Call(expr_call) = &mut item_const.expr.as_mut() {
                if expr_call.func.to_token_stream().to_string().contains("check_injected_version") {
                    if let Some(Expr::Call(arg_call)) = expr_call.args.first_mut() {
                        if arg_call.func.to_token_stream().to_string().contains("Some") {
                            if let Some(Expr::Struct(expr_struct)) = &mut arg_call.args.first_mut() {
                                if expr_struct.path.segments.last().unwrap().ident == "Version" {
                                    for field in &mut expr_struct.fields {
                                        match field.member.to_token_stream().to_string().as_str() {
                                            "major" | "minor" => {
                                                if let Expr::Lit(expr_lit) = &field.expr {
                                                    if let Some(next) = next_version_from(
                                                        expr_lit.lit.to_token_stream().to_string(),
                                                    ) {
                                                        field.expr = Expr::Lit(next);
                                                        changed = changed || true;
                                                    }
                                                }
                                            }
                                            _ => (),
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    changed
}

fn update_version(ast: &mut File) -> bool {
    let mut changed: bool = false;

    for item in &mut ast.items {
        if let Item::Impl(item_impl) = item {
            if item_impl.trait_.is_some()
                && item_impl
                    .trait_
                    .as_ref()
                    .unwrap()
                    .1
                    .to_token_stream()
                    .to_string()
                    .contains("Versioned")
            {
                changed |= update_versioned_trait_impl(item_impl);
            } else if item_impl.trait_.is_some()
                && item_impl
                    .trait_
                    .as_ref()
                    .unwrap()
                    .1
                    .to_token_stream()
                    .to_string()
                    .contains("Deserializable")
            {
                for impl_item in &mut item_impl.items {
                    if let ImplItem::Fn(method) = impl_item {
                        if method.sig.ident == "versioned_deserialize" {
                            changed |= update_deserialized_fn_impl(method);
                        }
                    }
                }
            }
        }
    }

    return changed;
}

fn process_file<P: AsRef<Path>>(path: P) -> std::io::Result<()> {
    let path = path.as_ref();
    if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("rs") {
        let content = fs::read_to_string(&path)?;
        let mut ast: File = parse_file(&content).expect("Failed to parse file");
        if update_version(&mut ast) {
            println!("Updating file: {:?}", path);
            fs::write(path, ast.into_token_stream().to_string())?;
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
