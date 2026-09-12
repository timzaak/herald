# 如何运行测试

优先运行本次改动涉及的测试；将 `<...>` 替换为实际文件或测试名。

| 测试 | 运行目录 | 命令 |
| --- | --- | --- |
| 后端 | 项目根目录 | `uv run scripts/backend-test.py -- <test_name>` |
| 前端 | `frontend/` | `npm run test:run -- <测试文件>` |
| Node SDK | `sdk/node/` | `npm run test:run -- <测试文件>` |
| Web SDK | `sdk/web/` | `npm run test:run -- <测试文件>` |
| Rust SDK | `sdk/rust/` | `cargo test <test_name>` |
| Demo | 项目根目录 | `uv run scripts/demo-test-runner.py "demo/e2e/<file>.e2e.ts" --run-id <唯一ID> --no-ngrok` |

- **准备**：后端需 uv、Rust、cargo-nextest、sccache 和 Docker；JS 项目先在对应目录执行 `npm ci`，Demo 另在 `demo/` 执行 `npx playwright install chromium`。后端和 Demo 脚本自动管理所需环境。
- **日志**：后端看 `backend-test-output.log`；Demo 看 `demo/test-results/runs/<run-id>/playwright-output.log`；其余看终端。
- **重试**：保留失败日志，修复后重跑原命令；Demo 每次换新 run ID，单用例筛选可追加 `--grep "<测试标题>"`，通过后重跑整文件。
- **结果**：零用例、类型检查和编译成功不等于测试通过；未运行须说明。测试入口变更时同步更新本文件。
