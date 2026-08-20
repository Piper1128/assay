#!/usr/bin/env python3
"""Assembles `ui/assay.html`: one file, no server, no network.

The module is base64'd into the page rather than fetched beside it, because a
page that fetches anything cannot run from `file://` — and running from
`file://` is the requirement. The tool has to be there between raids without
an install, a localhost port, or a connection.

Run after `cargo build -p assay-wasm --release --target wasm32-unknown-unknown`
and `wasm-bindgen --target no-modules --no-typescript --out-dir ui/pkg ...`.
`tools/check.sh` does both.
"""

import base64
import pathlib
import sys

root = pathlib.Path(__file__).resolve().parent.parent
template = root / "ui/template.html"
glue = root / "ui/pkg/assay_wasm.js"
module = root / "ui/pkg/assay_wasm_bg.wasm"
out = root / "ui/assay.html"

missing = [p for p in (template, glue, module) if not p.exists()]
if missing:
    print("build-ui: missing " + ", ".join(str(p.relative_to(root)) for p in missing))
    print("build-ui: run the wasm build first (see this file's docstring)")
    sys.exit(2)

page = template.read_text(encoding="utf-8")
wasm = base64.b64encode(module.read_bytes()).decode("ascii")

# The glue is inserted verbatim, so it must not contain the marker it is
# replacing; likewise the base64 alphabet cannot contain the other marker.
assert "__WASM_B64__" not in glue.read_text(encoding="utf-8"), "glue collides with the marker"
page = page.replace("__GLUE__", glue.read_text(encoding="utf-8"), 1)
page = page.replace("__WASM_B64__", wasm, 1)
assert "__GLUE__" not in page and "__WASM_B64__" not in page, "a marker survived"

out.write_text(page, encoding="utf-8")
size = out.stat().st_size
print(f"build-ui: wrote {out.relative_to(root)} ({size // 1024} KiB, module {len(wasm) // 1024} KiB base64)")
