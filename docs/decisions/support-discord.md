# support-discord Decision Log

## Active Decisions

| ID | Level | Topic | Decision | Rationale | Decided By | Source | Affects | Reopen When | Supersedes |
|---|---|---|---|---|---|---|---|---|---|
| DEC-support-discord-001 | D1 | scope.discord-provider | 支持 Discord OAuth 登录：作为第 5 个通用跳转式 OAuth Provider 接入既有 Provider 登录/回调链路，复用现有配置管理、四层身份匹配与全部门控；不新增专属流程、路由、外部依赖或数据库迁移。`.ai/future/next.md` 将「更多社交登录（含 Discord）」归入按需响应 Hold 池，本次用户需求即触发条件，Hold 解除 | 用户明确提出「支持 discord Oauth 登录」；技术预研确认该需求为既有 provider 模式的机械扩展，集成路径唯一且无歧义，无影响技术路线、范围边界、兼容性或成本/风险结论的用户决策问题 | user | `.ai/tech-research/support-discord.md` §1.1（输入源为用户原始需求） | prd/design/task | 出现通用跳转链路之外的 Discord 专属能力需求（如 Bot/guild 维度身份、扫码形态） | — |

## Resolved Questions

| ID | Topic | Resolution | Decision ID | Source |
|---|---|---|---|---|

## Deferred Questions

| ID | Topic | Why Non-blocking Now | Owner Stage | Must Resolve Before | User Informed |
|---|---|---|---|---|---|

## Superseded Decisions

| ID | Superseded By | Previous Decision | Source |
|---|---|---|---|
