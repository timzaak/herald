# openid-connect Decision Log

> 初始化于 2026-09-14。来源：`.ai/decision/openid-connect.md`（Decision Brief，2026-07-14，Verdict: Park）+ 本轮 `/t-prd` 澄清对话。
> Brief 未分配稳定 DEC ID，按 Decision Continuity Contract 从既有产物决策初始化；Brief 文件保留为历史记录，不修改。

## Active Decisions

| ID | Level | Topic | Decision | Rationale | Decided By | Source | Affects | Reopen When | Supersedes |
|---|---|---|---|---|---|---|---|---|---|
| DEC-openid-connect-001 | D0 | positioning.saas-base | Herald 维持"SaaS 底座"定位，不进入完整身份平台赛道 | 差异化护城河在计费+积分；完整身份平台是独立品类（Zitadel/Logto/Keycloak 专注），进入即放弃自身差异化 | user | `.ai/decision/openid-connect.md` §8（2026-09-14 用户选择"最小 OIDC 层"时再次确认） | prd/design/task | 用户决定做完整 OIDC Provider / 正式进入身份平台赛道 | — |
| DEC-openid-connect-004 | D1 | oidc.minimal-layer-scope | 重开 Park；本轮只做**最小 OIDC 兼容层**：id_token + JWKS + discovery + userinfo，叠加在既有 Authorization Code + PKCE 流程上，使标准 OIDC 客户端零 SDK 接入；**不引入** OIDC scope 管理、用户同意（consent）授权页与标准化 refresh；第三方接入边界由"仅为 Herald 开发的应用"扩展为"任意标准 OIDC 客户端亦可接入" | 用户在竞品差距分析（2026-09-14）后作出战略判断：OIDC 是 IdP 生态接入入场券。重启动因为用户显式决策而非客户需求证据（原重启条件 DEC-003 未被满足，由用户覆盖） | user | conversation 2026-09-14（`/t-prd` 澄清门禁 AskUserQuestion，选项"重开·最小 OIDC 层"） | prd/design/task/test | 出现完整 scope/consent 体系需求或身份平台定位调整 | DEC-openid-connect-002、DEC-openid-connect-003 |
| DEC-openid-connect-005 | D2 | oidc.issuer-endpoint-shape | issuer = `{origin}/api/oauth/{realmId}`（origin 复用 `realm_public_url_parts` 派生：enabled 自定义域名 → `https://{hostname}`，否则 `public_base_url`）；discovery/JWKS/userinfo 全部挂 `/api/oauth/{realmId}/` 前缀，发现 URL = issuer + `/.well-known/openid-configuration`（OIDC Core append 语义） | 路径式 issuer 对 Grafana/Vault 类标准客户端兼容面最广；不叠加前端 realm 路径前缀（否则发现 URL 命中 SPA fallback 不可路由）；issuer 一经发布即对外契约，反转代价高，且影响 task/test 两阶段 | agent | `.ai/design/openid-connect/backend.md` §4.2、§9.1（2026-09-14 `/t-design`） | task/test | 出现独立 OIDC 子域名需求或网关路由形态变化 | — |
| DEC-openid-connect-006 | D2 | oidc.signing-key-model | RS256、**平台级**签名密钥（Realm 无关）；migration 0010 新表 `oidc_signing_key`（部分唯一索引防并发双主），私钥 AES-256-GCM 静态加密（KEK = SHA256(域分隔前缀 + `[jwt] secret)`)；启动自举首把密钥；`/api/internal/oidc/signing-key/rotate`（复用 custom-domain ask_key 守卫）轮换 + 7 天 Retained 重叠期；JWKS 发布 Active + 未到期 Retained | iss+aud 校验已保证 realm 隔离，per-realm 轮换运维负担不成比例；RS256 客户端兼容面最广且 jsonwebtoken 已有 RS256 先例；密钥模型影响 task/test 且存量密钥形成后反转代价高 | agent | `.ai/design/openid-connect/backend.md` §5、§9.2（2026-09-14 `/t-design`） | task/test | 出现 per-realm 密钥、多算法或硬件密钥管理需求 | — |
| DEC-openid-connect-007 | D2 | oidc.token-coexistence | 不引入新令牌类型：id_token 为既有 `/token` 响应的**增量字段**（仅授权请求 scope 含 `openid` 时签发，`skip_serializing_if` 保证零回归）；userinfo 复用既有不透明浏览器 access token（Bearer）认证链；`nonce` 透传回显到 id_token | 保持令牌体系单一；"不含 openid 的请求行为不变"是 PRD 硬验收；nonce 是协议安全参数（非身份声明），标准客户端普遍发送并期望回显；影响 task/test 两阶段 | agent | `.ai/design/openid-connect/backend.md` §4.2、§6.1、§9.3（2026-09-14 `/t-design`） | task/test | 引入 introspection 型 JWT access token 或标准化 refresh（超出 DEC-004 范围，需重新立项评估） | — |

## Resolved Questions

| ID | Topic | Resolution | Decision ID | Source |
|---|---|---|---|---|
| Q-openid-connect-001 | park.reopen | 用户选择重开 Park 并采用最小 OIDC 兼容层；不维持 Park，也不回 `/t-decision` 重新立项 | DEC-openid-connect-004 | conversation 2026-09-14 |
| Q-openid-connect-002 | brief.blocking-handoff | Brief 的 Park Verdict 被 DEC-openid-connect-004 显式覆盖，`/t-prd` 解除阻塞继续；Brief 文件保留为历史记录，不修改 | DEC-openid-connect-004 | conversation 2026-09-14 |
| Q-openid-connect-003 | discovery.issuer-per-realm | issuer = `{origin}/api/oauth/{realmId}`，origin 派生复用 `realm_public_url_parts`（自定义域名优先）；discovery/JWKS/userinfo 挂 `/api/oauth/{realmId}/` 前缀；发现 URL = issuer + `/.well-known/openid-configuration` | DEC-openid-connect-005 | `.ai/design/openid-connect/backend.md` §4.2、§9.1 |
| Q-openid-connect-004 | signing.key-management | RS256、平台级密钥、migration 0010 表 + AES-256-GCM 静态加密（KEK 派生自 `[jwt] secret`）、启动自举 + ask_key 内部轮换端点、7 天 Retained 重叠期 | DEC-openid-connect-006 | `.ai/design/openid-connect/backend.md` §5、§9.2 |
| Q-openid-connect-005 | token.coexistence | id_token 为既有 `/token` 响应增量字段（scope 含 openid 才签发，零回归）；userinfo 复用既有浏览器 access token 认证链；nonce 透传回显；不引入新令牌类型 | DEC-openid-connect-007 | `.ai/design/openid-connect/backend.md` §4.2、§6.1、§9.3 |

## Deferred Questions

| ID | Topic | Why Non-blocking Now | Owner Stage | Must Resolve Before | User Informed |
|---|---|---|---|---|---|

> 无。原延期问题 Q-openid-connect-003/004/005 已于 2026-09-14 `/t-design` 阶段解决（DEC-005/006/007），移入 Resolved Questions。

## Superseded Decisions

| ID | Superseded By | Previous Decision | Source |
|---|---|---|---|
| DEC-openid-connect-002 | DEC-openid-connect-004 | 第三方接入边界：仅"为 Herald 开发的应用"（OIDC 不进入 scope） | `.ai/decision/openid-connect.md` §8 |
| DEC-openid-connect-003 | DEC-openid-connect-004 | 重启条件：出现具体客户/issue 提"标准 OIDC 工具接不上 Herald"且影响成单或集成才重启 | `.ai/decision/openid-connect.md` §8 |
