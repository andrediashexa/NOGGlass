#!/usr/bin/env bash
# Translation parity check: every message key must exist in all locales.
#
# A key that exists in one language and not the others is the most common i18n
# bug, and it is invisible until a visitor lands on the page in the wrong
# language and sees a raw key. CI fails instead.
#
# Uses python3 rather than jq: it is present everywhere this project builds.
set -uo pipefail

MESSAGES_DIR="${1:-crates/nogglass-server/ui/messages}"

if [[ ! -d "$MESSAGES_DIR" ]]; then
  echo "SKIP no message catalogue at $MESSAGES_DIR yet."
  exit 0
fi

python3 - "$MESSAGES_DIR" <<'PY'
import json
import pathlib
import sys

directory = pathlib.Path(sys.argv[1])
source_locale = "en"
locales = ["en", "pt-BR", "es"]

failed = False
catalogues = {}

for locale in locales:
    path = directory / f"{locale}.json"
    if not path.exists():
        print(f"FAIL missing catalogue {path}")
        failed = True
        continue
    try:
        catalogues[locale] = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as error:
        print(f"FAIL {path} is not valid JSON: {error}")
        failed = True

if source_locale not in catalogues:
    print(f"FAIL missing source catalogue {directory / (source_locale + '.json')}")
    raise SystemExit(1)

source_keys = set(catalogues[source_locale])

for locale, catalogue in catalogues.items():
    if locale == source_locale:
        continue
    keys = set(catalogue)
    missing = sorted(source_keys - keys)
    extra = sorted(keys - source_keys)
    if missing:
        print(f"FAIL {locale} is missing keys present in {source_locale}:")
        print("\n".join(f"  - {key}" for key in missing))
        failed = True
    if extra:
        print(f"FAIL {locale} has keys that do not exist in {source_locale}:")
        print("\n".join(f"  - {key}" for key in extra))
        failed = True
    empty = sorted(k for k, v in catalogue.items() if isinstance(v, str) and not v.strip())
    if empty:
        print(f"FAIL {locale} has empty translations:")
        print("\n".join(f"  - {key}" for key in empty))
        failed = True

if not failed:
    print(f"OK {len(locales)} locales share the same {len(source_keys)} keys.")

raise SystemExit(1 if failed else 0)
PY
