# Agent Instructions

## Engineering Workflow

- Prefer test-driven development for this project.
- For behavioral changes, write or update a focused failing test first, then implement the smallest change that makes it pass.
- Test durable behavior and public contracts, not temporary scaffolding or incidental UI structure.
- Keep the red-green-refactor loop visible in PR descriptions when practical.
- If a change is docs-only, tooling-only, or cannot reasonably be tested first, state that explicitly in the PR notes.
- Run the relevant focused tests before the full local suite. Before opening or updating a PR, run `./scripts/test-local.sh` unless the change is clearly docs-only.

## PR Closeout and Context Handoff

- Treat the repository as the durable project memory; do not rely on chat history surviving context resets.
- At the end of each PR, update the relevant `docs/cards/*.md` status and verification notes.
- Keep PR descriptions descriptive: explain what changed, why it matters, how it was tested, and any known follow-up debt.
- Maintain `.notes/current-context.md` as gitignored working memory for the next session. Include the active branch/PR, recent verification commands, local process state, blockers, and the next intended task.
- When a context reset is expected, make sure project changes are committed and pushed, then refresh `.notes/current-context.md` before stopping.
- On a fresh session, read `AGENTS.md`, the active/relevant cards under `docs/cards`, and `.notes/current-context.md` before continuing work.

## Protocol Boundary

- Keep the protocol layer application-agnostic.
- Protocol code may know about identities, CIDs, signed update logs, content fetch, provider discovery, relays, pinning, encryption/access grants, capabilities, schema references, and generic signed paths.
- Protocol code must not hardcode application concepts such as profiles, feeds, posts, galleries, games, timelines, spaces-as-UI, or lenses-as-runtimes.
- Application and lens concepts should be represented as signed content, schemas, manifests, and capability records above the protocol layer.
- A valid protocol statement is: identity `X` maps path `/gallery` to CID `Y` at sequence `N`.
- A valid application/lens statement is: CID `Y` is a gallery manifest and this lens knows how to render or edit it.
