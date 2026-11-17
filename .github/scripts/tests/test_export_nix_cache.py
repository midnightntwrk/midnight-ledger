# Copyright (C) Midnight Foundation
# SPDX-License-Identifier: Apache-2.0

import importlib.util
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location(
    "export_nix_cache", Path(__file__).resolve().parents[1] / "export-nix-cache.py"
)
cache = importlib.util.module_from_spec(spec)
spec.loader.exec_module(cache)


class ExportTests(unittest.TestCase):
    def test_only_built_project_directories_are_selected(self):
        with tempfile.TemporaryDirectory() as directory:
            for name, expected in [
                ("zkir-static-x86_64-unknown-linux-musl-2.2.0", True),
                ("midnight-zkir-3.2.1", True),
                ("midnight-local-params-10", True),
                ("ledger-wasm", True),
                ("ledger-static-x86_64-unknown-linux-musl-1.0.2", True),
                ("rust-stable-with-std-2026-04-16", False),
                ("source", False),
                ("cargo-audit-0.22.0-vendor", False),
            ]:
                path = Path(directory) / ("a" * 32 + "-" + name)
                path.mkdir()
                self.assertEqual(cache.reusable(path), expected, name)
            drv = Path(directory) / ("a" * 32 + "-zkir-2.2.0.drv")
            drv.touch()
            self.assertFalse(cache.reusable(drv))

    def test_failed_export_preserves_previous_cache(self):
        with tempfile.TemporaryDirectory() as directory:
            old = Path(directory) / "nix-ci-cache"
            old.mkdir()
            (old / "sentinel").write_text("valid cache")
            with patch.dict(os.environ, {"RUNNER_TEMP": directory}), patch.object(
                cache.subprocess, "check_output", return_value="/nix/store/root\n"
            ), patch.object(cache, "reusable", return_value=True), patch.object(
                cache.subprocess, "run", side_effect=subprocess.CalledProcessError(1, "nix")
            ):
                with self.assertRaises(subprocess.CalledProcessError):
                    cache.main()
            self.assertEqual((old / "sentinel").read_text(), "valid cache")

    def test_successful_export_discards_stale_outputs(self):
        with tempfile.TemporaryDirectory() as directory:
            old = Path(directory) / "nix-ci-cache"
            old.mkdir()
            (old / "stale.narinfo").touch()

            def export(*args, **kwargs):
                staging = Path(directory) / "nix-ci-cache-next"
                staging.mkdir()
                (staging / "current.narinfo").write_text("current")

            with patch.dict(os.environ, {"RUNNER_TEMP": directory}), patch.object(
                cache.subprocess, "check_output", return_value="/nix/store/root\n"
            ), patch.object(cache, "reusable", return_value=True), patch.object(
                cache.subprocess, "run", side_effect=export
            ):
                cache.main()
            self.assertFalse((old / "stale.narinfo").exists())
            self.assertTrue((old / "current.narinfo").exists())
