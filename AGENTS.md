# Agent Instructions

## Engineering Workflow

- Prefer test-driven development for this project.
- For behavioral changes, write or update a focused failing test first, then implement the smallest change that makes it pass.
- Test durable behavior and public contracts, not temporary scaffolding or incidental UI structure.
- Keep the red-green-refactor loop visible in PR descriptions when practical.
- If a change is docs-only, tooling-only, or cannot reasonably be tested first, state that explicitly in the PR notes.
- Run the relevant focused tests before the full local suite. Before opening or updating a PR, run `./scripts/test-local.sh` unless the change is clearly docs-only.

## Protocol Boundary

- Keep the protocol layer application-agnostic.
- Protocol code may know about identities, CIDs, signed update logs, content fetch, provider discovery, relays, pinning, encryption/access grants, capabilities, schema references, and generic signed paths.
- Protocol code must not hardcode application concepts such as profiles, feeds, posts, galleries, games, timelines, spaces-as-UI, or lenses-as-runtimes.
- Application and lens concepts should be represented as signed content, schemas, manifests, and capability records above the protocol layer.
- A valid protocol statement is: identity `X` maps path `/gallery` to CID `Y` at sequence `N`.
- A valid application/lens statement is: CID `Y` is a gallery manifest and this lens knows how to render or edit it.
