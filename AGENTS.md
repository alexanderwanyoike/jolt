# Agent Instructions

## Engineering Workflow

- Prefer test-driven development for this project.
- For behavioral changes, write or update a focused failing test first, then implement the smallest change that makes it pass.
- Test durable behavior and public contracts, not temporary scaffolding or incidental UI structure.
- Keep the red-green-refactor loop visible in PR descriptions when practical.
- If a change is docs-only, tooling-only, or cannot reasonably be tested first, state that explicitly in the PR notes.
- Run the relevant focused tests before the full local suite. Before opening or updating a PR, run `./scripts/test-local.sh` unless the change is clearly docs-only.
