# AGENTS.md

Conventions for anyone — human or agent — changing this repository.

## Every comment and doc is bilingual

The repo is read by English and Chinese speakers, so nothing ships in one language. Code comments carry both languages in one block; prose docs live in `docs/en` and `docs/zh-cn` at identical paths.

```rust
// Retry once when the network fails transiently.
// 网络瞬时失败时重试一次。

/// Parses `fino.toml` into a [`Config`].
///
/// 把 `fino.toml` 解析成 [`Config`]。
pub fn parse(text: &str) -> Result<Config> {
```

Identifiers, paths, commands, log output, and quoted third-party text stay as they are; translate the sentence around them.

Docs:

- A doc edit touches `docs/en/<path>` and `docs/zh-cn/<path>` in the same commit.
- Each file opens with `[English](architecture.md) · [简体中文](../zh-cn/architecture.md)`.
- The pair shares headings, order, and facts. Write each version as an original in its own language.
- Before committing, this prints nothing: `diff <(cd docs/en && find . -type f | sort) <(cd docs/zh-cn && find . -type f | sort)`
- `README.md` and `README.zh-CN.md` are matching English and Chinese index cards: project name, one sentence, and links into their language trees. Longer prose belongs in the trees.

Single-language by design: this file (agent instructions, one source of truth), `LICENSE`, generated and vendored text.

## Commit messages

Prefix every commit subject with exactly one [Gitmoji](https://gitmoji.dev/) emoji and a space, chosen for the primary change. Use the emoji itself rather than its `:shortcode:`. Example: `🎉 Initial commit`.

## Before finishing

`cargo fmt --all`, `cargo clippy --all-targets -- -D warnings`, and `cargo test` pass, and every comment and doc touched has its other language staged in the same commit.

Install the hooks once with `pre-commit install`. Use `pre-commit run --all-files` to run the same Rust checks used by CI.
