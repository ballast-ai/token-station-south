#!/usr/bin/env python3
"""Writes the issue text for a failed scheduled CI run (stream parser first-error record F3).

    python3 scripts/scheduled_failure.py <jobs.json> <run url> <commit sha>

`jobs.json` is the run's `GET /repos/{repo}/actions/runs/{run_id}/jobs` response. The script prints
the issue body: the run, the commit, and each failing job with its failing steps. The workflow
then comments on the open issue titled [`TITLE`], or opens one. The weekly fuzz job failed five
weeks running before anyone saw it, because a failed scheduled run notifies only the account that
last changed the schedule.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

TITLE = "Scheduled CI failed"
_FAILING = {"failure", "timed_out", "cancelled", "startup_failure"}


def body(jobs: dict, run_url: str, sha: str) -> str:
    """The issue body for one failed scheduled run."""
    lines = [f"The scheduled run {run_url} on `{sha}` failed.", ""]
    failing = [job for job in jobs.get("jobs", []) if job.get("conclusion") in _FAILING]
    if not failing:
        lines.append("The run failed, but no job reported a failure; see the run.")
    for job in failing:
        name = job.get("name", "unnamed job")
        url = job.get("html_url")
        lines.append(f"- [{name}]({url}) ({job['conclusion']})" if url else f"- {name} ({job['conclusion']})")
        for step in job.get("steps", []):
            if step.get("conclusion") in _FAILING:
                lines.append(f"  - {step.get('name', 'unnamed step')}")
    return "\n".join(lines) + "\n"


def main() -> int:
    if len(sys.argv) != 4:
        print(__doc__.strip().splitlines()[2].strip(), file=sys.stderr)
        return 2
    jobs = json.loads(Path(sys.argv[1]).read_text())
    print(body(jobs, sys.argv[2], sys.argv[3]), end="")
    return 0


if __name__ == "__main__":
    sys.exit(main())
