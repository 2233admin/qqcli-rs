# Contributing to qqcli

Thank you for your interest in contributing!

## Development Setup

```bash
git clone https://github.com/2233admin/qqcli-rs.git
cd qqcli-rs
cargo build --release
```

## Code Style

- Run `cargo fmt` before committing
- Run `cargo clippy --all-targets` to catch common mistakes

## Testing

```bash
cargo test
```

## Commit Messages

Follow [Conventional Commits](https://www.conventionalcommits.org/):

```
feat: add search highlight
fix: handle empty db gracefully
docs: update README
```

## Pull Requests

1. Fork the repo
2. Create a feature branch: `git checkout -b feat/your-feature`
3. Make your changes
4. Run tests: `cargo test`
5. Push and open a PR

## Questions?

Open an issue for bugs or feature requests.
