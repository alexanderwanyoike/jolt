# Website Strategy

## Audience

The Jolt website is for a technically curious newcomer first. Its first job is
to make the project thesis understandable without requiring protocol vocabulary
or a source checkout.

Secondary paths serve:

- application developers deciding whether Jolt is a useful substrate;
- protocol contributors reviewing architecture and RFCs;
- early users looking for Console, Spoke, and honest maturity information;
- future relay or community operators looking for implementation docs.

## Message

The homepage should make this distinction clear:

```text
Apps give content meaning and people an experience.
Jolt keeps identity, signed state, content verification, scoped authority,
transport, and availability beneath any one application.
```

The protocol layer never claims app concepts such as profiles, posts, feeds,
galleries, games, or timelines. Spoke is evidence that a social app can live
above the boundary; it does not define Jolt's data model.

## Structure

The v0 site is a hybrid landing page and protocol field guide:

- plain-language thesis and motivation;
- protocol loop and architecture;
- experimental status and limitations;
- links to installation, source, and Spoke;
- an RFC index and readable RFC pages.

Focused concept and installation pages can follow once the v0 identity, device,
community, encryption, and app-interface decisions are stable enough to explain
without presenting proposals as shipped behavior.

## Source of Truth

- `README.md` owns current project positioning, working behavior, installation,
  and limitations.
- `docs/` owns detailed architecture and design context.
- `rfcs/` owns compatibility-shaping proposals and decisions.
- `website/` presents those sources; it must not become an independent protocol
  specification.

Any PR that changes a public capability, maturity claim, installation path, or
accepted RFC should review the corresponding website section. The static-site
verification catches missing local targets, while content accuracy remains a
normal code-review responsibility.

## Delivery

The dependency-free static site lives in `website/`. GitHub Actions verifies it
on pull requests to `dev` and deploys it from `main` with GitHub Pages. It does
not require a running Jolt node.
