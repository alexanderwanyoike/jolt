#!/usr/bin/env python3
"""Render the canonical Jolt RFC Markdown collection into GitHub Pages."""

from html import escape
from pathlib import Path
import hashlib
import re

import markdown


ROOT = Path(__file__).resolve().parents[1]
SOURCE_DIR = ROOT / "rfcs"
OUTPUT_DIR = ROOT / "website" / "rfcs"


def render(source_path: Path) -> tuple[str, str]:
    source = source_path.read_text(encoding="utf-8")
    source_digest = hashlib.sha256(source.encode("utf-8")).hexdigest()
    header = re.match(
        r"# Jolt Request for Comments (?P<number>\d{4})\n\n## (?P<title>.+?)\n\n"
        r"(?:\x60){3}text\n(?P<memo>.+?)\n(?:\x60){3}\n\n",
        source,
        flags=re.DOTALL,
    )
    if header is None:
        raise SystemExit(f"{source_path.name} header does not match the renderer contract")

    number = header.group("number")
    title = header.group("title")
    memo = header.group("memo")
    status = memo_field(memo, "Status", "Internet-Draft")
    category = memo_field(memo, "Category", "Experimental")
    body = source[header.end():]
    body = body.replace("### Status of This Memo", "## Status of This Memo", 1)
    body = body.replace("### Abstract", "## Abstract", 1)
    body = re.sub(
        r"### Table of Contents\n\n.*?\n\n(?=## 1\.)",
        "",
        body,
        count=1,
        flags=re.DOTALL,
    )

    renderer = markdown.Markdown(
        extensions=["extra", "sane_lists", "toc"],
        extension_configs={"toc": {"permalink": False}},
    )
    article = renderer.convert(body)

    html = f"""<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <meta name="theme-color" content="#0d0f0f" />
    <meta name="description" content="JOLT-RFC-{number}: {escape(title)}" />
    <meta name="jolt-rfc-source-sha256" content="{source_digest}" />
    <title>JOLT-RFC-{number} — {escape(title)}</title>
    <link rel="icon" href="../favicon.svg" type="image/svg+xml" />
    <link rel="stylesheet" href="../styles.css" />
    <link rel="stylesheet" href="rfc.css" />
  </head>
  <body class="rfc-page">
    <aside class="rfc-sidebar">
      <a class="brand" href="../"><img class="brand-mark" src="../favicon.svg" alt="" width="30" height="30" /><span><strong>Jolt</strong><small>RFC {number}</small></span></a>
      <dl>
        <div><dt>Status</dt><dd>{escape(status)}</dd></div>
        <div><dt>Category</dt><dd>{escape(category)}</dd></div>
        <div><dt>Published</dt><dd>August 2026</dd></div>
      </dl>
      <nav aria-label="Document contents">{renderer.toc}</nav>
    </aside>
    <main class="rfc-main">
      <article class="rfc-document">
        <header class="rfc-title">
          <p>JOLT REQUEST FOR COMMENTS · {number}</p>
          <h1>{escape(title)}</h1>
          <pre class="memo-head">{escape(memo)}</pre>
        </header>
        {article}
        <footer class="document-footer">
          <a href="./">← RFC index</a>
          <a href="https://github.com/alexanderwanyoike/jolt/blob/dev/rfcs/{source_path.name}">Canonical Markdown ↗</a>
        </footer>
      </article>
    </main>
  </body>
</html>
"""
    return number, html


def memo_field(memo: str, field: str, default: str) -> str:
    match = re.search(rf"^{re.escape(field)}:\s*(.+)$", memo, flags=re.MULTILINE)
    return match.group(1).strip() if match else default


def main() -> None:
    sources = sorted(
        path for path in SOURCE_DIR.glob("[0-9][0-9][0-9][0-9]-*.md")
        if not path.name.startswith("0000-")
    )
    if not sources:
        raise SystemExit("no RFC sources found")

    OUTPUT_DIR.mkdir(parents=True, exist_ok=True)
    expected_outputs = set()
    for source_path in sources:
        number, html = render(source_path)
        output_path = OUTPUT_DIR / f"{source_path.stem}.html"
        output_path.write_text(html, encoding="utf-8")
        expected_outputs.add(output_path)
        print(f"rendered JOLT-RFC-{number}: {source_path.relative_to(ROOT)} -> {output_path.relative_to(ROOT)}")

    for output_path in OUTPUT_DIR.glob("[0-9][0-9][0-9][0-9]-*.html"):
        if output_path not in expected_outputs:
            output_path.unlink()
            print(f"removed stale RFC render: {output_path.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
