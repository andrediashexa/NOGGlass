#!/usr/bin/env bash
# Translation parity check: every message key must exist in all locales.
# No-op while the frontend does not exist yet.
set -uo pipefail

MESSAGES_DIR="${1:-frontend/messages}"
LOCALES=("en" "pt-BR" "es")
SOURCE_LOCALE="en"

if [[ ! -d "$MESSAGES_DIR" ]]; then
  echo "SKIP no message catalogue at $MESSAGES_DIR yet."
  exit 0
fi

command -v jq >/dev/null 2>&1 || { echo "jq is required"; exit 2; }

keys_of() { jq -r 'paths(scalars) | join(".")' "$1" | sort; }

src="$MESSAGES_DIR/$SOURCE_LOCALE.json"
[[ -f "$src" ]] || { echo "FAIL missing source catalogue $src"; exit 1; }

failed=0
for locale in "${LOCALES[@]}"; do
  file="$MESSAGES_DIR/$locale.json"
  if [[ ! -f "$file" ]]; then
    echo "FAIL missing catalogue $file"
    failed=1
    continue
  fi
  jq empty "$file" 2>/dev/null || { echo "FAIL $file is not valid JSON"; failed=1; continue; }
  [[ "$locale" == "$SOURCE_LOCALE" ]] && continue

  missing=$(comm -23 <(keys_of "$src") <(keys_of "$file"))
  extra=$(comm -13 <(keys_of "$src") <(keys_of "$file"))
  if [[ -n "$missing" ]]; then
    echo "FAIL $locale is missing keys present in $SOURCE_LOCALE:"
    echo "$missing" | sed 's/^/  - /'
    failed=1
  fi
  if [[ -n "$extra" ]]; then
    echo "FAIL $locale has keys that do not exist in $SOURCE_LOCALE:"
    echo "$extra" | sed 's/^/  - /'
    failed=1
  fi
done

[[ $failed -eq 0 ]] && echo "OK all locales share the same key set."
exit $failed
