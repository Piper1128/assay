#!/usr/bin/env python3
"""Builds `ui/assay.html`: one file, no server, no network.

Runs the whole chain — cargo, wasm-bindgen, then the assembly — so the page
can be rebuilt with one command from any shell. Python rather than a shell
script because the project already depends on Python for the mirror, and
because `bash` is not a given on the machine this tool is actually used on.

The module is base64'd into the page rather than fetched beside it: a page
that fetches anything cannot run from `file://`, and running from `file://` is
the requirement. The tool has to be there between raids without an install, a
localhost port, or a connection.

    python tools/build-ui.py            build it
    python tools/build-ui.py --open     build it and open it
"""

import base64
import pathlib
import shutil
import subprocess
import sys

root = pathlib.Path(__file__).resolve().parent.parent
template = root / "ui/template.html"
pkg = root / "ui/pkg"
glue = pkg / "assay_wasm.js"
module = pkg / "assay_wasm_bg.wasm"
wasm = root / "target/wasm32-unknown-unknown/release/assay_wasm.wasm"
out = root / "ui/assay.html"


def run(what, *args):
    """Runs one build step, and says which one failed rather than which file
    is missing three steps later."""
    if shutil.which(args[0]) is None:
        sys.exit(f"build-ui: {args[0]} is not on PATH — needed for {what}")
    done = subprocess.run(args, cwd=root, check=False)
    if done.returncode != 0:
        sys.exit(f"build-ui: {what} failed")


run(
    "compiling the resolver for the browser",
    "cargo",
    "build",
    "--quiet",
    "-p",
    "assay-wasm",
    "--release",
    "--target",
    "wasm32-unknown-unknown",
)
# The `wasm-bindgen` CLI and the crate must be the same version; the crate is
# pinned exactly so the two cannot drift apart without saying so.
run(
    "generating the browser bindings",
    "wasm-bindgen",
    "--target",
    "no-modules",
    "--no-typescript",
    "--out-dir",
    str(pkg),
    str(wasm),
)

missing = [p for p in (template, glue, module) if not p.exists()]
if missing:
    sys.exit("build-ui: missing " + ", ".join(str(p.relative_to(root)) for p in missing))

page = template.read_text(encoding="utf-8")
glue_js = glue.read_text(encoding="utf-8")
encoded = base64.b64encode(module.read_bytes()).decode("ascii")

# The glue goes in verbatim, so it must not contain the marker it replaces.
assert "__WASM_B64__" not in glue_js, "the glue collides with the marker"
page = page.replace("__GLUE__", glue_js, 1).replace("__WASM_B64__", encoded, 1)
assert "__GLUE__" not in page and "__WASM_B64__" not in page, "a marker survived"

out.write_text(page, encoding="utf-8")
print(f"build-ui: {out} ({out.stat().st_size // 1024} KiB)")

if "--open" in sys.argv:
    if sys.platform == "win32":
        subprocess.run(["cmd", "/c", "start", "", str(out)], check=False)
    else:
        subprocess.run(["xdg-open", str(out)], check=False)
