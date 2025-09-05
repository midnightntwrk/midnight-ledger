# This file is part of midnight-ledger.
# Copyright (C) 2025 Midnight Foundation
# SPDX-License-Identifier: Apache-2.0
# Licensed under the Apache License, Version 2.0 (the "License");
# You may not use this file except in compliance with the License.
# You may obtain a copy of the License at
# http://www.apache.org/licenses/LICENSE-2.0
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

{ system, rust-build-toolchain, nixpkgs, stdenv }:

{
  name,
  package-name,
  path,
  prefix ? "midnight",
  scope ? "@midnight-ntwrk",
  repo ? "https://github.com/midnight-ntwrk/artifacts",
  version,
  extraBuildInputs ? [],
  extraVariables ? {},
  src,
  debug ? false,
  features ? [],
}:

let
  pkgs = import nixpkgs { inherit system; };
  raw-wasm = ((pkgs.makeRustPlatform {
      rustc = rust-build-toolchain;
      cargo = rust-build-toolchain;
      inherit stdenv;
  }).overrideScope (final: prev: {
    cargoBuildHook = prev.cargoBuildHook.overrideDerivation (_: {
      rustcTarget = "wasm32-unknown-unknown";
    });
    cargoInstallHook = pkgs.hello;
  })).buildRustPackage ({
      pname = name;
      inherit version src;
      cargoLock.lockFile = "${src}/Cargo.lock";
      cargoLock.allowBuiltinFetchGit = true;
      buildType = if debug then "debug" else "wasm";
      cargoBuildFlags = "--package ${prefix}-${name}" + (if features != []
          then " --features ${(builtins.concatStringsSep "," features)}"
          else "");
      nativeBuildInputs = [
        rust-build-toolchain
      ] ++ extraBuildInputs;
      doCheck = false;
      postInstall = ''
        mkdir -p $out
        cp target/wasm32-unknown-unknown/${if debug then "debug" else "wasm"}/*.wasm $out/
      '';
    } // extraVariables);
  name-var = builtins.replaceStrings ["-"] ["_"] name;
in pkgs.stdenvNoCC.mkDerivation {
  inherit name version src;
  buildPhase = ''
    # We take the bundler bindings as the base for an ESM module
    # But make some tweaks to make it more portable.
    wasm-bindgen ${raw-wasm}/${prefix}_${name-var}.wasm --out-dir pkg --target bundler --omit-default-module-path --weak-refs --reference-types --no-typescript ${if debug then "--debug --keep-debug" else ""}
    # Optimize for size
    wasm-opt pkg/${prefix}_${name-var}_bg.wasm -Os --enable-reference-types -o pkg/${prefix}_${name-var}_bg.wasm
    # We copy the hand-crafted .d.ts
    if [ -e "${path}/assemble-dts.js" ]; then
      pushd ${path}
      node assemble-dts.js ${builtins.concatStringsSep " " features}
      popd
    fi
    cp ${path}/${package-name}.d.ts pkg/${package-name}.d.ts
    # We create a manual `package.json` that points to the correct exports.
    cat <<-EOF > pkg/package.json
      {
        "name": "${scope}/${package-name}",
        "version": "${version}",
        "type": "module",
        "files": [
          "${prefix}_${name-var}.js",
          "${prefix}_${name-var}_fs.js",
          "${prefix}_${name-var}_bg.js",
          "${prefix}_${name-var}_bg.wasm",
          "${package-name}.d.ts",
          "snippets"
        ],
        "sideEffects": [
          "./${prefix}_${name-var}.js",
          "./${prefix}_${name-var}_fs.js",
          "./snippets/*"
        ],
        "imports": {
          "#self": {
            "browser": "./${prefix}_${name-var}.js",
            "node": "./${prefix}_${name-var}_fs.js"
          }
        },
        "types": "./${package-name}.d.ts",
        "exports": {
          "types": "./${package-name}.d.ts",
          "browser": "./${prefix}_${name-var}.js",
          "node": "./${prefix}_${name-var}_fs.js"
        },
        "repository": {
          "type": "git",
          "url": "${repo}.git"
        }
      }
    EOF
    # Snippet imports
    snippets_raw=$(echo pkg/snippets/**/*.js)
    snippets=$(echo $snippets_raw | sed -e 's/pkg\/snippets/snippets/g')
    snippet_imports=$(for snippet in $snippets; do
      import=$(echo $snippet | sed -e 's/[-\/.]/_/g')
      echo "import * as $import from './$snippet';"
      echo "imports['./$snippet'] = $import;"
    done)
    # Create the _fs.js node entry point.
    cat <<-EOF > pkg/${prefix}_${name-var}_fs.js
      export * from "./${prefix}_${name-var}_bg.js";
      import * as exports from "./${prefix}_${name-var}_bg.js";
      import { __wbg_set_wasm } from "./${prefix}_${name-var}_bg.js";
      import { readFileSync } from 'fs';
      import { join, dirname } from 'path';
      import { fileURLToPath } from 'url';
      
      let imports = {};
      imports['./${prefix}_${name-var}_bg.js'] = exports;
    EOF
    echo "$snippet_imports" >> pkg/${prefix}_${name-var}_fs.js
    cat <<-EOF >> pkg/${prefix}_${name-var}_fs.js
      
      const __filename = fileURLToPath(import.meta.url);
      const __dirname = dirname(__filename);
      const wasmPath = join(__dirname, '${prefix}_${name-var}_bg.wasm');
      const bytes = readFileSync(wasmPath);
      
      const wasmModule = new WebAssembly.Module(bytes);
      const wasmInstance = new WebAssembly.Instance(wasmModule, imports);
      const wasm = wasmInstance.exports;
      
      __wbg_set_wasm(wasm);
      wasm.__wbindgen_start();
    EOF
    # Create a corresponding package.lock
    cd pkg
    npm install --package-lock-only
    cd ..
  '';
  installPhase = ''
    mkdir -p $out/lib/node_modules/${scope}/${package-name}
    cp -r pkg/* $out/lib/node_modules/${scope}/${package-name}/
    mv pkg package
    tar -czf $out/lib/${prefix}-${package-name}-${version}.tgz package
  '';
  npmDeps = pkgs.importNpmLock.buildNodeModules {
      npmRoot = "${src}/${path}";
      inherit (pkgs) nodejs;
  };
  nativeBuildInputs = [
    raw-wasm
    pkgs.importNpmLock.hooks.linkNodeModulesHook
    pkgs.nodejs
    pkgs.coreutils
    pkgs.gnused
    pkgs.wasm-bindgen-cli_0_2_100
    pkgs.binaryen
  ];
}
