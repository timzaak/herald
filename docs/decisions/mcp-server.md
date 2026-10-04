# mcp-server Decision Log

## Active Decisions

| ID | Level | Topic | Decision | Rationale | Decided By | Source | Affects | Reopen When | Supersedes |
|---|---|---|---|---|---|---|---|---|---|
| DEC-mcp-server-001 | D1 | mcp.tool-boundary | 首发只读工具集（5 个查询能力：用户、积分余额、积分流水、审计、Realm 配置状态），每个工具映射既有 `resource.action` 权限；写操作在 Wedge 验证后按逐工具安全评审加入（Minimal 阶段） | 最小安全面；1-2 天可交付楔子；对齐 `.ai/future/next.md` §4.4 Wedge→Minimal 两步走路径。AskUserQuestion 提问未获答复，按 next.md 既有推荐采纳，用户可在 /t-prd 推翻 | agent | .ai/future/next.md#4.4 + .ai/tech-research/mcp-server.md#4.4 | prd/design/task/test | Wedge 验证通过（4-6 周出现 agent 使用信号）进入 Minimal；或用户在 /t-prd 明确改边界 | — |
| DEC-mcp-server-002 | D2 | mcp.auth-model | MCP 端点复用 Client API Key 鉴权（api-ext 中间件模式）+ 既有 RBAC/realm 隔离；不实现 OAuth 2.1 授权流（RFC 9728/WWW-Authenticate/DCR 均不引入），OAuth 列为演进路径 | MCP 2026-07-28 规范明确 Authorization OPTIONAL，API Key 不违规；rmcp OAuth 能力仅客户端侧，服务端 OAuth 需自建成本高；主流客户端支持自定义 header 携带 API Key（Claude Code `--header` 官方文档化）。架构级集成选择，影响 prd/design/task，入账 | agent | .ai/tech-research/mcp-server.md#4.1 | prd/design/task | 客户端生态对静态 header 支持恶化（Claude Code #14976 类问题扩大），或用户要求浏览器授权 UX（Logto 同款体验） | — |

## Resolved Questions

| ID | Topic | Resolution | Decision ID | Source |
|---|---|---|---|---|
| Q-mcp-server-001 | mcp.auth-spec-compliance | MCP 授权规范下 API Key Bearer 是否合规：合规。规范原文「Authorization is OPTIONAL for MCP implementations」，仅当实现 OAuth 时 HTTP transport 才 SHOULD conform（届时 PRM/RFC 9728 为 MUST） | DEC-mcp-server-002 | .ai/tech-research/mcp-server.md#4.4 + modelcontextprotocol.io/specification/2026-07-28/basic/authorization |

## Deferred Questions

| ID | Topic | Why Non-blocking Now | Owner Stage | Must Resolve Before | User Informed |
|---|---|---|---|---|---|
| Q-mcp-server-002 | mcp.customer-facing-auth（MCP Auth 等价物：客户用 Herald 给自己的 MCP server 做 OAuth 2.1 鉴权） | 属另一产品方向，依赖已 Park 的 OIDC 决策（.ai/decision/openid-connect.md）重开，且无需求证据；不影响本 feature 的 MCP server 路线 | t-decision（新方向立项时） | 用户选择追求该方向时的 /t-decision | yes |

## Superseded Decisions

| ID | Superseded By | Previous Decision | Source |
|---|---|---|---|
