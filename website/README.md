# Jolt website

This is a dependency-free static site for GitHub Pages.

Preview it from the repository root:

```sh
python3 -m http.server 4173 --directory website
```

Then open <http://127.0.0.1:4173>. The Pages workflow deploys the same files
without a compilation step.

The landing-page copy should stay aligned with `README.md` and the protocol
documents under `docs/`. Detailed protocol decisions belong in `rfcs/`, not in
the marketing copy.
