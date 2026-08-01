# Contributing to aperture-router

Contributions are welcome! This guide covers the basics.

## Development Setup

```bash
git clone https://github.com/Wayazi/aperture-router.git
cd aperture-router
cargo build
cargo test
```

## Workflow

We use a two-branch workflow (see [docs/WORKFLOW.md](docs/WORKFLOW.md)):

1. Fork the repo and create a branch from `dev`:
   ```bash
   git checkout dev
   git checkout -b feature/your-feature
   ```

2. Make your changes. Before pushing, verify:
   ```bash
   cargo test
   cargo clippy --all-targets -- -D warnings
   cargo fmt --check
   ```

3. Push and create a PR targeting `dev` (not `main`):
   ```bash
   git push origin feature/your-feature
   ```

## Commit Messages

Use [Conventional Commits](https://www.conventionalcommits.org/):

```
feat: add support for tool calling
fix: resolve memory leak in streaming
docs: update installation guide
test: add integration tests for auth
chore: bump version to 0.3.1
```

## Code Style

- **No comments** unless explicitly requested
- Follow existing patterns — look at neighboring code
- Prefer `Arc<T>` over cloning large structs
- Use `anyhow::Result` for error handling
- Security-sensitive code: constant-time comparisons, zeroizing, never log secrets

## Testing

- All new features must have tests
- Integration tests go in `tests/`
- Unit tests go in `#[cfg(test)] mod tests` within the source file
- Run `cargo test` before every commit

## Security

If you find a security vulnerability, please report it privately via GitHub Issues rather than opening a public PR.

Key rules:
- Never log API keys
- Never enable HTTP redirects
- Never skip SSRF validation
- Config files must be `0o600`

## Documentation

Documentation uses the [Diátaxis framework](https://diataxis.fr/):

```
docs/
├── tutorials/       Step-by-step for beginners
├── how-to/          Achieve a specific goal
├── reference/       Commands, configs, specs
├── explanation/     Why things work this way
└── troubleshooting.md
```

When adding features, update the relevant docs section.

## Release Process (maintainers only)

1. Ensure `dev` is stable: `cargo test && cargo clippy -- -D warnings && cargo fmt --check`
2. Bump version in `Cargo.toml`
3. Update `CHANGELOG.md`
4. Commit: `chore: bump version to X.Y.Z`
5. Merge `dev` → `main`
6. Tag: `git tag -a vX.Y.Z -m "Release vX.Y.Z"`
7. Push: `git push origin main --tags`
8. Update AUR package
