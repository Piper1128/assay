"""Pulls a submission out of a GitHub issue body.

This is the boundary where text a stranger typed becomes a file the rest of
the pipeline reads, so it is a file in the repository rather than a few lines
buried in a workflow: it can be read, reviewed and tested like anything else.

The body arrives through an environment variable and never through shell
interpolation. A body substituted into a `run:` block is executed by the
shell, which makes "open an issue" and "run a command on the runner" the same
act; passing it as data closes that door and is the single most important
thing in this file.

Nothing here decides whether a submission is *true*. It decides whether a
blob of text is a submission at all, and says why not when it isn't. The
judgement stays where ADR-003 put it: a human, reading a pull request.

Usage:
    ISSUE_BODY="$body" python3 tools/from_issue.py out.json
"""

import json
import os
import pathlib
import re
import sys

# The Rust loader refuses anything past a megabyte (submission.rs MAX_BYTES).
# Refusing it here too means an oversized body is answered with a sentence
# instead of a stack trace three steps later.
MAX_BYTES = 1 << 20

FENCE = re.compile(r"```(?:json|jsonc)?\s*\n(.*?)```", re.DOTALL)


def fail(message: str) -> "None":
    """Writes a reason the workflow can post back, and stops.

    The wording is aimed at the person who opened the issue, because they are
    who reads it, and they cannot see this file.
    """
    print(message, file=sys.stderr)
    sys.exit(1)


def extract(body: str) -> str:
    """The submission text inside an issue body.

    A fenced block first, because the page writes one and because a body
    usually has a sentence around it. The whole body second, because someone
    pasting only the JSON has done nothing wrong.
    """
    blocks = FENCE.findall(body)
    for block in blocks:
        stripped = block.strip()
        if stripped.startswith("{"):
            return stripped
    stripped = body.strip()
    if stripped.startswith("{"):
        return stripped
    fail(
        "I could not find a submission in this issue. Paste the JSON the "
        "page copies for you, on its own or inside a ``` code block — the "
        '"Copy a submission" button puts exactly the right text on your '
        "clipboard."
    )


def main() -> "None":
    if len(sys.argv) != 2:
        fail("usage: from_issue.py <out.json>")
    body = os.environ.get("ISSUE_BODY")
    if body is None:
        fail("ISSUE_BODY is not set — the workflow must pass the body as data")
    if len(body.encode("utf-8")) > MAX_BYTES:
        fail(f"this issue is larger than {MAX_BYTES} bytes, which is the most a submission may be")

    text = extract(body)
    try:
        parsed = json.loads(text)
    except json.JSONDecodeError as e:
        fail(f"that is not valid JSON: {e}. Nothing was changed.")

    if not isinstance(parsed, dict):
        fail("a submission is a JSON object, and this is not one.")
    if "submission" not in parsed:
        fail(
            "this JSON has no `submission` field, so it is not a submission "
            "— it may be an item card or a loadout. The page's "
            '"Copy a submission" button writes the right shape.'
        )

    # Written back from the parsed value rather than passed through: whatever
    # reaches the loader is then something this file understood, not a string
    # that merely happened to sit next to something it understood.
    out = pathlib.Path(sys.argv[1])
    out.write_text(json.dumps(parsed, indent=2) + "\n", encoding="utf-8")
    print(f"submission {parsed.get('submission')} from {parsed.get('observer')!r}, "
          f"{len(parsed.get('items', []))} item(s), build {parsed.get('build')!r}")


if __name__ == "__main__":
    main()
