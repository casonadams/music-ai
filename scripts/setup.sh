#!/usr/bin/env bash
# Clone the pinned ACE-Step backend and install its Python environment.
set -euo pipefail

REPO=https://github.com/ACE-Step/ACE-Step-1.5.git
REV=ca1e85fe9430179831e6bc6be790c332190a3866
DIR="$(cd "$(dirname "$0")/.." && pwd)/ACE-Step-1.5"

if [ ! -d "$DIR/.git" ]; then
  git clone "$REPO" "$DIR"
fi
git -C "$DIR" fetch --quiet origin "$REV"
git -C "$DIR" checkout --quiet "$REV"
(cd "$DIR" && uv sync)
