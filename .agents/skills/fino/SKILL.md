---
name: fino
description: Use Fino's generic CLI and Cargo-based example runners for headless checks, scenarios, and structured artifact inspection. 使用 Fino 通用 CLI 与 Cargo 示例运行器执行无头检查、场景和结构化产物检查。
---

# Fino development tools / Fino 开发工具

## Rules / 规则

- Use `fino` only for generic project checks and artifact inspection. Never add example discovery, mounting, checks, or dependencies to `fino_cli`.
- 只把 `fino` 用于通用项目检查和产物检查，不在 `fino_cli` 中加入示例发现、挂载、检查或依赖。
- Run each example through its own Cargo package. The example owns its mount, smoke checks, and scenario runner.
- 通过示例自己的 Cargo package 运行示例；挂载、冒烟检查和场景运行器均归示例所有。
- Use `--format json` for agent workflows. Read large snapshots from artifact paths instead of pasting them into context.
- Agent 工作流使用 `--format json`；大型快照从产物路径读取，避免粘贴到上下文中。

## Counter workflow / 计数器工作流

```bash
fino check --format json
cargo run -p fino_counter -- check --format json
cargo run -p fino_counter -- scenario run examples/counter/scenarios/counter.json --backend mock --out artifacts/runs/counter-mock --format json
cargo run -p fino_counter -- scenario run examples/counter/scenarios/counter.json --backend web-headless --out artifacts/runs/counter-web --format json
fino inspect dom --run artifacts/runs/counter-mock --format json
fino inspect a11y --run artifacts/runs/counter-mock --format json
fino inspect reactive --run artifacts/runs/counter-mock --format json
fino inspect trace --run artifacts/runs/counter-mock --format json
```

The `web-headless` runner builds the wasm bundle and drives headless Chromium. It requires `wasm32-unknown-unknown`, `wasm-bindgen-cli` 0.2.129, and `npm ci --prefix examples/counter/web` plus `npx --prefix examples/counter/web playwright install chromium`. It fails explicitly when they are missing.
`web-headless` 运行器构建 wasm 包并驱动无头 Chromium，需要 `wasm32-unknown-unknown`、`wasm-bindgen-cli` 0.2.129，以及执行 `npm ci --prefix examples/counter/web` 和 `npx --prefix examples/counter/web playwright install chromium`。缺少时显式失败。

```bash
cargo test -p fino_counter --test web_browser -- --ignored
```

Run this browser test when changing the web backend; it is ignored in the default suite because it launches Chromium.
修改 Web 后端时运行该浏览器测试；它因启动 Chromium 而默认被忽略。

Run the same unchanged scenario on later backends through the example package, then inspect the resulting `run_dir`. Do not move the example's mount or checks into `fino_cli`.

后续后端也通过示例 package 运行同一场景，再检查生成的 `run_dir`。不要把示例挂载或检查移入 `fino_cli`。
