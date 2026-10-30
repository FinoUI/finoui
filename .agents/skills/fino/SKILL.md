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
cargo run -p fino_counter -- scenario run scenarios/counter.json --backend mock --out artifacts/runs/counter-mock --format json
fino inspect dom --run artifacts/runs/counter-mock --format json
fino inspect a11y --run artifacts/runs/counter-mock --format json
fino inspect reactive --run artifacts/runs/counter-mock --format json
fino inspect trace --run artifacts/runs/counter-mock --format json
```

Run the same unchanged scenario on later backends through the example package, then inspect the resulting `run_dir`. Do not move the example's mount or checks into `fino_cli`.

后续后端也通过示例 package 运行同一场景，再检查生成的 `run_dir`。不要把示例挂载或检查移入 `fino_cli`。
