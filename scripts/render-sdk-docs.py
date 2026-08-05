#!/usr/bin/env python3
"""Render the committed typedoc JSON for jolt-sdk into the website SDK reference.

Reads sdks/js/docs/api.json (the committed `yarn docs` output) and writes
website/sdk/reference.html in the site's hand-written style. The output embeds
a jolt-sdk-source-sha256 meta tag so scripts/verify-website.sh can detect a
stale render, mirroring the RFC renderer contract.
"""

from html import escape
from pathlib import Path
import hashlib
import json
import re

from pygments import highlight
from pygments.formatters import HtmlFormatter
from pygments.lexers import get_lexer_by_name

# Pygments has no TSX lexer; the TypeScript lexer handles the JSX subset fine.
_LEXER_ALIASES = {"ts": "typescript", "tsx": "typescript", "jsx": "typescript"}
_FORMATTER = HtmlFormatter(nowrap=True)


def highlight_code(code: str, lang: str) -> str:
    if lang == "text":
        return escape(code)
    lexer = get_lexer_by_name(_LEXER_ALIASES.get(lang, lang))
    return highlight(code, lexer, _FORMATTER).rstrip("\n")

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "sdks" / "js" / "docs" / "api.json"
OUTPUT = ROOT / "website" / "sdk" / "reference.html"

MODULE_ORDER = ["index", "transport-http", "transport-tauri", "testing"]

MODULE_IMPORTS = {
    "index": "jolt-sdk",
    "transport-http": "jolt-sdk/transport-http",
    "transport-tauri": "jolt-sdk/transport-tauri",
    "testing": "jolt-sdk/testing",
}

# The committed api.json predates the rename to the unscoped npm name; the
# published package is `jolt-sdk`, so every displayed occurrence is rewritten.
LEGACY_PACKAGE_NAME = "@jolt/sdk"
PACKAGE_NAME = "jolt-sdk"

KIND_NAMESPACE = 4
KIND_FUNCTION = 64
KIND_CLASS = 128
KIND_INTERFACE = 256
KIND_CONSTRUCTOR = 512
KIND_PROPERTY = 1024
KIND_METHOD = 2048
KIND_TYPE_ALIAS = 2097152
KIND_REEXPORT = 4194304

KIND_LABELS = {
    KIND_NAMESPACE: "namespace",
    KIND_FUNCTION: "function",
    KIND_CLASS: "class",
    KIND_INTERFACE: "interface",
    KIND_TYPE_ALIAS: "type",
    KIND_REEXPORT: "re-export",
}


def load_project() -> tuple[dict, str]:
    raw = SOURCE.read_bytes()
    digest = hashlib.sha256(raw).hexdigest()
    return json.loads(raw), digest


# ---------------------------------------------------------------------------
# Anchors: node id -> #fragment, so {@link} tags and reference types resolve.
# ---------------------------------------------------------------------------

class Anchors:
    def __init__(self) -> None:
        self.by_id: dict[int, str] = {}

    def register(self, node: dict, anchor: str) -> None:
        """Point this node and every unregistered descendant id at `anchor`."""

        def walk(value: object) -> None:
            if isinstance(value, dict):
                node_id = value.get("id")
                if isinstance(node_id, int) and node_id not in self.by_id:
                    self.by_id[node_id] = anchor
                for child in value.values():
                    walk(child)
            elif isinstance(value, list):
                for child in value:
                    walk(child)

        walk(node)

    def resolve(self, target: object) -> str | None:
        if isinstance(target, int):
            return self.by_id.get(target)
        return None


def member_anchor(module_name: str, *names: str) -> str:
    return ".".join([module_name, *names])


def collect_anchors(modules: list[dict]) -> Anchors:
    anchors = Anchors()
    for module in modules:
        for member in module.get("children", []):
            anchor = member_anchor(module["name"], member["name"])
            if member["kind"] == KIND_NAMESPACE:
                for nested in member.get("children", []):
                    anchors.register(
                        nested, member_anchor(module["name"], member["name"], nested["name"])
                    )
            elif member["kind"] in (KIND_CLASS, KIND_INTERFACE):
                for nested in member.get("children", []):
                    # Only callable members render their own headings; property
                    # links roll up to the owning class or interface.
                    if nested["kind"] in (KIND_CONSTRUCTOR, KIND_METHOD):
                        anchors.register(nested, f"{anchor}.{nested['name']}")
            anchors.register(member, anchor)
    return anchors


# ---------------------------------------------------------------------------
# TSDoc comments: typedoc splits them into parts; join back to markdown-ish
# text, then render the small subset of markdown the SDK sources use.
# ---------------------------------------------------------------------------

def comment_markdown(comment: dict | None, anchors: Anchors) -> str:
    if not comment:
        return ""
    pieces: list[str] = []
    for part in comment.get("summary", []):
        kind = part.get("kind")
        if kind == "text":
            pieces.append(part.get("text", ""))
        elif kind == "code":
            pieces.append(part.get("text", ""))
        elif kind == "inline-tag" and part.get("tag") == "@link":
            text = part.get("text", "")
            anchor = anchors.resolve(part.get("target"))
            if anchor:
                pieces.append(f"[`{text}`](#{anchor})")
            else:
                pieces.append(f"`{text}`")
        else:
            pieces.append(part.get("text", ""))
    return "".join(pieces).strip()


def render_inline(text: str) -> str:
    out = escape(text)
    out = re.sub(
        r"\[`([^`\]]+)`\]\((#[^)]+)\)",
        r'<a href="\2"><code>\1</code></a>',
        out,
    )
    out = re.sub(r"\[([^\]]+)\]\((#[^)]+)\)", r'<a href="\2">\1</a>', out)
    out = re.sub(r"`([^`]+)`", r"<code>\1</code>", out)
    out = re.sub(r"\*\*([^*]+)\*\*", r"<strong>\1</strong>", out)
    return out


def render_markdown(md: str) -> str:
    if not md:
        return ""
    html: list[str] = []
    for chunk in re.split(r"(```[\s\S]*?```)", md):
        if not chunk.strip():
            continue
        if chunk.startswith("```"):
            lang = re.match(r"^```(\w*)", chunk).group(1) or "ts"
            body = re.sub(r"^```[^\n]*\n", "", chunk)
            body = re.sub(r"\n?```$", "", body)
            html.append(f'<pre class="doc-code"><code>{highlight_code(body, lang)}</code></pre>')
            continue
        for block in re.split(r"\n\s*\n", chunk):
            lines = [line for line in block.splitlines() if line.strip()]
            if not lines:
                continue
            for tag, marker in (("ul", r"^\s*[-*]\s+"), ("ol", r"^\s*\d+\.\s+")):
                if re.match(marker, lines[0]):
                    items: list[str] = []
                    for line in lines:
                        if re.match(marker, line):
                            items.append(re.sub(marker, "", line))
                        else:
                            items[-1] += " " + line.strip()
                    rendered = "".join(f"<li>{render_inline(item)}</li>" for item in items)
                    html.append(f"<{tag}>{rendered}</{tag}>")
                    break
            else:
                html.append(f"<p>{render_inline(' '.join(lines))}</p>")
    return "".join(html)


def summary_html(node: dict, anchors: Anchors) -> str:
    return render_markdown(comment_markdown(node.get("comment"), anchors))


# ---------------------------------------------------------------------------
# Types -> escaped HTML strings (with links to internal anchors).
# ---------------------------------------------------------------------------

def render_type(t: dict | None, anchors: Anchors) -> str:
    if t is None:
        return "void"
    kind = t.get("type")
    if kind == "intrinsic":
        return escape(t["name"])
    if kind == "literal":
        value = t.get("value")
        if value is None:
            return "null"
        if isinstance(value, str):
            return escape(json.dumps(value))
        return escape(json.dumps(value))
    if kind == "reference":
        name = escape(t["name"])
        anchor = None if t.get("refersToTypeParameter") else anchors.resolve(t.get("target"))
        rendered = f'<a href="#{anchor}">{name}</a>' if anchor else name
        args = t.get("typeArguments")
        if args:
            inner = ", ".join(render_type(a, anchors) for a in args)
            rendered += f"&lt;{inner}&gt;"
        return rendered
    if kind == "union":
        return " | ".join(render_type(part, anchors) for part in t["types"])
    if kind == "intersection":
        return " &amp; ".join(render_type(part, anchors) for part in t["types"])
    if kind == "array":
        element = render_type(t["elementType"], anchors)
        if t["elementType"].get("type") in {"union", "intersection"}:
            element = f"({element})"
        return f"{element}[]"
    if kind == "typeOperator":
        return f"{escape(t['operator'])} {render_type(t['target'], anchors)}"
    if kind == "reflection":
        return render_reflection(t.get("declaration", {}), anchors)
    return escape(str(kind))


def render_reflection(declaration: dict, anchors: Anchors) -> str:
    signatures = declaration.get("signatures")
    children = declaration.get("children")
    if signatures and not children:
        sig = signatures[0]
        params = ", ".join(render_param(p, anchors) for p in sig.get("parameters", []))
        return f"({params}) =&gt; {render_type(sig.get('type'), anchors)}"
    if children:
        parts = []
        for child in children:
            optional = "?" if child.get("flags", {}).get("isOptional") else ""
            if child.get("signatures"):
                parts.append(
                    f"{escape(child['name'])}{optional}: "
                    f"{render_reflection(child, anchors)}"
                )
            else:
                parts.append(
                    f"{escape(child['name'])}{optional}: "
                    f"{render_type(child.get('type'), anchors)}"
                )
        return "{ " + "; ".join(parts) + " }"
    return "{}"


def render_param(param: dict, anchors: Anchors) -> str:
    optional = "?" if param.get("flags", {}).get("isOptional") or "defaultValue" in param else ""
    return f"{escape(param['name'])}{optional}: {render_type(param.get('type'), anchors)}"


def render_type_params(sig: dict) -> str:
    tps = sig.get("typeParameters")
    if not tps:
        return ""
    return "&lt;" + ", ".join(escape(tp["name"]) for tp in tps) + "&gt;"


# ---------------------------------------------------------------------------
# Members -> article HTML.
# ---------------------------------------------------------------------------

def signature_line(sig: dict, anchors: Anchors, name: str | None = None, prefix: str = "") -> str:
    shown = escape(name if name is not None else sig["name"])
    params = ", ".join(render_param(p, anchors) for p in sig.get("parameters", []))
    return (
        f'<pre class="api-signature"><code>{prefix}{shown}{render_type_params(sig)}'
        f"({params}): {render_type(sig.get('type'), anchors)}</code></pre>"
    )


def params_table(sig: dict, anchors: Anchors) -> str:
    params = sig.get("parameters", [])
    if not params:
        return ""
    rows = []
    for param in params:
        notes = []
        summary = comment_markdown(param.get("comment"), anchors)
        if summary:
            notes.append(render_inline(" ".join(summary.split())))
        if param.get("flags", {}).get("isOptional"):
            notes.append("optional")
        if "defaultValue" in param:
            notes.append(f"defaults to <code>{escape(str(param['defaultValue']))}</code>")
        rows.append(
            "<tr>"
            f"<td><code>{escape(param['name'])}</code></td>"
            f"<td>{render_type(param.get('type'), anchors)}</td>"
            f"<td>{'; '.join(notes)}</td>"
            "</tr>"
        )
    return (
        '<table class="api-params"><thead><tr>'
        "<th>Parameter</th><th>Type</th><th>Notes</th>"
        "</tr></thead><tbody>" + "".join(rows) + "</tbody></table>"
    )


def object_properties(node_type: dict | None) -> list[dict]:
    """Collect object-literal properties from a type alias's type."""
    if node_type is None:
        return []
    if node_type.get("type") == "reflection":
        return node_type.get("declaration", {}).get("children", []) or []
    if node_type.get("type") == "intersection":
        props: list[dict] = []
        for part in node_type["types"]:
            props.extend(object_properties(part))
        return props
    return []


def properties_table(props: list[dict], anchors: Anchors) -> str:
    if not props:
        return ""
    rows = []
    for prop in props:
        flags = prop.get("flags", {})
        marks = []
        if flags.get("isReadonly"):
            marks.append("readonly ")
        optional = "?" if flags.get("isOptional") else ""
        if prop.get("signatures"):
            type_html = render_reflection(prop, anchors)
            summary = comment_markdown(
                prop["signatures"][0].get("comment") or prop.get("comment"), anchors
            )
        else:
            type_html = render_type(prop.get("type"), anchors)
            summary = comment_markdown(prop.get("comment"), anchors)
        rows.append(
            "<tr>"
            f"<td><code>{''.join(marks)}{escape(prop['name'])}{optional}</code></td>"
            f"<td>{type_html}</td>"
            f"<td>{render_inline(' '.join(summary.split())) if summary else ''}</td>"
            "</tr>"
        )
    return (
        '<table class="api-params"><thead><tr>'
        "<th>Property</th><th>Type</th><th>Description</th>"
        "</tr></thead><tbody>" + "".join(rows) + "</tbody></table>"
    )


def member_header(anchor: str, kind_label: str, title: str, level: int = 3) -> str:
    return (
        f'<h{level} class="api-name" id="{escape(anchor)}">'
        f'<span class="api-kind">{escape(kind_label)}</span>{escape(title)}</h{level}>'
    )


def render_function(member: dict, anchor: str, anchors: Anchors, level: int = 3) -> str:
    parts = [member_header(anchor, KIND_LABELS[KIND_FUNCTION], member["name"], level)]
    for sig in member.get("signatures", []):
        parts.append(signature_line(sig, anchors, name=member["name"], prefix="function "))
        parts.append(render_markdown(comment_markdown(sig.get("comment"), anchors)))
        parts.append(params_table(sig, anchors))
    return "".join(parts)


def render_callable_member(member: dict, anchor: str, anchors: Anchors, static: bool) -> str:
    parts = []
    for sig in member.get("signatures", []):
        prefix = "static " if static else ""
        name = member["name"] if member["kind"] != KIND_CONSTRUCTOR else sig["name"]
        parts.append(
            f'<h4 class="api-sub-name" id="{escape(anchor)}">{escape(name)}</h4>'
        )
        parts.append(signature_line(sig, anchors, name=name, prefix=prefix))
        parts.append(render_markdown(comment_markdown(sig.get("comment"), anchors)))
        parts.append(params_table(sig, anchors))
    return "".join(parts)


def render_class(member: dict, anchor: str, anchors: Anchors) -> str:
    implemented = member.get("implementedTypes") or []
    title = member["name"]
    heading = [member_header(anchor, KIND_LABELS[KIND_CLASS], title)]
    if implemented:
        impls = ", ".join(render_type(t, anchors) for t in implemented)
        heading.append(
            f'<pre class="api-signature"><code>class {escape(title)} '
            f"implements {impls}</code></pre>"
        )
    heading.append(summary_html(member, anchors))
    for child in member.get("children", []):
        if child.get("flags", {}).get("isPrivate"):
            continue
        child_anchor = f"{anchor}.{child['name']}"
        static = bool(child.get("flags", {}).get("isStatic"))
        if child["kind"] in (KIND_CONSTRUCTOR, KIND_METHOD):
            heading.append(render_callable_member(child, child_anchor, anchors, static))
        elif child["kind"] == KIND_PROPERTY:
            heading.append(properties_table([child], anchors))
    return "".join(heading)


def render_interface(member: dict, anchor: str, anchors: Anchors) -> str:
    parts = [member_header(anchor, KIND_LABELS[KIND_INTERFACE], member["name"])]
    parts.append(summary_html(member, anchors))
    props = [c for c in member.get("children", []) if c["kind"] == KIND_PROPERTY]
    methods = [c for c in member.get("children", []) if c["kind"] == KIND_METHOD]
    parts.append(properties_table(props, anchors))
    for method in methods:
        parts.append(render_callable_member(method, f"{anchor}.{method['name']}", anchors, False))
    return "".join(parts)


def render_type_alias(member: dict, anchor: str, anchors: Anchors) -> str:
    parts = [member_header(anchor, KIND_LABELS[KIND_TYPE_ALIAS], member["name"])]
    tps = render_type_params(member)
    props = object_properties(member.get("type"))
    if props:
        declaration = f"type {escape(member['name'])}{tps}"
        node_type = member.get("type", {})
        if node_type.get("type") == "intersection":
            bases = [
                render_type(part, anchors)
                for part in node_type["types"]
                if part.get("type") != "reflection"
            ]
            joined = " &amp; ".join([*bases, "{ ... }"])
            parts.append(
                f'<pre class="api-signature"><code>{declaration} = {joined}</code></pre>'
            )
        else:
            parts.append(
                f'<pre class="api-signature"><code>{declaration} = {{ ... }}</code></pre>'
            )
        parts.append(summary_html(member, anchors))
        parts.append(properties_table(props, anchors))
    else:
        rendered = render_type(member.get("type"), anchors)
        parts.append(
            f'<pre class="api-signature"><code>type {escape(member["name"])}{tps} = '
            f"{rendered}</code></pre>"
        )
        parts.append(summary_html(member, anchors))
    return "".join(parts)


def render_reexport(member: dict, anchor: str, anchors: Anchors) -> str:
    target_anchor = anchors.resolve(member.get("target"))
    parts = [member_header(anchor, KIND_LABELS[KIND_REEXPORT], member["name"])]
    if target_anchor:
        parts.append(
            f'<p>Re-export of <a href="#{escape(target_anchor)}">'
            f"<code>{escape(target_anchor)}</code></a>.</p>"
        )
    else:
        parts.append("<p>Re-export.</p>")
    return "".join(parts)


def render_namespace(member: dict, module_name: str, anchor: str, anchors: Anchors) -> str:
    parts = [member_header(anchor, KIND_LABELS[KIND_NAMESPACE], member["name"])]
    parts.append(summary_html(member, anchors))
    for nested in member.get("children", []):
        nested_anchor = member_anchor(module_name, member["name"], nested["name"])
        parts.append('<div class="api-member api-member-nested">')
        parts.append(render_member(nested, module_name, nested_anchor, anchors, level=4))
        parts.append("</div>")
    return "".join(parts)


def render_member(
    member: dict, module_name: str, anchor: str, anchors: Anchors, level: int = 3
) -> str:
    kind = member["kind"]
    if kind == KIND_FUNCTION:
        return render_function(member, anchor, anchors, level)
    if kind == KIND_CLASS:
        return render_class(member, anchor, anchors)
    if kind == KIND_INTERFACE:
        return render_interface(member, anchor, anchors)
    if kind == KIND_TYPE_ALIAS:
        return render_type_alias(member, anchor, anchors)
    if kind == KIND_REEXPORT:
        return render_reexport(member, anchor, anchors)
    if kind == KIND_NAMESPACE:
        return render_namespace(member, module_name, anchor, anchors)
    raise SystemExit(f"unhandled member kind {kind} for {module_name}.{member['name']}")


# ---------------------------------------------------------------------------
# Modules, TOC, page.
# ---------------------------------------------------------------------------

def module_groups(module: dict) -> list[tuple[str, list[dict]]]:
    members = {member["id"]: member for member in module.get("children", [])}
    grouped: list[tuple[str, list[dict]]] = []
    for group in module.get("groups", []):
        entries = [members[i] for i in group.get("children", []) if i in members]
        if entries:
            grouped.append((group["title"], entries))
    return grouped


def render_module(module: dict, anchors: Anchors) -> str:
    name = module["name"]
    parts = [
        f'<section class="api-module" id="module-{escape(name)}">',
        f'<h2 class="api-module-name"><span class="api-kind">module</span>'
        f"{escape(MODULE_IMPORTS[name])}</h2>",
        summary_html(module, anchors),
    ]
    for title, entries in module_groups(module):
        parts.append(f'<p class="api-group-label">{escape(title)}</p>')
        for member in entries:
            anchor = member_anchor(name, member["name"])
            parts.append('<div class="api-member">')
            parts.append(render_member(member, name, anchor, anchors))
            parts.append("</div>")
    parts.append("</section>")
    return "".join(parts)


def render_toc(modules: list[dict]) -> str:
    items = []
    for module in modules:
        name = module["name"]
        member_items = []
        for _, entries in module_groups(module):
            for member in entries:
                anchor = member_anchor(name, member["name"])
                member_items.append(
                    f'<li><a href="#{escape(anchor)}">{escape(member["name"])}</a></li>'
                )
        items.append(
            f'<li class="toc-module"><a href="#module-{escape(name)}">'
            f"{escape(MODULE_IMPORTS[name])}</a>"
            f"<ul>{''.join(member_items)}</ul></li>"
        )
    return f'<ul class="toc">{"".join(items)}</ul>'


def render_page(project: dict, digest: str) -> str:
    by_name = {module["name"]: module for module in project.get("children", [])}
    missing = [name for name in MODULE_ORDER if name not in by_name]
    if missing:
        raise SystemExit(f"api.json is missing expected modules: {missing}")
    modules = [by_name[name] for name in MODULE_ORDER]

    anchors = collect_anchors(modules)
    body = "".join(render_module(module, anchors) for module in modules)
    toc = render_toc(modules)

    return f"""<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <meta name="theme-color" content="#0d0f0f" />
    <meta name="description" content="API reference for jolt-sdk, the TypeScript SDK for building applications on the Jolt network." />
    <meta property="og:title" content="Jolt SDK Reference" />
    <meta property="og:description" content="API reference for jolt-sdk, the TypeScript SDK for building applications on the Jolt network." />
    <meta property="og:type" content="website" />
    <meta property="og:site_name" content="Jolt" />
    <meta property="og:image" content="https://alexanderwanyoike.github.io/jolt/og-card.png" />
    <meta name="twitter:card" content="summary_large_image" />
    <meta name="jolt-sdk-source-sha256" content="{digest}" />
    <title>Jolt SDK Reference</title>
    <link rel="icon" href="../favicon.svg" type="image/svg+xml" />
    <link rel="stylesheet" href="../styles.css" />
    <link rel="stylesheet" href="../docs.css" />
  </head>
  <body class="doc-page">
    <aside class="doc-sidebar">
      <a class="brand" href="../"><img class="brand-mark" src="../favicon.svg" alt="" width="30" height="30" /><span><strong>Jolt</strong><small>SDK reference</small></span></a>
      <dl>
        <div><dt>Package</dt><dd>jolt-sdk</dd></div>
        <div><dt>Source</dt><dd>sdks/js/docs/api.json</dd></div>
        <div><dt>Modules</dt><dd>{len(modules)}</dd></div>
      </dl>
      <nav aria-label="API contents">{toc}</nav>
    </aside>
    <main class="doc-main">
      <article class="doc-document api-reference">
        <header class="doc-title">
          <p>JOLT SDK · API REFERENCE</p>
          <h1>jolt-sdk</h1>
          <p class="doc-lede">Every exported class, interface, function, and type of the four SDK modules, generated from the committed typedoc output. Start with the <a href="index.html">SDK overview</a> or the <a href="../guides/app-development.html">app development guide</a>.</p>
        </header>
        {body}
        <footer class="document-footer">
          <a href="index.html">← SDK overview</a>
          <a href="https://github.com/alexanderwanyoike/jolt/tree/dev/sdks/js">TypeScript source ↗</a>
        </footer>
      </article>
    </main>
  </body>
</html>
"""


def main() -> None:
    project, digest = load_project()
    html = render_page(project, digest)
    html = html.replace(LEGACY_PACKAGE_NAME, PACKAGE_NAME)
    if "\u2014" in html:
        raise SystemExit("rendered SDK reference contains an em dash; house style forbids it")
    OUTPUT.parent.mkdir(parents=True, exist_ok=True)
    OUTPUT.write_text(html, encoding="utf-8")
    print(f"rendered SDK reference: {SOURCE.relative_to(ROOT)} -> {OUTPUT.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
