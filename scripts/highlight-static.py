#!/usr/bin/env python3
"""Syntax-highlight code blocks in hand-authored website pages.

Rendered pages (the SDK reference, guides) are highlighted by their
renderers; hand-authored pages opt in by marking a block

    <pre class="doc-code"><code data-lang="ts">...</code></pre>

and running this script after editing the code. It is idempotent: existing
highlight spans are stripped and entities unescaped before re-highlighting,
so it can be re-run on already-highlighted pages.
"""

from html import escape, unescape
from pathlib import Path
import re

from pygments import highlight
from pygments.formatters import HtmlFormatter
from pygments.lexers import get_lexer_by_name

ROOT = Path(__file__).resolve().parents[1]
PAGES = [ROOT / "website" / "sdk" / "index.html"]

# Pygments has no TSX lexer; the TypeScript lexer handles the JSX subset fine.
LEXER_ALIASES = {"ts": "typescript", "tsx": "typescript", "jsx": "typescript"}
FORMATTER = HtmlFormatter(nowrap=True)

BLOCK = re.compile(
    r'(<pre class="doc-code"><code data-lang="(?P<lang>\w+)">)(?P<body>.*?)(</code></pre>)',
    flags=re.DOTALL,
)


def highlight_block(match: re.Match[str]) -> str:
    lang = match.group("lang")
    code = unescape(re.sub(r"</?span[^>]*>", "", match.group("body")))
    if lang == "text":
        body = escape(code)
    else:
        lexer = get_lexer_by_name(LEXER_ALIASES.get(lang, lang))
        body = highlight(code, lexer, FORMATTER).rstrip("\n")
    return f"{match.group(1)}{body}{match.group(4)}"


def main() -> None:
    for page in PAGES:
        source = page.read_text(encoding="utf-8")
        updated, count = BLOCK.subn(highlight_block, source)
        page.write_text(updated, encoding="utf-8")
        print(f"highlighted {count} blocks: {page.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
