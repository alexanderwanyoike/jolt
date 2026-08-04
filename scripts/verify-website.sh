#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

required=(
  website/index.html
  website/styles.css
  website/script.js
  website/rfcs/index.html
  rfcs/README.md
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
)

for value in "${required_download_contract[@]}"; do
  grep -Fq "$value" website/index.html || {
    echo "missing website download contract: $value" >&2
    exit 1
  }
done

grep -Fq "workflows: [\"Package Jolt Console\"]" .github/workflows/pages.yml || {
  echo "Pages workflow does not follow tagged Jolt Console releases" >&2
  exit 1
}
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

test "$rfc_count" -ge 1 || { echo "no RFC sources found" >&2; exit 1; }

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
