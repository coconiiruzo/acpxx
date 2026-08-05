#!/bin/sh
set -eu

catalog=${1:?catalog JSON path required}
output=${2:?signature envelope path required}
key=${CATALOG_SIGNING_KEY_PEM:?CATALOG_SIGNING_KEY_PEM must name an Ed25519 PEM private key}
key_id=${CATALOG_SIGNING_KEY_ID:-agentmux-catalog-2026-01}
temporary=$(mktemp "${TMPDIR:-/tmp}/agentmux-catalog-signature.XXXXXX")
trap 'rm -f "$temporary"' EXIT HUP INT TERM
openssl pkeyutl -sign -rawin -inkey "$key" -in "$catalog" -out "$temporary"
digest=$(shasum -a 256 "$catalog" | awk '{print $1}')
signature=$(base64 < "$temporary" | tr -d '\n')
python3 - "$output" "$digest" "$key_id" "$signature" <<'PY'
import json
import sys
from pathlib import Path

Path(sys.argv[1]).write_text(json.dumps({
    "schema_version": 1,
    "catalog_sha256": sys.argv[2],
    "signatures": [{
        "key_id": sys.argv[3],
        "algorithm": "ed25519",
        "signature": sys.argv[4],
    }],
}, indent=2) + "\n")
PY
