import argparse
import io
import json
from pathlib import Path
import sys
from unittest.mock import patch
import zipfile

import ci
import fetch
from test_readbacks import BASE, HEAD, CaptureCase


class ExplicitRuns(CaptureCase):
    def setUp(self):
        super().setUp()
        self.base_run = {"id": 10, "event": "push", "head_branch": "main", "head_sha": BASE,
                         "status": "completed", "conclusion": "success"}
        self.head_run = {"id": 20, "event": "pull_request", "head_branch": "feature", "head_sha": "f" * 40,
                         "status": "completed", "conclusion": "success"}
        self.artifacts = {"10": [{"id": 100, "name": "readbacks-windows-latest", "expired": False}],
                          "20": [{"id": 200, "name": "readbacks-windows-latest", "expired": False}]}
        self.args = argparse.Namespace(repo="owner/repo", base_sha=BASE, candidate_sha=HEAD,
                                       base_run_id="10", candidate_run_id="20", artifact_name="readbacks-windows-latest",
                                       output=self.root / "downloaded")

    def api(self, endpoint):
        prefix = "repos/owner/repo/actions/"
        self.assertTrue(endpoint.startswith(prefix))
        suffix = endpoint[len(prefix):]
        if suffix == "runs/10":
            return self.base_run
        if suffix == "runs/20":
            return self.head_run
        for run_id in ["10", "20"]:
            if suffix == f"runs/{run_id}/artifacts?per_page=100":
                return {"artifacts": self.artifacts[run_id]}
        raise AssertionError(f"unexpected GitHub query: {endpoint}")

    def download(self, command, stdout, check):
        self.assertTrue(check)
        self.assertEqual(command[:2], ["gh", "api"])
        archives = {"repos/owner/repo/actions/artifacts/100/zip": self.base,
                    "repos/owner/repo/actions/artifacts/200/zip": self.head}
        directory = archives[command[2]]
        buffer = io.BytesIO()
        with zipfile.ZipFile(buffer, "w") as archive:
            for path in directory.iterdir():
                archive.writestr(path.name, path.read_bytes())
        stdout.write(buffer.getvalue())

    def run_fetch(self):
        with patch.object(ci, "gh_json", self.api), patch.object(ci.subprocess, "run", self.download):
            return fetch.compare_runs(self.args, self.inventory)

    def test_pr_merge_sha_is_verified_from_tested_manifest(self):
        report = self.run_fetch()
        self.assertTrue(report["passed"])
        self.assertEqual(report["candidate_sha"], HEAD)
        self.assertTrue((self.args.output / "base" / "manifest.json").is_file())
        self.assertEqual(json.loads((self.args.output / "comparison.json").read_text())["base_run_id"], "10")

    def test_latest_main_cannot_replace_requested_base_run(self):
        self.base_run["head_sha"] = "c" * 40
        with self.assertRaisesRegex(ValueError, "exact base"):
            self.run_fetch()

    def test_failed_or_non_main_run_cannot_supply_base(self):
        self.base_run["head_branch"] = "feature"
        with self.assertRaisesRegex(ValueError, "successful main push"):
            self.run_fetch()

    def test_missing_artifact_requires_bootstrap(self):
        self.artifacts["10"] = []
        with self.assertRaisesRegex(ValueError, "bootstrap required"):
            self.run_fetch()

    def test_expired_artifact_requires_bootstrap(self):
        self.artifacts["10"][0]["expired"] = True
        with self.assertRaisesRegex(ValueError, "bootstrap required"):
            self.run_fetch()

    def test_existing_evidence_is_unchanged_by_a_rejected_fetch(self):
        self.args.output.mkdir()
        evidence = self.args.output / "failure.json"
        evidence.write_bytes(b"original evidence")
        arguments = ["fetch.py", "--repo", "owner/repo", "--base-sha", BASE, "--candidate-sha", HEAD,
                     "--base-run-id", "10", "--candidate-run-id", "20", "--artifact-name", "readbacks-windows-latest",
                     "--output", str(self.args.output)]
        with patch.object(sys, "argv", arguments):
            self.assertEqual(fetch.main(), 1)
        self.assertEqual(evidence.read_bytes(), b"original evidence")
