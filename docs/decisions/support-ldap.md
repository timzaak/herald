# support-ldap Decision Log

## Active Decisions

| ID | Level | Topic | Decision | Rationale | Decided By | Source | Affects | Reopen When | Supersedes |
|---|---|---|---|---|---|---|---|---|---|
| DEC-support-ldap-001 | D1 | scope.ldap-capability | 本期仅做 LDAP 登录认证（search-then-bind + JIT 建号 + 租户级 LDAP 服务器配置）；不做 LDAP 组→Herald 角色映射，不做后台目录同步（两者如需要各自独立立项） | 2026-08-26 AskUserQuestion 范围三问未获用户回答，agent 按推荐项落地（对齐 DEC-wechat-support-012 的未答处理先例）；推荐依据：与现有 WeChat/Apple/Google 外部身份接入深度一致，工作量最小 | user（未答，agent 按推荐项） | conversation (2026-08-26 AskUserQuestion 无回答) + `.ai/tech-research/support-ldap.md` §1/§4 | prd/design/task | 出现组→角色映射或目录同步的具体客户需求 | — |
| DEC-support-ldap-002 | D1 | provisioning.jit | 首次 LDAP bind 成功即 JIT 建号：`account` 无本地密码（复用 `create_user_without_password`），`provider` 链接 `type='ldap'`、`open_id` 存用户 DN；email 取 LDAP `mail` 属性，缺失时生成 placeholder 且不标记已验证（对齐 DEC-support-mobile-apple-login-005 范式） | 同上未答按推荐项；与现有 OAuth JIT 建号模式（`backend/api-oauth/src/helper.rs`）一致，用户零摩擦接入；本地密码列保持 NULL 使本地密码回退登录天然不可能 | user（未答，agent 按推荐项） | conversation (2026-08-26) + `backend/api-oauth/src/helper.rs` JIT 范式 | prd/design/task | 租户要求仅管理员预建账号可 LDAP 登录 | — |
| DEC-support-ldap-003 | D1 | scope.no-desktop-sso | 不做 Windows 桌面单点登录（SPNEGO/Kerberos 内网免密自动登录）；仅登录页表单（用户名+密码）认证 | 同上未答按推荐项；SPNEGO 依赖 GSSAPI C FFI，违背 DEC-wechat-support-004 纯 Rust/no-openssl 硬约束，且浏览器协商流程显著扩大架构与安全面 | user（未答，agent 按推荐项） | conversation (2026-08-26) + `docs/decisions/wechat-support.md` DEC-004 | prd/design/task | 出现内网域免密 SSO 的具体客户需求（届时独立立项评估 GSSAPI 之外的方案） | — |
| DEC-support-ldap-004 | D2 | dependency.ldap3-rustls-ring | 引入 `ldap3` 0.12 作为 workspace 依赖：`default-features = false`，`features = ["tls-rustls-ring"]`（不启用 `sync`、不启用 GSSAPI/NTLM） | 承接 DEC-wechat-support-004（禁 openssl/native-tls 及一切间接拉入 native-tls 的依赖）；ldap3 默认 features 含 `tls`（= native-tls）必须关闭；依赖树已有 rustls 0.23.36 + ring（经 reqwest/tokio-rustls），`tls-rustls-ring` 零新增 TLS 后端；MSRV 1.82 低于锁定工具链 1.96.1 | agent | `backend/Cargo.lock`（rustls 0.23.36、ring 在树、无 aws-lc-rs）+ `backend/rust-toolchain.toml`（1.96.1）+ crates.io/docs.rs | design/task | ldap3 出不兼容升级或项目 rustls 主版本迁移 | — |
| DEC-support-ldap-005 | D2 | config.storage-realm-config | LDAP 配置存既有 `realm_config`（`config_type='ldap'`，服务账号 bind 密码 `is_secret=true`），复用通用 `/api/configs/{realmId}` 管理 CRUD；v1 不做应用层加密 | 对齐 Stripe/Creem/Wechat 凭据存储主导先例（DEC-wechat-support-007 同结论）；不改变技术路线、依赖与兼容性 | agent | `backend/domain/src/realm_config` + `docs/decisions/wechat-support.md` DEC-007 | design/task | 全 provider 凭据统一应用层加密立项（LDAP 一并受益） | — |
| DEC-support-ldap-006 | D2 | auth.dedicated-endpoint | 新增专用端点（挂 `/api/auth/{realmId}` 下）做 LDAP 登录，完整镜像 `login.rs` 管线（Client App 解析、Turnstile、IP+标识符限流、TOTP/passkey 二因子探测、consent gate、OAuth code 分支、token family、审计 `method="ldap"`）；不在现有 `login.rs` 内部分支 | `UserServiceImpl::login` 内嵌 bcrypt 校验与时序均衡逻辑，LDAP bind 无本地哈希可验、无法复用该路径；`email_otp.rs` 已示范"替代第一因子走专用端点"形态，前端按状态开关展示（`EmailOtpLoginForm` 同型） | agent | `backend/api-auth/src/login.rs` + `backend/domain/src/user/services/basic.rs`（bcrypt 路径）+ `backend/api-auth/src/email_otp.rs`（专用端点先例） | design/task | 登录页统一单表单交互改版 | — |
| DEC-support-ldap-007 | D1 | provisioning.registration-gate | LDAP JIT 建号不受 realm 注册政策门控：LDAP 启用开关即管理员对该目录供给的授权，注册关闭的 realm 员工首登仍自动建号；不新增独立"LDAP 自动建号"子开关 | 行业调研（用户要求基于同类产品通行做法）：Keycloak 目录联合登录不受 realm Registration 设置门控、Zitadel 用 per-IdP auto-register（即配置即授权）、Auth0 企业连接首登落库——同类产品均把企业目录视为受控供给而非公开自注册；LDAP 在本 feature 是凭据权威非账号唯一来源（DEC-001 已排除目录同步），"关公开自注册 + 员工目录登录"是企业主场景，注册门控会打断主场景 | user | conversation (2026-08-26 AskUserQuestion：先反问行业惯例，给出 Keycloak/Zitadel/Auth0 调研后确认"不受门控，LDAP 开关即授权（推荐）") | prd/design/task | 出现需要把目录供给与 LDAP 启用解耦细控的客户需求（届时评估 Zitadel 式自动建号子开关） | — |
| DEC-support-ldap-008 | D1 | matching.email | 用户匹配策略：DN（provider 链接）→ email → 建号；LDAP `mail` 属性由企业管理员在目录中维护，视为可信（等价"已验证"）来源，允许据此登入既有账号 | 镜像 OAuth 四级匹配中可适用层级（`find_or_create_user`：union_id → open_id → email → create，LDAP 无 union_id 概念，DN 即 open_id 载体）；避免员工先有本地账号、后接 LDAP 时 email 撞车导致重复建号或登录失败 | user | conversation (2026-08-26 AskUserQuestion 选推荐项) + `backend/api-oauth/src/helper.rs` | prd/design/task | 出现目录 email 属性不可信的实际案例（目录属性治理差导致误关联） | — |
| DEC-support-ldap-009 | D2 | auth.unique-match | search-then-bind 的用户条目搜索必须唯一命中：命中 0 个或多于 1 个条目时认证失败（按泛化错误返回），不做猜测式绑定 | 猜测式绑定（取第一条命中）在目录存在重名/多条目时是安全漏洞；唯一命中是 search-then-bind 的行业标准行为（Keycloak 同型），无合理替代方案，属 agent 授权的工程取舍 | agent | `.ai/tech-research/support-ldap.md` §4.3 + search-then-bind 行业标准 | prd/design/task | 出现按多值属性（如 uid+org 唯一）消歧的具体目录形态 | — |

## Resolved Questions

| ID | Topic | Resolution | Decision ID | Source |
|---|---|---|---|---|
| Q-support-ldap-001 | scope.ldap-capability | 仅登录认证（不含组角色映射、不含目录同步） | DEC-support-ldap-001 | conversation (2026-08-26 AskUserQuestion 未答，按推荐项) |
| Q-support-ldap-002 | provisioning.jit | 首次 bind 成功 JIT 自动建号（无本地密码 + provider 链接） | DEC-support-ldap-002 | conversation (2026-08-26 AskUserQuestion 未答，按推荐项) |
| Q-support-ldap-003 | scope.desktop-sso | 仅表单登录，不做 SPNEGO/Kerberos 桌面 SSO | DEC-support-ldap-003 | conversation (2026-08-26 AskUserQuestion 未答，按推荐项) |
| Q-support-ldap-004 | provisioning.registration-gate | 不受 realm 注册政策门控，LDAP 开关即供给授权 | DEC-support-ldap-007 | conversation (2026-08-26，用户要求行业调研后确认) |
| Q-support-ldap-005 | matching.email | DN → email → 建号（目录 email 视为可信来源，可登入既有账号） | DEC-support-ldap-008 | conversation (2026-08-26，用户选推荐项) |

## Deferred Questions

| ID | Topic | Why Non-blocking Now | Owner Stage | Must Resolve Before | User Informed |
|---|---|---|---|---|---|

## Superseded Decisions

| ID | Superseded By | Previous Decision | Source |
|---|---|---|---|
