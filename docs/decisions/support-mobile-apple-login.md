# support-mobile-apple-login Decision Log

## Active Decisions

| ID | Level | Topic | Decision | Rationale | Decided By | Source | Affects | Reopen When | Supersedes |
|---|---|---|---|---|---|---|---|---|---|
| DEC-support-mobile-apple-login-001 | D0 | scope.client-ownership | 同时支持第一方与第三方 Client App:端点双分支,直接 session(第一方/绑定 client_id)与下游 Code+PKCE(第三方,brokered downstream_state),完全参照 google_one_tap 的形态 | 用户明确"类似 google one tap,herald 用户在自己 realm 里创建对接,使之能在手机 app 里对接";复用现有 brokered 流程避免再造 | user | conversation | prd/design/task | 用户提出只需单一模式 | — |
| DEC-support-mobile-apple-login-002 | D0 | scope.client-secret | 本次仅做 native 路径(只校验 identityToken,不调 Apple token 端点),不修 Apple web redirect 的 JWT client_secret 运行时自动签发缺陷 | native Sign in with Apple 最佳实践是 App 端 ASAuthorizationAppleIDProvider 直接拿 identityToken,无需换 code,天然绕开 client_secret 每 6 个月续签的运维负担;web redirect 缺陷独立,留后续 | user | conversation | prd/design/task | 需要支持 web redirect Apple 登录的自动 client_secret 续签 | — |
| DEC-support-mobile-apple-login-003 | D0 | scope.frontend | Herald 前端(Web SPA)无改动;纯后端能力,iOS 端由接入方自行实现 | 用户明确"无 iOS App 计划,纯后端能力";本仓库 frontend 是 Web SPA,Apple native 登录的客户端在 iOS 侧,不在本仓库 | user | conversation | design/task | Herald 决定自建第一方 iOS App | — |
| DEC-support-mobile-apple-login-004 | D2 | impl.jwks-injection | `verify_apple_id_token` 增加 `jwks_url` 参数(对齐 google 的 `verify_google_id_token`),并在 AppState 增加 `apple_jwks_url` 字段(从 config 读取,默认 `https://appleid.apple.com/auth/keys`) | 与 google verify 函数保持一致的测试性:scenario 测试可在 AppState 私有副本上指向 wiremock JWKS,无需进程级 mutation;google 侧已有成熟测试范式可照搬 | repository-fact | `infra/src/oauth/providers/google.rs:208-214` 注释 + `api-base/.../state.rs:299-303` 注释 | design/task | — | — |
| DEC-support-mobile-apple-login-005 | D1 | business.empty-email-account | Apple identityToken 的 email 为空(非首次登录或用户隐藏邮箱)且 open_id 未命中(Herald 无 provider 记录)时,生成 `{sub}@apple.placeholder` 占位 email、`verified=false` 建号 | (1) `account.email` 是 NOT NULL + 唯一索引 `(realm_id, email)`,不能存空;(2) `find_or_create_user` 优先级是 union_id → open_id → email,Apple open_id=Some(sub) 稳定,故 email 仅在「首次建号」场景影响建号,后续登录靠 open_id 命中不再依赖 email;(3) 对齐项目现有 WeChat placeholder 范式(wechat.rs:157 / wechat_miniprogram.rs:107,`{id}@wechat.placeholder` + verified=false,PRD `docs/prd/auth/wechat-oauth.md` §4.1 已记录),保持社交登录建号策略一致;(4) Apple 的 `@privaterelay.appleid.apple.com` 中转邮箱是合法可收信邮箱,作真实 email 处理,不归为 placeholder | user + repository-fact | conversation + `migrations/0001_core.sql:25,35` + `helper.rs:401-467` | prd/design/task | 项目决定废弃 placeholder 范式(需 WeChat/Apple 一并迁移) | — |

## Resolved Questions

| ID | Topic | Resolution | Decision ID | Source |
|---|---|---|---|---|
| Q-support-mobile-apple-login-001 | scope.client-ownership | 端点同时支持第一方(直接 session)与第三方(下游 Code+PKCE)双分支 | DEC-support-mobile-apple-login-001 | conversation |
| Q-support-mobile-apple-login-002 | scope.client-secret | 仅做 native 路径,不修 web redirect client_secret 缺陷 | DEC-support-mobile-apple-login-002 | conversation |
| Q-support-mobile-apple-login-003 | scope.frontend | 无 iOS App 计划,前端不改 | DEC-support-mobile-apple-login-003 | conversation |
| Q-support-mobile-apple-login-004 | business.empty-email-account | email 为空且 open_id 未命中时,生成 `{sub}@apple.placeholder`、verified=false 建号(对齐 WeChat placeholder 范式) | DEC-support-mobile-apple-login-005 | conversation |

## Deferred Questions

| ID | Topic | Why Non-blocking Now | Owner Stage | Must Resolve Before | User Informed |
|---|---|---|---|---|---|

## Superseded Decisions

| ID | Superseded By | Previous Decision | Source |
|---|---|---|---|
