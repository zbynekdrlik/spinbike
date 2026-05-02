#!/usr/bin/env python3
"""Sanity check for the `Mutation Testing (UI)` CI gate.

Detects the silent-no-op pattern: non-test code in `spinbike-ui/src/`
changed in this PR but cargo-mutants found 0 candidates — likely the
wasm32 test runner is broken (the case PR #41 fell into).

Block-aware: lines INSIDE a `#[cfg(test)] mod tests { ... }` block are
correctly excluded, unlike a single-pass grep that only excludes lines
literally containing `#[cfg(test)]`.

Usage:
    python3 .github/scripts/sanity_check_ui_mutants.py <base_ref>

The base_ref is the PR's base branch (e.g. `main`).
"""

from __future__ import annotations

import re
import subprocess
import sys


def find_test_block_ranges(content: str) -> list[tuple[int, int]]:
    """Return inclusive 1-indexed (start_line, end_line) ranges inside any
    `#[cfg(test)]` block (mod / impl / fn).

    Uses simple brace tracking. Brace characters inside string literals,
    char literals, or comments would be miscounted, but Rust's `#[cfg(test)]`
    blocks live in code, not literals; this is good enough in practice.
    """
    lines = content.splitlines()
    ranges: list[tuple[int, int]] = []
    i = 0
    while i < len(lines):
        if lines[i].lstrip().startswith("#[cfg(test)]"):
            # Find the line with the opening brace.
            j = i
            while j < len(lines) and "{" not in lines[j]:
                j += 1
            if j >= len(lines):
                i += 1
                continue
            depth = 0
            start_line = i + 1  # 1-indexed
            cursor = j
            done = False
            while cursor < len(lines) and not done:
                for ch in lines[cursor]:
                    if ch == "{":
                        depth += 1
                    elif ch == "}":
                        depth -= 1
                        if depth == 0:
                            ranges.append((start_line, cursor + 1))
                            i = cursor + 1
                            done = True
                            break
                cursor += 1
            if not done:
                # Unbalanced — treat rest of file as test.
                ranges.append((start_line, len(lines)))
                i = len(lines)
        else:
            i += 1
    return ranges


def parse_added_lines(diff_output: str) -> dict[str, set[int]]:
    """Parse `git diff --unified=0` output → {filename: {post_image_line_no}}."""
    result: dict[str, set[int]] = {}
    current_file: str | None = None
    current_post_line = 0
    hunk_re = re.compile(r"^@@ -\d+(?:,\d+)? \+(\d+)(?:,\d+)? @@")
    for line in diff_output.split("\n"):
        if line.startswith("+++ b/"):
            current_file = line[6:]
            result.setdefault(current_file, set())
            continue
        if line.startswith("+++") or line.startswith("---"):
            continue
        m = hunk_re.match(line)
        if m:
            current_post_line = int(m.group(1))
            continue
        if line.startswith("+") and current_file is not None:
            result[current_file].add(current_post_line)
            current_post_line += 1
    return result


def count_nontest_added_lines(base_ref: str) -> int:
    diff = subprocess.check_output(
        [
            "git",
            "diff",
            "--unified=0",
            f"origin/{base_ref}...HEAD",
            "--",
            "spinbike-ui/src/",
        ],
        text=True,
    )
    added = parse_added_lines(diff)
    total = 0
    for filepath, line_numbers in added.items():
        if not filepath.endswith(".rs") or not line_numbers:
            continue
        try:
            head_content = subprocess.check_output(
                ["git", "show", f"HEAD:{filepath}"], text=True
            )
        except subprocess.CalledProcessError:
            # Newly added file (no HEAD version yet against base) — count all
            # added lines as non-test, conservative.
            for line_no in line_numbers:
                total += 1
            continue
        test_ranges = find_test_block_ranges(head_content)
        head_lines = head_content.splitlines()
        for line_no in line_numbers:
            if any(s <= line_no <= e for s, e in test_ranges):
                continue
            actual = head_lines[line_no - 1].strip() if line_no <= len(head_lines) else ""
            if not actual or actual.startswith("//"):
                continue
            total += 1
    return total


def count_mutants() -> int:
    proc = subprocess.run(
        [
            "cargo",
            "mutants",
            "--list",
            "--in-diff",
            "pr.diff",
            "--manifest-path",
            "spinbike-ui/Cargo.toml",
            "--",
            "--target",
            "wasm32-unknown-unknown",
        ],
        capture_output=True,
        text=True,
    )
    return sum(1 for line in proc.stdout.splitlines() if line.strip())


def main() -> int:
    if len(sys.argv) < 2:
        print("usage: sanity_check_ui_mutants.py <base_ref>", file=sys.stderr)
        return 2
    base_ref = sys.argv[1]
    nontest = count_nontest_added_lines(base_ref)
    mutants = count_mutants()
    print(f"Sanity check: {nontest} non-test src/ lines changed, {mutants} mutants.")
    if nontest > 0 and mutants == 0:
        print(
            "::error::Non-test spinbike-ui/src/ code changed but cargo mutants found 0 candidates.\n"
            "This likely means the wasm32 test runner is broken. Investigate before merging.",
            file=sys.stderr,
        )
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
