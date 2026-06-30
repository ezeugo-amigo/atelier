#!/usr/bin/env bash
# Build the Elm frontend into web/elm.js.

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
    echo "› serving http://localhost:8754  (Ctrl-C to stop)"
    cd web && exec python3 -m http.server 8754
  fi
done
