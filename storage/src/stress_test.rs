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

//! Stress testing.
//!
//! We want to run the stress tests in a separate process, so that we can
//! monitor their resource usage and kill them if necessary, instead of OOMing
//! or hanging. Unlike Haskell, Rust doesn't support async exceptions, so there is
//! no easy way to kill threads without their cooperation. Processes, OTOH, can
//! be killed at the OS level.
//!
//! Also, by running the stress tests in a separate process, we can build them
//! in "release" mode, even when testing is being done in "debug" mode. Perhaps
//! this could cause some confusion, but the motivation is that stress tests
//! should be demanding, and so compiling with optimization may be more
//! realistic.
//!
//! # How it works
//!
//! A stress test is a normal `#[test]` test, that calls
//! `runner::StressTest::run(<test name>)`, where `<test name>` is a *public*
//! `fn()` which is registered with the stress test binary `stress` defined in
//! `:storage/bin/stress.rs`. For an example, see this module:
//!
//! - the `pub` sub-module `stress_tests` defines the `fn()`s which are
//!   registered with the `stresss` binary. See `:storage/bin/stress.rs` for how
//!   that is done.
//!
//! - the `#[cfg(test)]` sub-module `tests` defines the actual stress tests,
//!   which run the `fn()`s from `stress_tests` inside the `stress` binary using
//!   the `runner::StressTest::run`

/// A stress test runner for use in `#[test]` tests.
///
/// This calls the `stress` binary in a subprocess, monitoring resource usage
/// and killing the stress test if resource bounds are exceeded.
#[cfg(test)]
pub(crate) mod runner {
    use std::{
        io::Read as _,
        process::{Command, ExitStatus},
    };

    /// Stress testing configuration.
    pub(crate) struct StressTest {
        nocapture: bool,
        max_memory_bytes: u64,
        max_runtime_seconds: u64,
    }

    /// Builder style interface to stress testing.
    impl StressTest {
        /// Create a stress tester with default limits.
        pub(crate) fn new() -> Self {
            // Set `nocapture` iff test was run with `cargo test -- --nocapture`.
            let nocapture = std::env::args().any(|arg| arg == "--nocapture");
            let max_memory_bytes = 1 << 30; // 1gb
            let max_runtime_seconds = 10;
            Self {
                nocapture,
                max_memory_bytes,
                max_runtime_seconds,
            }
        }

        /// Override `nocapture`, which by default is inferred from the
        /// presence/lack of presence of the `--nocapture` flag.
        ///
        /// This can be useful if you want to mark your stress test with
        /// `#[should_panic = <some msg>]`, where `<some msg>` includes output
        /// from the failed test, since we can only report the output of the
        /// failed test when its captured.
        pub(crate) fn with_nocapture(mut self, nocapture: bool) -> Self {
            self.nocapture = nocapture;
            self
        }

        /// Set max memory in bytes.
        pub(crate) fn with_max_memory(mut self, max_memory_bytes: u64) -> Self {
            self.max_memory_bytes = max_memory_bytes;
            self
        }

        /// Set max run-time in seconds.
        ///
        /// Whole seconds only, because the underlying `sysinfo` lib we use for
        /// timing only supported full-second resolution.
        pub(crate) fn with_max_runtime(mut self, max_runtime_seconds: u64) -> Self {
            self.max_runtime_seconds = max_runtime_seconds;
            self
        }

        /// Call `run_with_args` with empty arguments.
        pub(crate) fn run(&self, test_name: &str) {
            self.run_with_args(test_name, &[]);
        }

        /// Run stress test `test_name`.
        ///
        /// Monitors resource usage and kills the test if usage exceeds limits.
        ///
        /// The runner is careful to respect the `--nocapture` setting, and
        /// simulate the output behavior of running `cargo test` directly:
        ///
        /// - with `--nocapture`, the test output run in the subprocess appears
        ///   on stdout and stderr as its produced.
        ///
        /// - without `--nocapture`, the test output is redirected to a single
        ///   stream, and then printed to stdout after the test finishes, if the
        ///   test fails. Note that this preserves the relative ordering /
        ///   interleaving of stdout and stderr.
        pub(crate) fn run_with_args(&self, test_name: &str, args: &[&str]) {
            let pkg_name = env!("CARGO_PKG_NAME");
            let bin_name = "stress";

            println!("{test_name}: building stress-test runner ...");
            // Make sure the stress test runner is already built. We don't want the
            // build time here to be clocked against our time budget.
            let build_succeeded = Command::new("cargo")
                .arg("build")
                .arg("-p")
                .arg(pkg_name)
                .arg("--all-features")
                // Note that the `command` below also assumes "release" here, so
                // if you change this, then change that too. Otherwise, building
                // the stress tester could count against the clock-time of the
                // test itself.
                .arg("--release")
                .arg("--quiet")
                .args(["--bin", bin_name])
                .spawn()
                .unwrap()
                .wait()
                .unwrap()
                .success();
            assert!(build_succeeded, "failed to build stress test runner");
            println!("{test_name}: done building, running stress test ...");

            // Run the actual stress test.
            let mut command = Command::new("cargo");
            command
                .arg("run")
                .arg("-p")
                .arg(pkg_name)
                .arg("--all-features")
                .arg("--release")
                .arg("--quiet")
                .args(["--bin", bin_name])
                .args(["--", test_name])
                .args(args);
            let mut maybe_reader = None;
            if !self.nocapture {
                let (reader, writer) = os_pipe::pipe().unwrap();
                maybe_reader = Some(reader);
                let writer_clone = writer.try_clone().unwrap();
                // The child process output is not captured by the test harness,
                // unlike output we print here. We would like this to behave the
                // same as the test's own output, i.e. be shown on failure or if
                // `--nocapture` is used, but hidden otherwise. So, if
                // `--nocapture` is not specified, we capture the output and
                // print it directly below, which makes the test harness capture
                // it.
                //
                // We combine the subprocess stdout and stderr on a single pipe,
                // to preserve their interleave ordering. This is the same thing
                // `cargo test` does.
                command.stdout(writer).stderr(writer_clone);
            }
            // Implicitly starting the clock now, because we use the OS's
            // measure of the process's runtime.
            let mut child = command
                .spawn()
                .unwrap_or_else(|e| panic!("failed to run stress tester: {e:?}"));
            // Avoid deadlock, see example code here: https://docs.rs/os_pipe/latest/os_pipe/#examples.
            drop(command);

            // Initialize resource monitoring.
            let mut system = sysinfo::System::new_all();
            let pid = sysinfo::Pid::from_u32(child.id());

            // Run the test to completion, or until resource exhaustion,
            // classifying the outcome.
            enum Outcome {
                Exit(ExitStatus),
                OutOfMemory,
                OutOfTime,
            }
            let outcome = loop {
                std::thread::sleep(std::time::Duration::from_millis(100));

                // Check for completion.
                if let Some(status) = child.try_wait().unwrap() {
                    break Outcome::Exit(status);
                }

                // Update process stats.
                let remove_dead = true;
                system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), remove_dead);
                let process = system.process(pid).unwrap();

                // Kill on excessive memory use.
                let memory_usage_bytes = process.memory();
                if memory_usage_bytes > self.max_memory_bytes {
                    assert!(process.kill());
                    break Outcome::OutOfMemory;
                }

                // Kill on excessive run time.
                let runtime_seconds = process.run_time();
                if runtime_seconds > self.max_runtime_seconds {
                    assert!(process.kill());
                    break Outcome::OutOfTime;
                }
            };

            // Read subprocess output. Since we don't read the pipe till the
            // subprocess is done, a subprocess which produces a lot of output
            // could block on a full pipe. It will then fail by timeout above,
            // giving an erroneous timeout outcome classification. If we ever
            // need to handle this case, one option is to read the subprocess
            // output concurrently in another thread, instead of waiting till
            // the end here (or equivalently, run the monitoring loop in another
            // thread).
            let mut output = String::new();
            if !self.nocapture {
                maybe_reader
                    .as_ref()
                    .unwrap()
                    .read_to_string(&mut output)
                    .unwrap();
                print!("{}", output.clone());
            }

            // Panic if the outcome was not success. Note that some tests use
            // `#[should_panic = <error message>]` for the below error messages,
            // so fix those tests if you change these error messages.
            //
            // Alternatively, we could return something like `Result<(),
            // ErrorOutcome>` and let the caller match on that (we'd want to
            // keep the success outcome separate, so that the common use case
            // would be `self::run(...).unwrap()`).
            match outcome {
                Outcome::Exit(status) => {
                    if !status.success() {
                        let msg = if self.nocapture {
                            String::from("stress test failed, but its output was not captured")
                        } else {
                            format!("stress test failed: {}", output)
                        };
                        panic!("{msg}")
                    }
                }
                Outcome::OutOfMemory => {
                    panic!("memory usage exceeded limit, killing stress test...")
                }
                Outcome::OutOfTime => panic!("runtime exceeded limit, killing stress test..."),
            }
        }
    }
}

/// Stress tests, to be registered in the stress test runner `:storage/bin/stress.rs`.
pub mod stress_tests {
    /// Test `run` with succeeding test.
    pub fn run_pass() {}

    /// Test `run` with too much memory usage.
    pub fn run_fail_oom() {
        let mut string = String::from("uh oh");
        loop {
            string.push_str(&string.clone());
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }

    /// Test `run` with timeout.
    pub fn run_fail_timeout() {
        loop {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }

    /// Test that when output is not captured, the subprocess stdout and stderr
    /// are properly interleaved. Correctness means that 1,2,3,4 will occur in
    /// the output on consecutive lines, when run without `--nocapture`.
    pub fn run_fail_capture_interleave() {
        // Interleave printing to stdout and stderr.
        println!("1");
        eprintln!("2");
        println!("3");
        eprintln!("4");
        // Fail so that `cargo test` will print the output after capturing it.
        panic!();
    }

    /// Test that we can match on the output of a failed test using
    /// `#[should_panic = <output>]`.
    pub fn run_fail_match_output() {
        panic!("billions of bilious blue blistering barnacles!");
    }
}

#[cfg(test)]
mod tests {
    use super::runner::StressTest;

    #[test]
    fn run_pass() {
        StressTest::new().run("stress_test::stress_tests::run_pass");
    }

    #[test]
    #[should_panic = "memory usage exceeded limit"]
    fn run_fail_oom() {
        let one_gb = 1 << 30;
        StressTest::new()
            .with_max_memory(one_gb)
            .run("stress_test::stress_tests::run_fail_oom");
    }

    #[test]
    #[should_panic = "runtime exceeded limit"]
    fn run_fail_timeout() {
        StressTest::new()
            .with_max_runtime(5)
            .run("stress_test::stress_tests::run_fail_timeout");
    }

    /// Test that subprocess interleaving is correct in capture mode. The goal
    /// here is check that the output of `cargo test` is correct, so we invoke
    /// `cargo test` on this test in the `testception` test below.
    #[test]
    #[ignore = "testception()"]
    fn run_fail_capture_interleave() {
        StressTest::new()
            // The behavior we're testing only happens when output is captured.
            .with_nocapture(false)
            .run("stress_test::stress_tests::run_fail_capture_interleave");
    }

    /// Use `cargo test` to run `run_fail_capture_interleave` and check that the
    /// numbers 1,2,3,4 occur on consecutive lines in the output 🙃
    ///
    /// This tests that the stress-test runner correctly captures test output
    /// the way `cargo test` would if the test were run directly.
    #[test]
    fn testception() {
        use std::process::Command;
        let pkg_name = env!("CARGO_PKG_NAME");
        let output = Command::new("cargo")
            .arg("test")
            .arg("-p")
            .arg(pkg_name)
            .args(["--features", "stress-test"])
            .arg("stress_test::tests::run_fail_capture_interleave")
            .args(["--", "--include-ignored"])
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let canary = "1\n2\n3\n4\n";
        if !stdout.contains(canary) {
            panic!(
                "subprocess output doesn't contain the canary:\n<stdout>\n{}</stdout>\n<stderr>\n{}</stderr>",
                stdout, stderr
            );
        }
    }

    #[test]
    #[should_panic = "billions of bilious blue blistering barnacles!"]
    fn run_fail_match_output() {
        StressTest::new()
            // The behavior we're testing only happens when output is captured.
            .with_nocapture(false)
            .run("stress_test::stress_tests::run_fail_match_output");
    }
}
