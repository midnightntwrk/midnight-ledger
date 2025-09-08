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

{
  description = "Midnight ledger prototype";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    utils.url = "github:numtide/flake-utils";
    fenix.url = "github:nix-community/fenix";
    inclusive.url = "github:input-output-hk/nix-inclusive";
    #compactc = {
    #  url = "github:midnightntwrk/compactc";
    #  inputs.zkir.follows = "zkir";
    #  inputs.onchain-runtime.follows = "";
    #};
    zkir = {
      url = "github:midnightntwrk/midnight-ledger/dde9ed5570893a1e382bf87859469fb44b99d1d2";
      # Have the self-recursion just be a fixpoint.
      inputs.zkir.follows = "zkir";
    };
  };

  outputs = {
    self,
    nixpkgs,
    utils,
    fenix,
    inclusive,
    #compactc,
    zkir,
    ...
  }:
    utils.lib.eachDefaultSystem (
      system: let
        pkgs = nixpkgs.legacyPackages.${system};
        pkgsStatic = pkgs.pkgsStatic;
        mkShell = pkgs.mkShell.override {
          stdenv = pkgs.clangStdenv;
        };
        isDarwin = pkgs.lib.hasSuffix "-darwin" system;
        rustWorkspaceSrc = inclusive.lib.inclusive ./. [
          ./Cargo.toml
          ./Cargo.lock
          ./static
          ./zswap
          ./ledger
          ./ledger-wasm
          ./proof-server
          ./storage
          ./zkir
          ./base-crypto-derive
          ./base-crypto
          ./transient-crypto
          ./coin-structure
          ./serialize
          ./onchain-vm
          ./onchain-state
          ./onchain-runtime
          ./onchain-runtime-wasm
          ./generate-cost-model
          ./rustfmt.toml
          ./wasm-proving-demos/zkir-mt
        ];
        rust = fenix.packages.${system};
        bagel-wasm = (import ./bagel.nix) {
          inherit system nixpkgs;
          stdenv = pkgs.clangStdenv;
          inherit (self.packages.${system}) rust-build-toolchain;
        };
        contractSrc = inclusive.lib.inclusive ./. [./zswap/zswap.compact ./ledger/dust.compact ./zkir-precompiles];
        rust-build = self.packages.${system}.rust-build-toolchain;
        ledger-version = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).workspace.package.version;
        zswap-version = (builtins.fromTOML (builtins.readFile ./zswap/Cargo.toml)).package.version;
        proof-server-version = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).workspace.package.version;
        mkWasm = { name, package-name, require-artifacts ? false, features ? [], experimental ? false }:
          self.lib.${system}.bagel-wasm {
            inherit name package-name features;
            path = name;
            src = rustWorkspaceSrc;
            version = (builtins.fromTOML (builtins.readFile ./${name}/Cargo.toml)).package.version
              + (if features != [] then "+" + (builtins.concatStringsSep "," features) else "");
            extraVariables = {
              # clang doesn't support 'zerocallusedregs' for wasm, but nix tries to set it
              # anyway. The stack protector tries to pull in OS code that doesn't exist.
              hardeningDisable = ["zerocallusedregs" "stackprotector"];
            } // (if require-artifacts then {
              MIDNIGHT_PP = "${self.packages.${system}.local-params}";
            } else {}) // (if experimental then {
              MIDNIGHT_LEDGER_EXPERIMENTAL = 1;
            } else {});
            extraBuildInputs = (if require-artifacts then
              [
                self.packages.${system}.local-params
                zkir.packages.${system}.zkir
              ] else [])
              ++ (
                if isDarwin
                then [pkgs.darwin.apple_sdk.frameworks.SystemConfiguration]
                else []
              );
          };
        mkLedger = {
          isCrossArm ? false,
          heavy-checks ? false,
          build-target ? null,
        }: (pkgs.makeRustPlatform {
            rustc = self.packages.${system}.rust-build-toolchain;
            cargo = self.packages.${system}.rust-build-toolchain;
          }).buildRustPackage (rec {
              pname = "ledger";
              version = ledger-version;
              src = rustWorkspaceSrc;
              cargoLock.lockFile = ./Cargo.lock;
              cargoLock.allowBuiltinFetchGit = true;

              CARGO_BUILD_TARGET = {
                "x86_64-linux" = "x86_64-unknown-linux-musl";
                "x86_64-darwin" = "x86_64-apple-darwin";
                "aarch64-linux" = "aarch64-unknown-linux-musl";
                "aarch64-darwin" = "aarch64-apple-darwin";
              }.${if isCrossArm then "aarch64-linux" else system};

              checkPhase = ''
                cargo fmt -- --check
                cargo check --all-targets --workspace
                cargo test ${if heavy-checks then "--release --target" else ""}
              '';
              cargoBuildFlags = (if build-target != null then "--package ${build-target} " else "") + "--target ${CARGO_BUILD_TARGET}";

              MIDNIGHT_PP = "${self.packages.${system}.local-params}";
              MIDNIGHT_LEDGER_TEST_STATIC_DIR =
                if heavy-checks
                then "${self.packages.${system}.test-artifacts}"
                else "";
              RUST_BACKTRACE = "full";
              nativeBuildInputs =
                [
                  self.packages.${system}.local-params
                  zkir.packages.${system}.zkir
                  rust-build
                  pkgs.chez
                ]
                ++ (
                  if isDarwin
                  then [pkgs.darwin.apple_sdk.frameworks.SystemConfiguration]
                  else []
                );

              doCheck = true;
            }
            // (
              if isCrossArm
              then {
                depsBuildBuild = [
                  pkgs.pkgsCross.aarch64-multiplatform.stdenv.cc
                ];
                # blst uses CC rather than the 'real' compiler, nudge it into
                # the right direction.
                preBuild = ''
                  export CC=$CC_AARCH64_UNKNOWN_LINUX_MUSL
                '';
                CC_AARCH64_UNKNOWN_LINUX_MUSL = "${pkgs.pkgsCross.aarch64-multiplatform-musl.stdenv.cc}/bin/aarch64-unknown-linux-musl-cc";
                CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER = "${pkgs.pkgsCross.aarch64-multiplatform-musl.stdenv.cc}/bin/aarch64-unknown-linux-musl-cc";
                doCheck = false;
                buildInputs = [pkgs.pkgsCross.aarch64-multiplatform.stdenv.cc];
              }
              else {}
            ));

        mkDocker = isCrossArm:
          with if isCrossArm
          then pkgs.pkgsCross.aarch64-multiplatform-musl
          else pkgs.pkgsCross.musl64; let
            proof-server = mkLedger {
              inherit isCrossArm;
              build-target = "midnight-proof-server";
            };
          in
            dockerTools.buildImage {
              name = "ghcr.io/midnight-ntwrk/proof-server";
              tag = "${proof-server-version}-${
                if isCrossArm
                then "arm64"
                else "amd64"
              }";
              copyToRoot = [
                # When we want tools in /, we need to symlink them in order to
                # still have libraries in /nix/store. This behavior differs from
                # dockerTools.buildImage but this allows to avoid having files
                # in both / and /nix/store.
                (pkgs.buildEnv {
                  name = "root";
                  paths = [
                    bashInteractive
                    coreutils
                  ];
                  pathsToLink = ["/bin"];
                })
                dockerTools.caCertificates
              ];

              config = {
                entrypoint = ["${bashInteractive}/bin/bash" "-c"];
                Cmd = ["${proof-server}/bin/midnight-proof-server --port $PORT"];
                Env = [
                  "PATH=${lib.makeBinPath [
                    proof-server
                  ]}"
                  "PORT=6300"
                ];
                ExposedPorts = {
                  "6300/tcp" = {};
                };
              };
            };
      in
        rec {
          lib.bagel-wasm = bagel-wasm;
          packages.rust-build-toolchain = rust.combine [
            rust.stable.rustc
            rust.targets.wasm32-unknown-unknown.stable.rust-std
            rust.targets.aarch64-unknown-linux-musl.stable.rust-std
            rust.targets.aarch64-unknown-linux-gnu.stable.rust-std
            rust.targets.x86_64-unknown-linux-musl.stable.rust-std
            rust.targets.x86_64-unknown-linux-gnu.stable.rust-std
            rust.complete.cargo
            rust.complete.rustfmt
            rust.stable.clippy
          ];
          packages.rust-dev-toolchain = rust.combine [
            rust.stable.rustc
            rust.targets.wasm32-unknown-unknown.stable.rust-std
            rust.targets.aarch64-unknown-linux-musl.stable.rust-std
            rust.targets.aarch64-unknown-linux-gnu.stable.rust-std
            rust.targets.x86_64-unknown-linux-musl.stable.rust-std
            rust.targets.x86_64-unknown-linux-gnu.stable.rust-std
            rust.complete.cargo
            rust.complete.rustfmt
            rust.stable.clippy
            rust.stable.rust-docs
            rust.stable.rust-src
            rust.stable.rust-analyzer
          ];
          packages.rust-doc-toolchain = rust.combine [
            rust.complete.rustc
            rust.complete.cargo
          ];
          packages.proof-server-version = proof-server-version;

          packages.default = pkgs.symlinkJoin {
            name = "ledger-all";
            paths = [
              packages.ledger
              packages.proof-server
              packages.onchain-runtime-wasm
              packages.ledger-wasm
              packages.zkir-wasm
            ];
          };

          packages.test-artifacts = pkgs.stdenvNoCC.mkDerivation {
            pname = "midnight-ledger-test-artifacts";
            version = ledger-version;
            src = inclusive.lib.inclusive ./zkir-precompiles [./zkir-precompiles];
            #src = inclusive.lib.inclusive ./ledger/tests
            #   [ledger/tests/fallible.compact ledger/tests/micro-dao.compact
            #    ledger/tests/simple-merkle-tree.compact
            #    ledger/tests/composable-inner.compact
            #    ledger/tests/composable-outer.compact
            #    ledger/tests/composable-relay.compact
            #    ledger/tests/composable-burn.compact];
            MIDNIGHT_PP = "${packages.public-params}";
            #COMPACT_PATH = "${compactc.packages.${system}.compactc-no-runtime}/lib";
            nativeBuildInputs = [
              packages.public-params
              zkir.packages.${system}.zkir
              #compactc.packages.${system}.compactc-no-runtime
            ];
            buildPhase = ''
              #for file in *.compact; do
              #  fname="$(basename -s .compact "$file")"
              #  compactc "$file" "$fname"
              #done
              for contract in *; do
                mv "$contract" "$contract-tmp"
                mkdir -p "$contract/keys"
                mv $contract-tmp "$contract/zkir"
                zkir compile-many "$contract/zkir" "$contract/keys"
              done
            '';
            installPhase = ''
              mkdir $out
              #for file in *.compact; do
              #  file=$(basename -s .compact "$file")
              #  cp -a "$file" "$out/$file"
              #done
              for contract in *; do
                cp -a "$contract" "$out/$contract"
              done
            '';
          };

          packages.local-params = pkgs.stdenvNoCC.mkDerivation rec {
            pname = "midnight-local-params";
            version = builtins.readFile static/version;
            src = contractSrc;
            MIDNIGHT_PP = "${packages.public-params}";
            #COMPACT_PATH = "${compactc.packages.${system}.compactc-no-runtime}/lib";
            nativeBuildInputs = [
              packages.public-params
              zkir.packages.${system}.zkir
              #compactc.packages.${system}.compactc-no-runtime
              pkgs.coreutils
            ];
            buildPhase = ''
              mkdir -p zswap/zkir
              mkdir -p zswap/keys
              cp zkir-precompiles/zswap/* zswap/zkir
              zkir compile-many zswap/zkir zswap/keys
              #compactc --no-communications-commitment zswap/zswap.compact zswap
              for file in zswap/keys/* zswap/zkir/*; do
                sha256sum "$file" > "$file.sha256"
              done
              mkdir -p dust/zkir
              mkdir -p dust/keys
              cp zkir-precompiles/dust/* dust/zkir
              zkir compile-many dust/zkir dust/keys
              #compactc --no-communications-commitment ledger/dust.compact dust
              for file in dust/keys/* dust/zkir/*; do
                sha256sum "$file" > "$file.sha256"
              done
            '';
            installPhase = ''
              mkdir -p $out/zswap/${version}
              mkdir -p $out/dust/${version}
              cp -a $MIDNIGHT_PP/* $out/
              cp -a zswap/zkir/* $out/zswap/${version}/
              cp -a zswap/keys/* $out/zswap/${version}/
              cp -a dust/zkir/* $out/dust/${version}/
              cp -a dust/keys/* $out/dust/${version}/
            '';
          };

          packages.ledger = mkLedger {};

          packages.onchain-runtime-wasm = mkWasm { name = "onchain-runtime-wasm"; package-name = "onchain-runtime-v1"; };

          packages.ledger-wasm = mkWasm { name = "ledger-wasm"; package-name = "ledger-v6"; require-artifacts = true; };
          packages.zkir-wasm = mkWasm { name = "zkir-wasm"; package-name = "zkir-v2"; require-artifacts = true; };

          # For now, that's the only binary output
          packages.proof-server = mkLedger {build-target = "midnight-proof-server";};

          packages.proof-server-oci = mkDocker false;

          packages.proof-server-oci-arm64 = mkDocker true;

          packages.zkir = ({
              "x86_64-linux" = pkgsStatic;
              "x86_64-darwin" = pkgs;
              "aarch64-linux" = pkgsStatic;
              "aarch64-darwin" = pkgs;
          }.${system}.makeRustPlatform {
            rustc = packages.rust-build-toolchain;
            cargo = packages.rust-build-toolchain;
          }).buildRustPackage rec {
              pname = "zkir";
              version = (builtins.fromTOML (builtins.readFile ./zkir/Cargo.toml)).package.version;
              src = rustWorkspaceSrc;
              cargoLock.lockFile = ./Cargo.lock;
              cargoLock.allowBuiltinFetchGit = true;

              MIDNIGHT_PP = "${packages.public-params}";

              buildInputs = [
                packages.public-params
              ];
              cargoBuildFlags = "--package zkir --features binary";
              nativeBuildInputs = [
                packages.rust-build-toolchain
              ];
              doCheck = false;
            };
          packages.public-params = let
              param-for = k: "https://midnight-s3-fileshare-dev-eu-west-1.s3.eu-west-1.amazonaws.com/bls_filecoin_2p${builtins.toString k}";
          in pkgs.stdenvNoCC.mkDerivation {
            pname = "midnight-testing-public-parameters";
            version = "0.1.0";

            srcs = [
              (pkgs.fetchurl { url = param-for 10; hash = "sha256-0aNAPB+Gaegu0o2TkeEwEa6naAGyj+FLQr920UG076I="; })
              (pkgs.fetchurl { url = param-for 11; hash = "sha256-tQR/BYANvYT9HqQ7lqiFDhKLelle0TLNcliMwssUayk="; })
              (pkgs.fetchurl { url = param-for 12; hash = "sha256-syeRd1r1//GuXq1oLD2IMpF+uwZStDz4EKHjlW6yenE="; })
              (pkgs.fetchurl { url = param-for 13; hash = "sha256-ua9DiSw8uQMh+gCjbl5ZBR81bfFF1/WDaFMfKNISk3s="; })
              (pkgs.fetchurl { url = param-for 14; hash = "sha256-SSPlp/u3Fdgc21wDucDiEXaNNczFLYL0nD2TvPjTalY="; })
              (pkgs.fetchurl { url = param-for 15; hash = "sha256-Fi+sDPcLmwLgIZXsNwE8BJl7OdwYMal9WoP0epzjnJc="; })
              (pkgs.fetchurl { url = param-for 16; hash = "sha256-TrwNB3/mZF6bfKZWMhe+IXbwDf45zJez9g7LrTVz+XM="; })
              (pkgs.fetchurl { url = param-for 17; hash = "sha256-cijEUZ6W7OLFS/L1N9nyaw7QQoGXM3JmI/q14X6sQ2A="; })
            ];

            dontUnpack = true;

            buildPhase = "";

            installPhase = ''
              mkdir $out
              for src in $srcs; do
                name=$(echo $src | sed -e 's/^.*-//')
                cp $src $out/$name
              done
              ls -lh $out
            '';
          };
          apps.doc = let
            script = pkgs.writeShellScript "generate-docs" ''
              export PATH="${pkgs.lib.makeBinPath [
                packages.rust-doc-toolchain
                pkgs.stdenv
              ]}:$PATH"
              export MIDNIGHT_PP="${packages.local-params}";
              export RUSTDOCFLAGS="--cfg nix_doc"
              export MIDNIGHT_LEDGER_EXPERIMENTAL=1
              cargo doc --all-features -Zunstable-options --keep-going "$@"
            '';
          in {
            type = "app";
            program = "${script}";
          };

          apps.proof-server = {
            type = "app";
            program = "${packages.proof-server}/bin/midnight-proof-server";
          };

          devShells.ci = mkShell {
            inputsFrom = with packages; [ledger];
            packages = [
              fenix.packages.${system}.minimal.toolchain
              pkgs.nodejs_22
              pkgs.yarn
              pkgs.jq
              pkgs.cargo-hack
              pkgs.wasm-pack
              pkgs.git
              pkgs.cargo-spellcheck
            ];
            buildInputs = [packages.public-params];
            MIDNIGHT_PP = "${packages.local-params}";
            MIDNIGHT_LEDGER_TEST_STATIC_DIR = "${packages.test-artifacts}";
          };

          devShells.nightly-rust = mkShell {
            inputsFrom = with packages; [ledger];
            packages = [
              fenix.packages.${system}.minimal.toolchain
              pkgs.nodejs_22
              pkgs.yarn
              pkgs.jq
              #compactc.packages.${system}.compactc-no-runtime
              pkgs.cargo-hack
              pkgs.wasm-pack
            ];
            buildInputs = [packages.public-params];
            MIDNIGHT_PP = "${packages.local-params}";
            MIDNIGHT_LEDGER_TEST_STATIC_DIR = "${packages.test-artifacts}";
            MIDNIGHT_LEDGER_EXPERIMENTAL = 1;
            #COMPACT_PATH = "${compactc.packages.${system}.compactc-no-runtime}/lib";
          };

          devShells.default = mkShell {
            inputsFrom = with packages; [ledger];
            packages = [
              packages.rust-dev-toolchain
              pkgs.nodejs_22
              pkgs.yarn
              pkgs.jq
              pkgs.yq
              pkgs.clang
              #compactc.packages.${system}.compactc-no-runtime
              pkgs.cargo-hack
              pkgs.wasm-pack
              pkgs.wasm-bindgen-cli_0_2_100
              pkgs.cargo-spellcheck
            ];
            buildInputs = [packages.public-params];

            # This is required to build blst for wasm. This will not affect
            # Native build outputs of this flake, though it does make native
            # builds *from this devshell* marginally less secure.
            # The stack protector tries to pull in OS code that doesn't exist.
            hardeningDisable = ["zerocallusedregs" "stackprotector"];

            MIDNIGHT_PP = "${packages.local-params}";
            MIDNIGHT_LEDGER_TEST_STATIC_DIR = "${packages.test-artifacts}";
            MIDNIGHT_LEDGER_EXPERIMENTAL = 1;
            #COMPACT_PATH = "${compactc.packages.${system}.compactc-no-runtime}/lib";
            shellHook = ''
              rm -rf .build
              mkdir -p .build
            '';
            #  ln -s ${compactc}/compiler/midnight-ledger.ss .build/midnight-ledger.ss
            #'';
            #MIDNIGHT_LEDGER_ADT_DECL = "${compactc}/compiler/midnight-ledger.ss";
          };
        }
    );
}
