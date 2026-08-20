#!/usr/bin/env bash
# Thin wrapper so `tools/check.sh` can call the UI build like every other
# step. The build itself lives in Python, because the machine this tool is
# used on does not necessarily have a working bash.
set -eu
cd "$(dirname "$0")/.."
python tools/build-ui.py
