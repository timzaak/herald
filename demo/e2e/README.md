# Demo E2E 测试目录

Playwright demo 测试按「操作角色 / 外部依赖」分目录组织，新 demo 一律放入对应目录：

- `realm-admin/` — Realm Admin 角色
- `super-admin/` — Super Admin 角色
- `regular-user/` — 普通用户
- `billing-admin/` — 计费管理员
- `live/` — 依赖真实外部服务或手工步骤的 demo（规则见 [live/README.md](live/README.md)）

根层 8 个 `.e2e.ts` 为遗留文件，早于上述目录约定，除非修复否则不要新增到根层：

- `authentication-redirect-demo.e2e.ts`
- `device-code-flow-demo.e2e.ts`
- `device-code-error-config-demo.e2e.ts`
- `i18n-error-translation-demo.e2e.ts`
- `i18n-language-switching-demo.e2e.ts`
- `i18n-persistence-detection-demo.e2e.ts`
- `oauth-pkce-flow-demo.e2e.ts`
- `oauth-pkce-error-demo.e2e.ts`

`fixtures/`、`helpers/`、`pages/`、`selectors.ts`、`secrets/` 为共享基础设施，有意保留在根层。

运行命令见仓库根 [scripts/index.md](/scripts/index.md)（`uv run scripts/demo-test-runner.py ...`）。
