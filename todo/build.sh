#!/usr/bin/env bash
# Build the Elm frontend into web/elm.js (the directory Tauri bundles).
#
#   ./build.sh                compile src/Main.elm -> web/elm.js
#   ./build.sh --optimize     compile optimized (used for release bundles)
#   ./build.sh --serve        compile, then serve web/ on http://localhost:8753
#
# Tauri runs this as its beforeDevCommand / beforeBuildCommand (see
# src-tauri/tauri.conf.json). You can also run the frontend standalone in a
# browser with --serve. A static server is required (not file://): the page
# loads db.js as an ES module and uses IndexedDB, both blocked on file:// origins.

set -euo pipefail
cd "$(dirname "$0")"

OPTIMIZE=""
for arg in "$@"; do
  [[ "$arg" == "--optimize" ]] && OPTIMIZE="--optimize"
done

echo "› elm make src/Main.elm -> web/elm.js"
elm make src/Main.elm --output=web/elm.js $OPTIMIZE

for arg in "$@"; do
  if [[ "$arg" == "--serve" ]]; then
    echo "› serving http://localhost:8753  (Ctrl-C to stop)"
    cd web && exec python3 -m http.server 8753
  fi
done
