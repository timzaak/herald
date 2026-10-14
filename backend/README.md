# Backend

Rust workspace（22 crate，`backend/Cargo.toml` 为根），Herald 多租户认证与授权后端。

## 分层地图

- **domain** — `domain`（业务实体与端口 trait，零外部依赖）、`entity`（数据库实体）。
- **infra** — `infra`（仓储等外部适配器实现）、`infra-stripe` / `infra-creem` / `infra-iap` / `infra-wechatpay`（各支付渠道适配）。
- **core** — `herald-core`：六边形架构装配点（re-export domain/entity/infra）+ 配置与第三方集成。
- **api** — `api-base`（HTTP 基础设施：中间件、错误、限流）；领域子 crate：`api-auth`（登录/注册/reauth/改邮箱等）、`api-oauth`（第三方登录）、`api-admin`（管理台）、`api-billing`（计费与 webhook）、`api-points`（积分）、`api-mcp`（MCP 端点 `/mcp/{realmId}`）、`api-ext`（API Key 外部 API）；`api` 聚合各子 crate 成统一路由/服务器。
- **运行时** — `app`（可执行入口）、`worker`（后台任务）。
- **测试支撑** — `test-db`（测试数据库连接）、`test-support`（schema 隔离测试上下文与公共 helper）、`integration-tests`（SDK 集成测试，见下）。
- `config/` 为运行配置 TOML，非 crate。

## 两类场景测试的选择

1. **schema 场景测试** — `api/src/tests/scenarios/`：用 axum `oneshot` 直接打 HTTP 路由（Bearer/表单体），在 `SchemaTestContext` 的 schema 隔离库上断言状态码、权限、隔离等 API 契约。按用户故事命名并在头注释引用故事 ID（如 US-RU-016）。验证后端路由行为/权限/领域规则时写在这里。
2. **SDK 集成测试** — `integration-tests/tests/sdk_*`：经 `herald_sdk::Client`（API Key 认证）打真实 API，验证 SDK 方法与 wire 契约。验证 SDK 暴露面（方法、参数、错误映射）时写在这里。

## 其他入口

- 测试与脚本命令表：仓库根 `scripts/index.md`
