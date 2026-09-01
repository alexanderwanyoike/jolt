#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

required=(
  website/index.html
  website/styles.css
  website/script.js
  website/docs.css
  website/rfcs/index.html
  website/sdk/index.html
  website/sdk/reference.html
  website/guides/app-development.html
  website/guides/data-sdk.html
  website/guides/data-sdk-migrations.html
  website/guides/data-sdk-mutations.html
  website/guides/data-sdk-manual-conflicts.html
  website/guides/data-sdk-subscriptions.html
  website/guides/data-sdk-testing.html
  rfcs/README.md
  sdks/js/docs/api.json
)

for file in "${required[@]}"; do
  test -s "$file" || { echo "missing website file: $file" >&2; exit 1; }
done

required_download_contract=(
  '__VERSION__'
  '__RELEASE_DATE__'
  'https://github.com/alexanderwanyoike/jolt/releases/download/__VERSION__/jolt-console-x86_64.AppImage'
  'https://github.com/alexanderwanyoike/jolt/releases/download/__VERSION__/jolt-console-aarch64.dmg'
  'https://github.com/alexanderwanyoike/jolt/releases/download/__VERSION__/jolt-console-x86_64-setup.exe'
  'xattr -dr com.apple.quarantine "/Applications/Jolt Console.app"'
)

for value in "${required_download_contract[@]}"; do
  grep -Fq "$value" website/index.html || {
    echo "missing website download contract: $value" >&2
    exit 1
  }
done

required_protocol_visual_contract=(
  'Your data<br />should be<br /><em>yours.</em>'
  '<meta property="og:title" content="Jolt | Your data should be yours." />'
  'Jolt nodes exchanging signed path records'
  'SIGNED PATH RECORDS · MOVING BETWEEN NODES'
  '"owner_public_key"'
  '"content_id"'
  '"signature"'
  'bafkr…6f'
  'JOLT NETWORK'
  'SIGNED PATH RECORD'
  'DIRECT OR VIA RELAY'
)

for value in "${required_protocol_visual_contract[@]}"; do
  grep -Fq "$value" website/index.html || {
    echo "missing protocol-accurate hero contract: $value" >&2
    exit 1
  }
done

for value in \
  'signed JSON' \
  'SIGNED JSON' \
  'alice.jolt' \
  'bafy…' \
  'bafkr…9f' \
  'NETWORK LIVE' \
  'private spaces' \
  'community spaces' \
  'whole community space' \
  'Your identity<br />should outlive'; do
  if grep -Fq "$value" website/index.html; then
    echo "misleading protocol hero terminology remains: $value" >&2
    exit 1
  fi
done

grep -Fq "workflows: [\"Package Jolt Console\"]" .github/workflows/pages.yml || {
  echo "Pages workflow does not follow tagged Jolt Console releases" >&2
  exit 1
}

for sdk_docs_path in \
  'sdks/js/docs/api.json' \
  'sdks/js/typedoc.json' \
  'scripts/render-sdk-docs.py'; do
  grep -Fq "\"${sdk_docs_path}\"" .github/workflows/pages.yml || {
    echo "Pages workflow does not verify SDK docs change: $sdk_docs_path" >&2
    exit 1
  }
done

grep -Fq 'gh release view' .github/workflows/pages.yml || {
  echo "Pages workflow does not resolve the latest published release" >&2
  exit 1
}

rfc_count=0
for source in rfcs/[0-9][0-9][0-9][0-9]-*.md; do
  case "$(basename "$source")" in
    0000-*) continue ;;
  esac
  output="website/rfcs/$(basename "${source%.md}.html")"
  test -s "$output" || { echo "missing rendered RFC: $output" >&2; exit 1; }
  rfc_source_sha="$(sha256sum "$source" | cut -d' ' -f1)"
  grep -q "jolt-rfc-source-sha256.*${rfc_source_sha}" "$output" || {
    echo "rendered RFC is stale: $output; run: python3 scripts/render-rfcs.py" >&2
    exit 1
  }
  rfc_count=$((rfc_count + 1))
done

test "$rfc_count" -eq 8 || {
  echo "expected 8 published RFC sources, found $rfc_count" >&2
  exit 1
}

sdk_source_sha="$(sha256sum sdks/js/docs/api.json | cut -d' ' -f1)"
(cd sdks/js && yarn docs >/dev/null)
regenerated_sdk_source_sha="$(sha256sum sdks/js/docs/api.json | cut -d' ' -f1)"
if [[ "$regenerated_sdk_source_sha" != "$sdk_source_sha" ]]; then
  echo "SDK API snapshot is stale: sdks/js/docs/api.json; run: cd sdks/js && yarn docs" >&2
  exit 1
fi
if grep -Fq '"sources":' sdks/js/docs/api.json; then
  echo "SDK API snapshot contains volatile source metadata; enable TypeDoc disableSources" >&2
  exit 1
fi
grep -q "jolt-sdk-source-sha256.*${sdk_source_sha}" website/sdk/reference.html || {
  echo "rendered SDK reference is stale: website/sdk/reference.html; run: python3 scripts/render-sdk-docs.py" >&2
  exit 1
}

for guide_source in guides/*.md; do
  guide_name="$(basename "$guide_source" .md)"
  guide_includes="$(sed -n 's/^@include \([^ ]*\) as .*$/\1/p' "$guide_source")"
  # shellcheck disable=SC2086 # include paths are word-split on purpose
  guide_sha="$(cat "$guide_source" $guide_includes | sha256sum | cut -d' ' -f1)"
  grep -q "jolt-guide-source-sha256.*${guide_sha}" "website/guides/${guide_name}.html" || {
    echo "rendered guide is stale: website/guides/${guide_name}.html; run: python3 scripts/render-guides.py" >&2
    exit 1
  }
done

# These calls expose protocol/session plumbing. Their presence in the rendered
# beginner app is an architectural regression, independent of tutorial wording.
if grep -Eq 'publishAppend|requestSession|createJoltClient' website/guides/app-development.html; then
  echo "beginner Chirp guide leaks low-level SDK setup" >&2
  exit 1
fi

required_sdk_contract=(
  'href="sdk/"'
  'href="guides/app-development.html"'
)

for value in "${required_sdk_contract[@]}"; do
  grep -Fq "$value" website/index.html || {
    echo "website navigation is missing the SDK docs contract: $value" >&2
    exit 1
  }
done

# Strip markup first: syntax highlighting wraps the command in token spans.
# (Process substitution, not a pipe: grep -q quitting early would SIGPIPE
# sed and trip pipefail.)
grep -Fq 'yarn add jolt-sdk' <(sed 's/<[^>]*>//g' website/sdk/index.html) || {
  echo "SDK page does not document the jolt-sdk install command" >&2
  exit 1
}

required_data_sdk_reference=(
  'id="module-data"'
  'jolt-sdk/data'
  'id="data.Schema"'
  'id="data.Schema.parse"'
  'id="data.Schema.migrate"'
  'id="data.Migrations"'
)

for value in "${required_data_sdk_reference[@]}"; do
  grep -Fq "$value" website/sdk/reference.html || {
    echo "SDK reference is missing the Data SDK contract: $value" >&2
    exit 1
  }
done

required_rfc_library_contract=(
  'Protocol series · eight experimental drafts'
  '0001-0008'
  '0002-device-authority.html'
  '0003-device-writer-logs.html'
  '0004-encrypted-device-access.html'
  '0005-community-membership.html'
  '0006-community-app-indexes.html'
  '0007-app-sessions.html'
  '0008-device-writer-extensions.html'
)

for value in "${required_rfc_library_contract[@]}"; do
  grep -Fq "$value" website/index.html website/rfcs/index.html || {
    echo "missing complete RFC library contract: $value" >&2
    exit 1
  }
done

grep -Fq 'https://alexanderwanyoike.github.io/spoke/' website/index.html &&
  grep -Fq 'https://alexanderwanyoike.github.io/pastey/' website/index.html || {
  echo "website does not link to the Spoke and Pastey showcase sites" >&2
  exit 1
}

em_dash="$(printf '\342\200\224')"
for file in website/index.html website/rfcs/index.html website/docs.css \
  website/sdk/index.html website/sdk/reference.html website/guides/*.html \
  scripts/render-rfcs.py scripts/render-sdk-docs.py scripts/verify-website.sh; do
  if grep -Fq "$em_dash" "$file"; then
    echo "house-style em dash remains in $file" >&2
    exit 1
  fi
done

python3 - <<'PY'
import ast
from pathlib import Path
import re

expected = {
    "0001": "Implemented with known gaps",
    "0002": "Implemented v1",
    "0003": "Implemented v1",
    "0004": "Envelope and daemon operations implemented; device-key custody experimental",
    "0005": "Design only",
    "0006": "Design only",
    "0007": "Implemented v0",
    "0008": "Implemented operation levels 2/3 and sync level 2",
}

rows = {}
for line in Path("rfcs/README.md").read_text(encoding="utf-8").splitlines():
    cells = [cell.strip() for cell in line.strip().strip("|").split("|")]
    if len(cells) != 4 or not cells[0].startswith("["):
        continue
    number = cells[0][1:5]
    rows[number] = cells[3]

if rows != expected:
    raise SystemExit(f"canonical RFC implementation statuses differ: {rows!r}")

index = Path("website/rfcs/index.html").read_text(encoding="utf-8")
for number, implementation in expected.items():
    marker = f"Experimental Draft · {implementation}"
    if marker not in index:
        raise SystemExit(f"RFC {number} index badge is not derived from canonical status: {marker}")

renderer_source = Path("scripts/render-rfcs.py").read_text(encoding="utf-8")
renderer_tree = ast.parse(renderer_source)
memo_date_node = next(
    node
    for node in renderer_tree.body
    if isinstance(node, ast.FunctionDef) and node.name == "memo_date"
)
namespace = {"re": re}
exec(compile(ast.Module(body=[memo_date_node], type_ignores=[]), "render-rfcs.py", "exec"), namespace)
synthetic = "Request for Comments: 9999\\nDate: September 2031"
if namespace["memo_date"](synthetic) != "September 2031":
    raise SystemExit("RFC renderer does not derive the publication month from the memo header")
PY

for contract in \
  'Fetched bytes are verified against the requested CID before return or caching' \
  'Equal-height fork detection is not implemented' \
  'sequence-overflow guard' \
  'signed action paths' \
  'empty path segments' \
  'crates/jolt-network/src/fetch_manager.rs' \
  'crates/jolt-network/src/node/'; do
  grep -Fq "$contract" rfcs/0001-core-protocol.md || {
    echo "RFC 0001 implementation contract missing: $contract" >&2
    exit 1
  }
done

if grep -Fq 'crates/jolt-content' rfcs/0001-core-protocol.md; then
  echo "RFC 0001 still references the nonexistent jolt-content crate" >&2
  exit 1
fi

python3 - <<'PY'
from pathlib import Path

text = " ".join(
    " ".join(Path(path).read_text(encoding="utf-8").split())
    for path in (
        "rfcs/0003-device-writer-logs.md",
        "rfcs/0004-encrypted-device-access.md",
        "rfcs/0007-app-sessions.md",
    )
)
for contract in (
    "rejects the whole imported batch",
    "Content AAD does not include the nonce",
    "does not enforce `declared_size`",
    "does not require the session to be active first",
    "not compared in constant time",
):
    if contract not in text:
        raise SystemExit(f"RFC implementation caveat missing: {contract}")
PY

python3 - <<'PY'
from html.parser import HTMLParser
from pathlib import Path
from urllib.parse import urlsplit

root = Path("website")
html_files = list(root.rglob("*.html"))

class Links(HTMLParser):
    def __init__(self):
        super().__init__()
        self.links = []
    def handle_starttag(self, tag, attrs):
        if tag in {"a", "link", "script"}:
            values = dict(attrs)
            target = values.get("href") or values.get("src")
            if target:
                self.links.append(target)

for page in html_files:
    parser = Links()
    parser.feed(page.read_text())
    for link in parser.links:
        split = urlsplit(link)
        if split.scheme or link.startswith(("#", "//")):
            continue
        target = (page.parent / split.path).resolve()
        if split.path.endswith("/") or (target.exists() and target.is_dir()):
            target = target / "index.html"
        if not target.exists():
            raise SystemExit(f"broken local link in {page}: {link}")

print(f"verified {len(html_files)} HTML pages")
PY
