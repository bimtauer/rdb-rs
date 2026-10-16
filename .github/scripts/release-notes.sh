#!/usr/bin/env bash
# Print the CHANGELOG.md section for version $1 (without its heading).
# Fails if the section is missing or empty.
set -euo pipefail

version="$1"
notes=$(awk -v heading="## [${version}]" '
    index($0, heading) == 1 { found = 1; next }
    found && /^## \[/ { exit }
    found { print }
' "$(dirname "$0")/../../CHANGELOG.md")

if [ -z "$(printf '%s' "$notes" | tr -d '[:space:]')" ]; then
    echo "CHANGELOG.md has no section for ${version}" >&2
    exit 1
fi
printf '%s\n' "$notes"
