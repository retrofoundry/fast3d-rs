import argparse
import json
from pathlib import Path
import subprocess
import sys

import ci
from readbacks import compare


def compare_runs(args, inventory):
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    base_run = ci.gh_json(f"repos/{args.repo}/actions/runs/{args.base_run_id}")
    candidate_run = ci.gh_json(f"repos/{args.repo}/actions/runs/{args.candidate_run_id}")
    for run, run_id in [(base_run, args.base_run_id), (candidate_run, args.candidate_run_id)]:
        if str(run["id"]) != str(run_id) or run["status"] != "completed":
            raise ValueError(f"requested run {run_id} is not complete")
    if base_run["event"] != "push" or base_run["head_branch"] != "main" or base_run["conclusion"] != "success":
        raise ValueError("base artifact must come from a successful main push")
    if base_run["head_sha"] != args.base_sha:
        raise ValueError("requested run does not test the exact base SHA")
    if candidate_run["event"] not in ["pull_request", "push"]:
        raise ValueError("candidate must be a PR or push validation run")
    (output / "workflow-runs.json").write_text(json.dumps({"base": base_run, "candidate": candidate_run}, indent=2) + "\n")
    ci.download_artifact(args.repo, args.base_run_id, args.artifact_name, output / "base")
    ci.download_artifact(args.repo, args.candidate_run_id, args.artifact_name, output / "candidate")
    report = compare(output / "base", output / "candidate", inventory, args.base_sha, args.candidate_sha,
                     args.base_run_id, args.candidate_run_id)
    report["candidate_run_conclusion"] = candidate_run["conclusion"]
    (output / "comparison.json").write_text(json.dumps(report, indent=2) + "\n")
    return report


def main():
    parser = argparse.ArgumentParser(description="Download explicit base/PR runs and compare exact RGBA.")
    parser.add_argument("--repo", required=True)
    parser.add_argument("--base-sha", required=True)
    parser.add_argument("--candidate-sha", required=True)
    parser.add_argument("--base-run-id", required=True)
    parser.add_argument("--candidate-run-id", required=True)
    parser.add_argument("--artifact-name", required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        report = compare_runs(args, ci.INVENTORY)
    except FileExistsError as error:
        print(error, file=sys.stderr)
        return 1
    except (ValueError, KeyError, OSError, subprocess.CalledProcessError) as error:
        args.output.mkdir(parents=True, exist_ok=True)
        report = {"passed": False, "bootstrap_required": True, "error": str(error)}
        (args.output / "failure.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report))
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    sys.exit(main())
