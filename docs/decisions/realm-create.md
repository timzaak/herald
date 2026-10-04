# realm-create Decision Log

> 本账本维护 `realm-create` feature 在 Decision、PRD、Tech Research、Design、Task 之间的决策连续性。
> 记录规则、稳定 ID 和用户决策暴露门禁统一参考 `${CLAUDE_PLUGIN_ROOT}/protocols/decision-continuity-contract.md`。

## Active Decisions

| ID | Level | Topic | Decision | Rationale | Decided By | Source | Affects | Reopen When | Supersedes |
|---|---|---|---|---|---|---|---|---|---|
| DEC-realm-create-001 | D1 | entry.host-and-exclusivity | 自助开通注册页面由 admin realm 托管并对未登录访客公开访问；该能力为 admin realm 独有，其他 realm 不承载平台级开通入口 | 用户明确要求“admin realm 独有”；admin realm 是平台级 realm 生命周期的归属（见 `docs/prd/core/realm.md` §4.1） | user | conversation | prd/frontend/design | 用户要求开通入口可由非 admin realm 承载 | — |
| DEC-realm-create-002 | D1 | provisioning.model | 注册即开通：访客提交注册信息后系统立即开通新 realm，注册者成为该 realm 的 realm-admin；注册环节不引入套餐选择或支付 | 用户表述“注册帐号，并为其开通 realm”描述即时开通；仓库已有独立 billing 系统，付费由其后续承载 | user + repository-fact | conversation + `docs/prd/core/realm.md` | prd/design/task | 用户要求注册时选套餐或必须支付后才开通 | — |
| DEC-realm-create-003 | D1 | provisioning.reuse | 复用既有 realm 初始化机制（RBAC 默认角色/权限/策略、`admin-web-console`、`admin-api-client`、`registration.enabled=false`、Normal 状态管理员），不新建并行开通路径 | `docs/prd/core/realm.md` §3.2/§4.1 已定义稳定初始化规则；Rule 2 最小改动 | repository-fact + agent | `docs/prd/core/realm.md` | prd/design/task | 初始化规则在正式 PRD 中变更 | — |
| DEC-realm-create-004 | D1 | scope.single-realm-per-registration | 一次注册对应一个新 realm；注册者即该 realm 的 realm-admin；平台级“一个账号拥有多个 realm”不在本 PRD 范围 | 最简可审阅模型；多 realm 归属需要平台账号层，超出本轮“注册即开通”意图 | agent | conversation | prd/design | 用户要求支持单账号多 realm 或跨 realm 所有者 | — |
| DEC-realm-create-005 | D1 | stories.new-vs-reuse | 新建独立用户故事（actor：SaaS 自助注册访客），不复用 `US-AR-001`（手动内部开通，actor 为 Admin Realm 管理员，入口与流程不同） | 两者 actor、入口、前置权限与验收目标不同；Rule 6 不合并冲突模式 | agent | `docs/user-stories/core/admin-realm.md` | prd/user-stories | 用户确认两者应合并为同一旅程 | — |
| DEC-realm-create-006 | D1 | abuse.email-verification-before-access | 新 realm 管理员账号沿用既有 realm-create 行为：创建即 Normal（已验证），注册后可立即进入新 realm；是否强制“邮箱验证后才允许访问”作为防滥用细节延期 | 与既有 `docs/prd/core/realm.md` §3.2 行为一致；防滥用阈值与策略为技术设计细节（见 Q-realm-create-001） | repository-fact + agent | `docs/prd/core/realm.md` | prd/design | Q-realm-create-001 决议要求邮箱前置验证 | — |
| DEC-realm-create-007 | D0 | abuse.ip-rate-limit | 同一 IP 每 24 小时最多自助注册开通 2 个 realm，超出后注册被拒绝并提示限额 | 用户明确要求（安全防滥用）；防止刷号抢占 realm 标识 | user | conversation | prd/design/task | 用户调整限额阈值或窗口 | — |
| DEC-realm-create-008 | D0 | abuse.turnstile-binding | 自助注册页面启用 Cloudflare Turnstile 人机验证，条件是 admin realm 侧的 Turnstile 已开启（即绑定自助注册页面的 Client App 的 Turnstile 配置为启用时才强制人机验证；未开启时不强制） | 用户明确要求“有 cloudflare 验证码如果 admin realm 开启了的话”；仓库事实——Turnstile 配置归属 Client App 级（`docs/prd/integration/client-app.md` §4.1 D-PROTECT-01），未认证身份端点按请求绑定 Client App 的 Turnstile 配置执行，非 realm 级独立开关 | user + repository-fact | conversation + `docs/prd/integration/client-app.md` | prd/design/task | 用户要求 Turnstile 与 Client App 解耦、作为独立 realm 级开关 | — |
| DEC-realm-create-009 | D0 | governance.feature-toggle | 自助开通整体为一个平台开关（Admin Realm 管理员可开启/关闭），关闭后访客无法自助注册；该开关从 P1 提升为本 PRD 必备能力 | 用户明确要求“这个功能要是一个开关” | user | conversation | prd/user-stories | 用户要求移除开关或调整其层级 | — |
| DEC-realm-create-010 | D2 | backend.policy-bypass | 新增 `RealmService::create_realm_self_service(request, audit_ctx)`（不调用 `can_create_realm` policy gate），由 signup handler 自行保证前置校验（开关 + Turnstile + IP 限额）；既有 admin/ext 两条 `create_realm` 鉴权不变，共享抽出后的私有 `create_realm_inner` | Rule 2/3 最小改动；不污染既有鉴权路径；显式命名“公开开通”新语义；备选方案（系统 Identity 特权、复用 admin 鉴权）扩散面大 | agent | `.ai/design/realm-create.md` §4.1/§5.1 | 需要在非公开端点也绕过 policy，或既有 create_realm 鉴权语义变更 | — |
| DEC-realm-create-011 | D2 | abuse.signup-count-semantics | 同 IP 24h 限流计数口径为“进入开通阶段的尝试计数”（在通过校验与 Turnstile 后、调用 create_realm 前执行 `rate_limit_hit`，失败不回退）；满足验收“同 IP 第 3 次 429”；若线上出现失败请求耗尽配额投诉，改为仅成功计数（重开条件） | 复用既有 `rate_limit_hit`（先 INCR 再判断）；避免预检+成功后记录两段式复杂度；DEC-007 措辞“开通”与此口径在验收脚本上等价 | agent | `.ai/design/realm-create.md` §4.1/§7 | 用户要求仅计成功开通，或线上反馈失败请求误耗配额 | — |
| DEC-realm-create-012 | D2 | session.target-realm | signup 成功后签发**新 realm** 的 `admin-web-console` first-party token（`create_first_party_token_family`），而非 admin realm 的 token；响应携带 `realmId`/`realmName` 供前端切换路由上下文 | US-SR-002 要求“立即进入新 realm 管理控制台”；create_realm 已在新 realm 创建 first-party admin-web-console 与 Normal 管理员，签发无传播延迟 | repository-fact + agent | `.ai/design/realm-create.md` §4.1/§5.1 | 用户要求签发 admin realm token 或要求额外登录步骤 | — |
| DEC-realm-create-013 | D2 | governance.toggle-storage | 平台开关存于 admin realm 的 `realm_config`（新增 `ConfigType::PlatformSignup`，`config_key="enabled"`，`config_value="true"/"false"`），不新建表；读取缺失按 `false`（fail-closed） | Rule 2 最小改动；realm_config 是 realm 级配置唯一存储；admin realm 是平台级配置自然归属 | agent | `.ai/design/realm-create.md` §4.3 | prd/design | 用户要求独立平台配置表或独立开关服务 | — |
| DEC-realm-create-014 | D2 | api.conflict-status-code | realmSlug 已占用时 signup 返回 **400**（沿用既有 realm 仓库 `CoreError::BadRequest("Realm with ID '...' already exists")` 与 admin `create_realm` 的 400 约定），而非设计 §4.2.2 理想化的 409 | 仓库事实 + `backend/api/src/application/http/realm/crud.rs` OpenAPI「400 - ID already exists」；signup 复用同一 `create_realm` 仓库路径；仅为 signup 单独改 409 会分裂两个调用方 | repository-fact + agent | `backend/infra/src/realm/mod.rs` + `backend/api/src/application/http/realm/crud.rs` | api/test/frontend | 用户要求 realmSlug 冲突统一为 409（届时需同时改仓库返回 `CoreError::Conflict` 并同步 admin/ext 路径） | — |

## Resolved Questions

| ID | Topic | Resolution | Decision ID | Source |
|---|---|---|---|---|
| Q-realm-create-000 | scope.billing | 注册环节不计费，注册即开通；付费由既有 billing 系统后续承载（曾通过 AskUserQuestion 追问，用户跳过，按最简且符合用户措辞的方向裁决） | DEC-realm-create-002 | conversation |
| Q-realm-create-001 | abuse.controls | 防滥用边界已由用户确认：同一 IP 24 小时上限 2 个（DEC-007）；Turnstile 人机验证按绑定 Client App 的 Turnstile 配置强制（DEC-008）；其余细节（IP 识别方式、窗口实现、失败计数行为）下沉技术设计 | DEC-realm-create-007, DEC-realm-create-008 | conversation |
| Q-realm-create-002 | quota.free-tier | **本轮不引入免费层配额或试用期**；维持 DEC-002“注册即开通、无资源上限”语义。重开条件（“若引入套餐/试用模型则重开 PRD 范围”）未满足——本设计恰好不引入任何配额/试用，故不重开 PRD。owner 已从 t-design 推进至解决 | DEC-realm-create-002 | `.ai/design/realm-create.md` §1.3/§1.4 |
| Q-realm-create-004 | abuse.email-verification-before-access | **不增加邮箱前置验证门禁**；沿用 DEC-006“创建即 Normal（已验证）”，signup 后立即签发会话进入控制台。重开条件（“Q-001 决议要求邮箱前置验证”）未满足——Q-001 决议（DEC-007/008）只规定 IP 限额与 Turnstile；US-SR-002 明确“无需额外审核或等待”。owner 已从 t-design 推进至解决 | DEC-realm-create-006 | `.ai/design/realm-create.md` §1.4 |

## Deferred Questions

| ID | Topic | Why Non-blocking Now | Owner Stage | Must Resolve Before | User Informed |
|---|---|---|---|---|---|
| Q-realm-create-003 | governance.feature-toggle-default | 平台自助开通开关的默认值（默认开启 / 默认关闭）属运营决策，不改变功能存在性与验收目标 | t-task | t-run | yes |

## Superseded Decisions

| ID | Superseded By | Previous Decision | Source |
|---|---|---|---|
