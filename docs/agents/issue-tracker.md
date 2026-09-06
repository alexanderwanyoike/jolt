# Issue tracker: Private work cards

Engineering issues and design work for this repository live as Markdown cards in the canonical private sibling checkout at `../jolt-development-docs/cards/`.

## Conventions

- Use one card per file: `<NNN>-<slug>.md`.
- Include `Type`, `Milestone`, `Status`, and `Blocked by` metadata.
- Publish cards in dependency order.
- Keep links to private design and context documents relative within the sibling repository.
- Do not create public GitHub issues unless Alexander explicitly asks.
- Do not create or use `jolt/.notes`; the sibling repository is canonical.
- Preserve existing uncommitted drafts.

## Publishing work

When a workflow says to publish an issue, create or update the corresponding card under `../jolt-development-docs/cards/`.

Do not modify a parent card while breaking it into child cards unless Alexander explicitly asks.

## Fetching work

Before acting on a card:

1. Read the card.
2. Read `../jolt-development-docs/current-context.md`.
3. Follow `../jolt-development-docs/CONTEXT-MAP.md` to the relevant bounded-context documents.
4. Check the card's dependencies and current status.
