"""Tests for scripts/scheduled_failure.py.

Run with `python3 -m unittest discover -s scripts -p 'test_*.py'` from the repository root.
"""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

# No __pycache__ beside the script in the working tree.
sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))

import scheduled_failure  # noqa: E402

JOBS = {
    "jobs": [
        {
            "name": "quality",
            "conclusion": "success",
            "steps": [{"name": "Test", "conclusion": "success"}],
        },
        {
            "name": "fuzz",
            "conclusion": "failure",
            "html_url": "https://github.com/o/r/actions/runs/1/job/2",
            "steps": [
                {"name": "Fuzz untrusted headers", "conclusion": "success"},
                {"name": "Fuzz untrusted contract parsers", "conclusion": "failure"},
            ],
        },
        {"name": "scheduled-failure", "conclusion": None, "steps": []},
    ]
}


class BodyTest(unittest.TestCase):
    def test_names_only_the_failing_jobs_and_steps(self) -> None:
        body = scheduled_failure.body(JOBS, "https://github.com/o/r/actions/runs/1", "abc1234")
        self.assertIn("https://github.com/o/r/actions/runs/1", body)
        self.assertIn("abc1234", body)
        self.assertIn("[fuzz](https://github.com/o/r/actions/runs/1/job/2)", body)
        self.assertIn("Fuzz untrusted contract parsers", body)
        self.assertNotIn("Fuzz untrusted headers", body)
        self.assertNotIn("quality", body)

    def test_a_cancelled_or_timed_out_job_counts_as_failing(self) -> None:
        jobs = {"jobs": [{"name": "components", "conclusion": "timed_out", "steps": []}]}
        self.assertIn("components", scheduled_failure.body(jobs, "url", "sha"))

    def test_no_failing_job_still_says_the_run_failed(self) -> None:
        body = scheduled_failure.body({"jobs": []}, "url", "sha")
        self.assertIn("no job reported a failure", body)

    def test_the_title_is_fixed_so_a_new_failure_finds_the_open_issue(self) -> None:
        self.assertEqual(scheduled_failure.TITLE, "Scheduled CI failed")


if __name__ == "__main__":
    unittest.main()
