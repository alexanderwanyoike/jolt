#!/usr/bin/env python3
"""Render guides/*.md into website/guides/ pages with highlighted code.

Code blocks come in two forms:

  ```lang optional/file/caption
  literal code
  ```

  @include sdks/js/guide/src/beginner/chirp.ts as src/chirp.ts

`@include` blocks pull the file straight from the repository, so the rendered
guide can never drift from code that is type-checked and tested. The output
carries a jolt-guide-source-sha256 meta tag over the Markdown source plus every
included file, in order; scripts/verify-website.sh recomputes it without any
renderer dependencies.
"""

from html import escape
from pathlib import Path
import hashlib
import re

import markdown
from pygments import highlight
from pygments.formatters import HtmlFormatter
from pygments.lexers import get_lexer_by_name

ROOT = Path(__file__).resolve().parents[1]
SOURCE_DIR = ROOT / "guides"
OUTPUT_DIR = ROOT / "website" / "guides"

# Pygments has no TSX lexer; the TypeScript lexer handles the JSX subset fine.
LEXER_ALIASES = {"ts": "typescript", "tsx": "typescript", "jsx": "typescript"}
CAPTION_LANGS = {"ts": "ts", "tsx": "tsx", "css": "css", "toml": "toml", "json": "json", "rust": "rust"}
FORMATTER = HtmlFormatter(nowrap=True)

# A plain-text marker: python-markdown strips raw STX/ETX control characters
# (it uses them for its own internal placeholders), so this must be ASCII.
PLACEHOLDER = "JOLTCODEBLOCK{index}MARKER"


def highlight_code(code: str, lang: str) -> str:
    if lang == "text":
        return escape(code)
    lexer = get_lexer_by_name(LEXER_ALIASES.get(lang, lang))
    return highlight(code, lexer, FORMATTER).rstrip("\n")


def code_figure(code: str, lang: str, caption: str | None) -> str:
    body = f'<pre class="doc-code"><code>{highlight_code(code, lang)}</code></pre>'
    if caption is None:
        return body
    return (
        '<figure class="code-figure">'
        f'<figcaption><span>{escape(caption)}</span><span>{CAPTION_LANGS.get(lang, lang)}</span></figcaption>'
        f"{body}</figure>"
    )


def render(source_path: Path) -> str:
    source = source_path.read_text(encoding="utf-8")
    digest = hashlib.sha256(source.encode("utf-8"))

    header = re.match(
        r"# (?P<title>.+?)\n\n```meta\n(?P<meta>.+?)\n```\n\n",
        source,
        flags=re.DOTALL,
    )
    if header is None:
        raise SystemExit(f"{source_path.name} header does not match the renderer contract")
    title = header.group("title")
    meta = dict(
        line.split(": ", 1) for line in header.group("meta").splitlines() if ": " in line
    )
    for field in ("Guide", "Description"):
        if field not in meta:
            raise SystemExit(f"{source_path.name} meta block is missing {field}")
    kicker = meta.get("Kicker", "JOLT APP DEVELOPMENT GUIDE")
    facts = {
        key: value
        for key, value in meta.items()
        if key not in ("Guide", "Description", "Kicker")
    }
    body = source[header.end():]

    blocks: list[str] = []

    def stash(html: str) -> str:
        blocks.append(html)
        return PLACEHOLDER.format(index=len(blocks) - 1)

    def replace_include(match: re.Match[str]) -> str:
        include_path = ROOT / match.group(1)
        code = include_path.read_text(encoding="utf-8")
        digest.update(code.encode("utf-8"))
        lang = include_path.suffix.lstrip(".")
        return stash(code_figure(code.rstrip("\n"), lang, match.group(2)))

    def replace_fence(match: re.Match[str]) -> str:
        lang, caption, code = match.group(1), match.group(2), match.group(3)
        return stash(code_figure(code.rstrip("\n"), lang, caption))

    body = re.sub(r"^@include (\S+) as (\S+)$", replace_include, body, flags=re.MULTILINE)
    body = re.sub(
        r"^```(\w+)(?: (\S+))?\n(.*?)^```$",
        replace_fence,
        body,
        flags=re.MULTILINE | re.DOTALL,
    )

    renderer = markdown.Markdown(
        extensions=["extra", "sane_lists", "toc"],
        extension_configs={"toc": {"permalink": False}},
    )
    article = renderer.convert(body)
    for index, block in enumerate(blocks):
        token = PLACEHOLDER.format(index=index)
        article = article.replace(f"<p>{token}</p>", block).replace(token, block)

    lede, _, sections = article.partition("<h2")
    if sections:
        sections = "<h2" + sections
    lede = lede.replace("<p>", '<p class="doc-lede">', 1)

    toc_entries = "".join(
        f'<li class="toc-module"><a href="#{match.group(1)}">{match.group(2)}</a></li>'
        for match in re.finditer(r'<h2 id="([^"]+)">(.+?)</h2>', sections)
    )

    stem = source_path.stem
    return f"""<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <meta name="theme-color" content="#0d0f0f" />
    <meta name="description" content="{escape(meta["Description"])}" />
    <meta property="og:title" content="Jolt Guide | {escape(title)}" />
    <meta property="og:description" content="{escape(meta["Description"])}" />
    <meta property="og:type" content="article" />
    <meta property="og:site_name" content="Jolt" />
    <meta property="og:image" content="https://alexanderwanyoike.github.io/jolt/og-card.png" />
    <meta name="twitter:card" content="summary_large_image" />
    <meta name="jolt-guide-source-sha256" content="{digest.hexdigest()}" />
    <title>Jolt Guide | {escape(title)}</title>
    <link rel="icon" href="../favicon.svg" type="image/svg+xml" />
    <link rel="stylesheet" href="../styles.css" />
    <link rel="stylesheet" href="../docs.css" />
  </head>
  <body class="doc-page">
    <aside class="doc-sidebar">
      <a class="brand" href="../"><img class="brand-mark" src="../favicon.svg" alt="" width="30" height="30" /><span><strong>Jolt</strong><small>guide {escape(meta["Guide"])}</small></span></a>
      <dl>
        {"".join(f'<div><dt>{escape(key)}</dt><dd>{escape(value)}</dd></div>' for key, value in facts.items())}
      </dl>
      <nav aria-label="Guide contents">
        <ul class="toc">{toc_entries}</ul>
      </nav>
    </aside>
    <main class="doc-main">
      <article class="doc-document">
        <header class="doc-title">
          <p>{escape(kicker)} · {escape(meta["Guide"])}</p>
          <h1>{escape(title)}</h1>
          {lede}
        </header>
        {sections}
        <footer class="document-footer">
          <a href="../sdk/">← SDK overview</a>
          <a href="https://github.com/alexanderwanyoike/jolt/blob/dev/guides/{stem}.md">Canonical Markdown ↗</a>
        </footer>
      </article>
    </main>
  </body>
</html>
"""


def main() -> None:
    OUTPUT_DIR.mkdir(parents=True, exist_ok=True)
    for source_path in sorted(SOURCE_DIR.glob("*.md")):
        html = render(source_path)
        output_path = OUTPUT_DIR / f"{source_path.stem}.html"
        output_path.write_text(html, encoding="utf-8")
        print(f"rendered guide: {source_path.relative_to(ROOT)} -> {output_path.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
