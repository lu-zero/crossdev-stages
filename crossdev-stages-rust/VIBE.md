# VIBE Rules

## Code Formatting
- Before any commit, run `cargo fmt` at least once to ensure consistent formatting.

## Warnings
- The code must be warning-free. If you introduce a warning, stop and fix it first before proceeding.

## Documentation
- Always try to read the documentation for the crates you are going to use.
- Use `cargo doc --document-private-items -p {the crate}` to produce the documentation.
- Use `cargo metadata --format-version=1 | jq -r '.packages[] | "\(.name) \(.version)\t\(.targets[0].src_path // "—")"'` to see where the sources for the dependencies are cached.
