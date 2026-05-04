# Contributing to rupo

## CI Requirements

Every PR must pass all checks on **Linux, macOS, and Windows** before merging:

```
cargo fmt --check
cargo clippy -- -D warnings
cargo test --all
```

Do not merge a PR that is red on any platform.

## Testing Policy

A PR that changes logic **must** include tests. No exceptions for AI-assisted contributions.

The following changes may be merged without new tests:
- Documentation or comment changes
- `cargo fmt` only
- Dependency version bumps
- Refactoring fully covered by existing tests

## AI Contribution Policy

AI-assisted contributions are welcome, but:
- The human submitting the PR is fully responsible for all code
- Tests are always required — the documentation-only exemption does not apply
- PRs without human review and sign-off will be closed

## License

By contributing, you agree your contributions will be licensed under the [Apache License 2.0](LICENSE).
