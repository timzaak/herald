# OAuth 与第三方集成产品需求文档 (PRD)

**创建时间**: 2025-01-10
**优先级**: P0

---

## 1. 相关用户故事

> 详细故事与验收标准请查看 `docs/user-stories/` 中对应文档。

### 1.1 故事引用

**Realm Admin:**
- `[US-RA-008]` 配置 Realm 设置 (P0) — 来源 `docs/user-stories/core/realm-admin.md`
  - 角色：Realm Admin
  - 摘要：配置 OAuth Provider，启用第三方登录

**第三方应用开发者:**
- `[US-TP-001]` OAuth 授权码登录 Authorization Code + PKCE (P0) — 来源 `docs/user-stories/auth/third-party-app.md`
  - 角色：第三方应用
  - 摘要：使用 Authorization Code + PKCE 流程安全获取访问令牌
- `[US-TP-002]` 验证用户登录状态 (P0) — 来源 `docs/user-stories/auth/third-party-app.md`
  - 角色：第三方应用
  - 摘要：验证用户登录状态和身份，保护应用资源
- `[US-TP-003]` 检查用户权限 (P0) — 来源 `docs/user-stories/auth/third-party-app.md`
  - 角色：第三方应用
  - 摘要：检查用户是否有权限访问特定资源，实现细粒度访问控制
- `[US-TP-006]` 第三方应用授权登录 (P0) — 来源 `docs/user-stories/auth/third-party-app.md`
  - 角色：第三方应用开发者
  - 摘要：使用 OAuth Provider 进行第三方登录，快速接入系统
- `[US-TP-006]` 处理异常情况 (P1) — 来源 `docs/user-stories/auth/third-party-app.md`
  - 角色：第三方应用
  - 摘要：正确处理各种异常情况，提供友好体验
- `[US-TP-007]` 会话管理 (P1) — 来源 `docs/user-stories/auth/third-party-app.md`
  - 角色：第三方应用
  - 摘要：管理用户会话，实现 SSO 和登出
- `[US-TP-008]` 第三方 API 认证 (P0) — 来源 `docs/user-stories/auth/third-party-app.md`
  - 角色：第三方应用
  - 摘要：使用 API Key 认证调用 Herald 第三方接口，安全集成 Herald 系统
- `[US-TP-009]` 查询订阅状态 (P0) — 来源 `docs/user-stories/auth/third-party-app.md`
  - 角色：第三方应用
  - 摘要：查询客户端应用的订阅状态，根据订阅状态提供相应功能
- `[US-TP-015]` 第三方 Web SPA 发起 SSO 登录 (P0) — 来源 `docs/user-stories/auth/third-party-app.md`
  - 角色：第三方应用开发者
  - 摘要：从 Web SPA 发起 Herald SSO 登录，无需额外后端即可完成认证
- `[US-TP-016]` 第三方后端用授权码换取令牌 (P0) — 来源 `docs/user-stories/auth/third-party-app.md`
  - 角色：第三方应用开发者
  - 摘要：后端用授权码和 PKCE 验证换取令牌，安全完成认证

**普通用户:**
- `[US-RU-008]` 访问第三方应用 (P0) — 来源 `docs/user-stories/core/regular-user.md`
  - 角色：普通用户
  - 摘要：使用 Herald 账号登录第三方应用，获得 SSO 体验
- `[US-RU-010]` 从第三方 Web 应用跳转登录 (P0) — 来源 `docs/user-stories/core/regular-user.md`
  - 角色：普通用户
  - 摘要：从第三方应用跳转到 Herald 完成认证后自动返回，无缝使用第三方服务

**主管理员:**
- API Key 管理 (P0) — 来源 `docs/user-stories/core/admin-realm.md`
  - 角色：主管理员
  - 摘要：创建和管理第三方 API Keys，控制第三方访问

**Client App 设置:**
- `[US-TP-008]` 配置 Client App 跳转地址白名单 (P0) — 来源 `docs/user-stories/auth/client-app-settings.md`
  - 摘要：redirect_uri 白名单精确匹配
- `[US-TP-010]` 启用/禁用 Client App (P0) — 来源 `docs/user-stories/auth/client-app-settings.md`
  - 摘要：禁用的 Client App 拒绝 OAuth 授权

### 1.2 优先级汇总

| 优先级 | 数量 | 关键故事 |
|--------|------|----------|
| P0 | 12 | 配置 OAuth Provider、Authorization Code + PKCE 流程、Web SPA SSO、令牌交换、API Key 认证、权限检查、订阅查询、第三方跳转登录、白名单配置 |
| P1 | 2 | 异常处理、会话管理 |
| P2 | 0 | - |

---

## 2. 范围界定

### 2.1 包含功能

- OAuth Provider 配置管理（Google、GitHub、Facebook、Apple、WeChat、WeChat Mini Program）及 Provider 启用/禁用控制
- Authorization Code + PKCE 流程（OAuth 2.1 推荐模式），支持第三方 SPA 发起授权请求
- 用户在 Herald 登录页完成认证后生成 authorization_code，通过 redirect_uri 回传第三方
- 第三方后端用 authorization_code + code_verifier 换取 access_token
- State 校验（防 CSRF）和 authorization_code 一次性使用（防重放）
- redirect_uri 白名单精确匹配（origin + port 一致）
- TOTP 二次认证流程中保持 OAuth 上下文
- 前端登录页透传 OAuth 参数，处理后端返回的 redirectTo 跳转
- 第三方 API 认证（API Key 方式），支持用户登录状态验证、权限检查和订阅状态查询
- API Key 绑定到特定 Client App（Client App Scope），普通 Client App 的 API Key 仅能访问该 App 所属资源，Admin API Client 的 Key 可跨 App 访问
- API Key 轮换（Rotate），生成新密钥并立即失效旧密钥
- API Key Realm 隔离，API Key 使用统计
- OAuth 2.0 Device Authorization Grant (RFC 8628)，详见独立 PRD `docs/prd/auth/device-code.md`
- Herald 作为 OAuth Client 的 SSO 登录，通过 `/api/oauth/{realmId}/{provider}/login` 和 `/{provider}/callback` 路径实现第三方 Provider 登录

### 2.2 不包含功能 (Out of Scope)

- Refresh Token（server-side token 当前不支持令牌刷新）；浏览器 token 变体支持旋转 refresh token（见 [自建用户 UI](/docs/prd/integration/custom-user-ui.md) D-TOK-01）
- Token 撤销：server-side token 当前不支持撤销；浏览器 token 变体支持即时吊销（见 [自建用户 UI](/docs/prd/integration/custom-user-ui.md) D-TOK-02）
- OAuth 2.0 Scope 管理（没有细粒度 scope 授权页面）
- 用户主动授权/拒绝授权页面（当前授权自动完成，用户无需手动批准）
- Implicit Flow（已被 OAuth 2.1 废弃）
- API Key 管理界面（后续优化）
- 审计日志（后续优化）
- Webhooks 和 GraphQL 支持（后续优化）

### 2.3 依赖项

- Realm 系统 — OAuth Config 属于 Realm 级别；API Key 绑定到 realm
- 权限管理系统 — Realm Admin 权限检查；权限检查 API
- Client App 系统 — OAuth 回调验证、redirect_uri 白名单
- 用户认证系统 — 提供登录和会话管理
- Redis 缓存 — state、authorization_code 存储
- TOTP 系统 — TOTP 二次认证
- 订阅系统 — 订阅状态查询
- Session Token 验证 — 第三方 API 中的 session token 校验

---

## 3. 需求概述

### 3.1 功能描述

为 Herald 多租户系统提供完整的 OAuth 与第三方集成能力，包括两个核心功能域：

1. **OAuth Provider 配置管理**：允许 Realm Admin 为每个 Realm 配置第三方登录提供商（Google、GitHub、Facebook、Apple、WeChat、WeChat Mini Program），管理 Provider 的启用/禁用状态和 OAuth 凭证。用户可通过已配置的 Provider 实现 SSO 登录。

2. **第三方应用 OAuth 集成 (Authorization Code + PKCE)**：基于 OAuth 2.1 标准流程，允许第三方 Web 应用通过 Herald 系统验证用户身份。第三方 SPA 发起授权请求，用户在 Herald 完成认证后，通过授权码安全交换令牌。

3. **第三方 API 接入**：第三方应用通过 API Key 认证接入 Herald 系统，实现用户登录状态验证、权限检查和订阅状态查询等功能。Ext API 还提供 Realm、User、Client App、Billing（订阅计划查询）、Points（余额查询与消费）等完整管理能力。详细内容参考各自独立 PRD。

4. **Herald OAuth Client SSO 登录**：Herald 本身作为 OAuth Client，通过通用登录路径 `/api/oauth/{realmId}/{provider}/login` 发起第三方 Provider 授权，回调路径 `/{provider}/callback` 接收授权结果并完成用户关联登录。通用路径适用于 Google、GitHub、Facebook、Apple；微信网站登录使用微信专属路由，小程序通过 code2session 直连接口完成登录，见 [wechat-oauth.md](wechat-oauth.md)。

5. **OAuth 2.0 Device Authorization Grant**：完整实现 RFC 8628 设备授权流程，包含 authorize、token、verify、confirm 四个独立端点，详见独立 PRD `docs/prd/auth/device-code.md`。

### 3.2 关键特性

- OAuth Provider 是 Realm 级别资源，由 Realm Admin 管理，与普通 Realm Config（key-value）独立
- Authorization Code + PKCE 为 OAuth 2.1 推荐模式，前端只获得 code，后端换 token，安全性高于旧 Implicit Flow
- 所有 OAuth 凭证（state、authorization_code）为一次性使用，防止重放攻击
- redirect_uri 白名单精确匹配（origin + port），禁止前缀匹配，防止开放重定向；例外：第一方 Client App（内置管理控制台/用户账户中心）跳过 redirect_uri 白名单校验（其回调固定为 Herald 自有前端路由）
- TOTP 二次认证流程中保持 OAuth 上下文，认证完成后同样返回 redirectTo
- API Key 绑定到特定 realm，实现跨租户隔离
- API Key 可绑定到特定 Client App（Client App Scope），普通 Client App 的 Key 仅能访问该 App 资源，Admin API Client 的 Key 可跨 App 访问
- API Key 支持轮换（Rotate），旧密钥立即失效，返回新密钥（仅展示一次）
- 命名约定：推荐使用 "Provider" 或 "Identity Provider"，而非 "OAuth Config"，符合行业标准
- Herald 本身可作为 OAuth Client，通过通用路径实现第三方 Provider SSO 登录
- 支持 OAuth 2.0 Device Authorization Grant (RFC 8628)，适用于无浏览器设备

---

## 4. 业务规则与状态

### 4.1 业务规则

**OAuth Provider 管理:**
- Provider 配置为 Realm 级别资源，仅 Realm Admin 可管理
- 每个 Realm 可配置多个 OAuth Provider（Google、GitHub、Facebook、Apple、WeChat、WeChat Mini Program）
- Provider 可独立启用/禁用；禁用的 Provider 不在登录页显示
- Provider 配置包含 Client ID、Client Secret、Scopes 和启用状态
- 编辑 Provider 时 Client Secret 为可选（留空表示保持原值）；前端不应显示已存储的 Client Secret
- 删除 Provider 需要二次确认
- WeChat Provider Scope 仅允许 `snsapi_login`；WeChat Mini Program 不使用 Scope

**OAuth 授权流程:**
- 第三方 SPA 必须使用 Authorization Code + PKCE 流程，不支持 Implicit Flow
- Client App 必须存在且已启用，redirect_uri 必须在白名单中精确匹配（origin + port 完全一致；第一方 Client App 例外，见上）
- Google One Tap 与 Apple 原生（Sign in with Apple）直连登录由专属 PRD 承载（[google-one-tap.md](google-one-tap.md)、[support-mobile-apple-login.md](support-mobile-apple-login.md)），不经本 PRD 的 authorize/code 交换流
- State 和 authorization_code 必须一次性使用，验证后立即删除
- PKCE 的 code_challenge 必须使用 S256 方法（SHA256）
- 无 OAuth 参数时，登录行为与现有普通登录完全一致
- OAuth 参数不完整时（缺少任意一项），应显示错误提示，不静默降级为普通登录
- 未认证 OAuth 端点实施 per-IP 速率限制，超限返回 429：`/authorize` 与 `/token` 默认 30 次/分钟/IP；发起 Provider 登录（含上游 JWKS/code2session 拉取）与 Device Authorization Grant 的 authorize 端点默认 10 次/分钟/IP（阈值为后端统一常量管理的运行默认值，第三方集成方须处理 429）

**第三方 API 接入:**
- 第三方应用使用 API Key（通过 X-API-Key header）认证，与 session token 认证体系分离
- API Key 绑定到特定 realm，只能访问所属 realm 的资源
- API Key 可绑定到特定 Client App（Client App Scope），绑定后只能访问该 Client App 所属资源
- Admin API Client（`admin-api-client`）的 API Key 不受 Client App Scope 限制，可跨 App 访问
- 未绑定 Client App 的 API Key 也不受 Client App Scope 限制
- API Key 支持轮换（Rotate），调用 `POST /api/api-keys/{realmId}/{apiKeyId}/rotate` 生成新密钥，旧密钥立即失效（旧缓存条目通过 TTL 自然过期）
- API Key 有启用/禁用和过期时间控制
- 记录 API Key 最后使用时间（节流更新：每分钟最多一次写库）
- 无效或缺失 API Key 返回 401；过期或禁用 API Key 返回 401
- 无效 session token 在权限检查时返回 `allowed: false`，而非报错

**Herald OAuth Client SSO 登录:**
- Herald 作为 OAuth Client 通过 `/api/oauth/{realmId}/{provider}/login` 发起第三方 Provider 授权
- 回调路径 `/{provider}/callback` 接收 Provider 授权结果，创建或关联 OAuth 用户账户，完成 SSO 登录
- 支持所有已配置的 Provider 类型（Google、GitHub、Facebook、Apple、WeChat、WeChat Mini Program）
- OAuth 账户通过 open_id 关联用户；未命中 provider 身份时才按 Email 匹配。回调是由一次性 state 约束的未认证入口，不以浏览器中是否另有 Herald 会话作为关联依据；Email 命中既有账号时 Provider 返回的邮箱必须已验证，未验证邮箱不得用于关联既有账号（防止经 Provider 未验证邮箱接管既有密码账号，如 GitHub 非主邮箱）。唯一例外是由已验签 provider subject 确定性生成且完全匹配的内部占位邮箱，用于恢复“账号已创建但 provider link 未落账”的失败重试
- **自动建号受 Realm 注册政策门控（注册政策优先）**：当 Provider 凭证未命中已有用户、需要新建账号时，必须先检查当前 Realm 的注册开关（`registration.enabled` / `is_registration_enabled`）。Realm 未开启自动注册时，OAuth 路径**不得**绕过注册政策自动建号，返回注册未开放提示（实现上以 `409 conflict` 表达），引导用户走显式注册入口。已命中已有用户的关联登录不受此门控影响。该原则与邮箱验证码登录一致（见 `docs/prd/auth/email-otp-login.md` §4.1「注册政策优先」），对所有 OAuth Provider（Google、GitHub、Facebook、Apple、WeChat 等）统一适用。

**Herald 作为身份 Broker（brokered downstream-state redirect）:**
- 当第三方 Client App 已在 Herald `/authorize` 发起自身的 Authorization Code + PKCE 授权事务时，可在跳转 `/api/oauth/{realmId}/{provider}/login` 时携带 `downstream_state` 参数，将该事务标识传递给 Herald
- `downstream_state` 必须指向一个已存在、未消费、与当前 realm/client_id/redirect_uri/code_challenge 完整绑定的下游授权事务；校验失败拒绝发起 Provider 授权
- Provider 回调 `/{provider}/callback` 时，若上下文携带有效的 `downstream_state`，Herald 不为该用户创建 Herald 自身会话，而是消费该下游 state（一次性，GETDEL 语义）并签发一个一次性 `authorization_code`，重定向回下游 Client App 的 `redirect_uri`（携带 `code` 与 `state`）
- 下游 Client App 随后通过既有 `/token` 端点 + PKCE 校验换取令牌，与普通 Authorization Code + PKCE 流程一致
- 该流程使 Herald 在充当 OAuth Client（对接 Google 等 IdP）的同时充当下游 Client App 的身份 Broker，把 IdP 认证结果转换为下游可用的授权码

**TOTP + OAuth 兼容:**
- TOTP 临时会话中保存 OAuth 上下文（oauth_client_id、redirect_uri、state）
- TOTP 验证成功后检查临时会话中的 OAuth 字段，有 OAuth 字段时走同样的 authorization_code 生成逻辑

**异常处理:**
- 用户拒绝授权（OAuth provider 返回 access_denied）：显示友好错误信息，引导使用其他登录方式
- State Token 验证失败（不存在或已过期）：提示"登录链接已过期，请重新发起登录"
- 授权码无效或过期：提示"授权失败，请重新登录"
- 获取用户信息失败：提示"无法获取用户信息，请联系管理员"
- Email 冲突：Provider 邮箱已验证时自动关联到已有用户；Provider 邮箱未验证时拒绝以该邮箱关联既有账号。OAuth 回调不依赖或信任当前浏览器会话，关联授权来自已校验的 provider 凭证与一次性 state
- Provider 被禁用/删除：在列表 API 中过滤掉禁用的 Provider

### 4.2 关键状态与异常

- **Provider 状态**: Enabled / Disabled — 禁用的 Provider 不在登录页展示、不参与授权流程
- **Client App 状态**: Enabled / Disabled — 禁用的 Client App 拒绝 OAuth 授权；该检查实时生效，禁用同时使其名下 API Key 的鉴权立即失效（包括缓存命中路径，返回 401）
- **API Key 状态**: Enabled / Disabled / Expired — 无效状态均返回 401
- **authorization_code**: 一次性，使用后立即失效（Redis 删除）
- **state token**: 一次性，校验后立即失效（Redis 删除），TTL 5 分钟
- **OAuth 上下文参数**: oauthClientId、redirectUri、state 三项必须完整才触发 OAuth 流程

---

## 5. 功能需求

### 5.1 核心需求

**OAuth Provider 配置管理:**
- Realm Admin 可在 Settings 页面（Providers Tab）管理 OAuth Provider 配置
- 支持 Provider 的增删改查操作，包含 Provider Type、Client ID、Client Secret、Scopes、Enabled 字段
- Provider 列表展示名称、Client ID、状态、Scopes 和操作按钮
- 编辑时 Client Secret 为可选字段，前端提示"留空保持现有密钥不变"
- 各 Provider Type 有默认 Scopes 配置
- 登录页动态加载已启用的 Provider，显示对应的登录按钮

**第三方应用 OAuth 授权:**
- 支持 authorize 请求：校验 Client App 存在且启用、redirect_uri 在白名单中，存储 state 到 Redis，重定向到 Herald 登录页
- 支持用户认证 + 授权码生成：登录成功后校验 state，生成 authorization_code 存入 Redis（关联 code_challenge、client_id、redirect_uri），返回 redirectTo 指向第三方 callback
- 支持令牌交换：校验 code 有效未使用、client_id 和 redirect_uri 匹配、PKCE 校验通过后创建 session 返回 access_token

**第三方 API 接入:**
- API Key 认证系统：提取验证 X-API-Key header，校验 API Key 有效且未过期，更新使用统计
- API Key Client App Scope 校验：绑定了 Client App 的 API Key 仅能访问该 App 的资源，Admin API Client 的 Key 除外
- API Key 轮换：通过 `POST /api/api-keys/{realmId}/{apiKeyId}/rotate` 轮换密钥，旧密钥立即失效，返回新明文密钥（仅展示一次）
- 权限检查：第三方应用使用 API Key + 用户 session token，检查用户对指定资源的权限，支持 batch 检查
- 订阅状态查询：第三方应用使用 API Key 查询客户端应用的订阅状态，无订阅时返回 free tier 信息
- Ext API 完整能力：除权限检查和订阅查询外，还提供 Realm（创建/列表/查询）、User（创建/列表/查询）、Client App（创建/列表/查询）、Billing（订阅计划/分配查询）、Points（余额查询/消费/交易查询）管理接口。详细内容参考各自独立 PRD

### 5.2 验收目标

- OAuth Provider 可在管理后台完成完整的增删改查，Provider 启用/禁用即时生效
- 第三方 SPA 可成功发起 Authorization Code + PKCE 授权流程，完成用户认证和令牌交换
- 所有一次性凭证（state、authorization_code）使用后不可重用
- redirect_uri 非白名单地址被拒绝，前缀匹配被拒绝
- TOTP 二次认证场景下 OAuth 流程可正常完成
- 第三方应用可通过 API Key 完成用户登录验证、权限检查和订阅查询
- API Key 实现严格的 realm 隔离，不可跨租户访问
- 无效/过期/禁用的凭证均返回正确的错误状态
- OAuth 参数不完整时前端明确报错，不静默降级

---

## 6. API 相关约束

**适用性**: 适用

- OAuth Provider 管理接口为 Realm 级别资源，仅 Realm Admin 可访问
- 第三方 OAuth 授权接口涉及 authorize 和 token 两个核心能力：authorize 负责 Client App 校验和登录页重定向，token 负责授权码校验和令牌签发
- redirect_uri 校验必须精确匹配白名单（禁止前缀匹配）
- authorization_code 和 state 必须一次性使用（验证后立即删除，非标记）
- PKCE 的 code_challenge 必须使用 S256 方法（SHA256）
- 第三方 API 接入使用独立的 API Key 认证体系（X-API-Key header），与 session token 认证分离
- API Key 绑定 realm，第三方接口只能访问所属 realm 的资源
- API Key 可绑定 Client App（Client App Scope），绑定后仅能访问该 Client App 资源；Admin API Client 和未绑定 Client App 的 Key 不受此限制
- API Key 轮换端点 `POST /api/api-keys/{realmId}/{apiKeyId}/rotate`，需要 `api_keys.manage` 权限
- 权限检查接口支持 batch 模式（多个 rules），无效 session token 返回 `allowed: false` 而非报错
- 订阅查询接口在无订阅时返回 free tier 信息
- Client App 禁用时拒绝所有 OAuth 授权请求
- Herald OAuth Client SSO 路径：`GET /api/oauth/{realmId}/{provider}/login`（发起授权）和 `GET|POST /api/oauth/{realmId}/{provider}/callback`（回调处理），用于 Herald 自身通过第三方 Provider 登录
- OAuth 2.0 Device Authorization Grant 完整实现（RFC 8628），端点包含 authorize、token、verify、confirm，详见 `docs/prd/auth/device-code.md`
- 详细端点契约、认证方式和错误模型应下沉到技术设计或接口说明文档

---

## 7. 前端/交互约束

**适用性**: 适用

- OAuth Provider 配置入口在 Settings 页面的 Providers Tab，与 Turnstile、Registration 并列
- Provider 列表以表格形式展示名称、Client ID、状态（Enabled/Disabled 用不同颜色 Badge 区分）、Scopes 和操作按钮（编辑、启用/禁用切换、删除）
- 新增/编辑 Provider 通过对话框表单完成，字段包含 Provider Type（下拉选择）、Client ID、Client Secret（编辑时可选，提示"留空保持不变"）、Scopes（多选）、Enabled 开关
- 删除 Provider 需要二次确认交互
- 登录页动态加载已启用的 Provider 列表，展示为独立的登录按钮
- 登录页 search schema 须支持 OAuth 上下文参数（oauthClientId、redirectUri、state）
- OAuth 参数完整（三项都存在）时提交登录须一并传给后端；不完整时显示错误提示，不静默降级为普通登录
- 后端返回 redirectTo 时直接跳转第三方 callback（不经前端安全重定向检查，安全性由后端白名单保证）
- TOTP 完成后同样支持 redirectTo 跳转
- 无 OAuth 参数时登录行为与现有普通登录完全一致
- 涉及第三方接入时，明确区分 Herald 后台完成的流程和第三方应用/外部平台完成的流程

---

## 8. 已确认决策

### 8.1 已确认决策

- 采用 Authorization Code + PKCE 替代旧 Implicit Flow，符合 OAuth 2.1 标准
- OAuth Provider 配置独立于 Realm Config（key-value），使用独立的 Provider 实体管理
- 命名使用 "Provider" / "Identity Provider"，避免与 OAuth Config 技术术语混淆
- redirect_uri 白名单采用精确匹配策略（origin + port），不使用前缀匹配
- 第三方 API 认证使用独立 API Key 体系，与 session token 分离
- API Key 支持 Client App Scope 绑定，限制 API Key 仅访问特定 Client App 资源；Admin API Client 不受此限制
- OAuth Provider 支持 WeChat 和 WeChat Mini Program，WeChat Scope 限制为 `snsapi_login`，WeChat Mini Program 不使用 Scope
- Herald 自身作为 OAuth Client 通过通用登录/回调路径实现 SSO 登录
- OAuth 2.0 Device Authorization Grant (RFC 8628) 独立实现，详见 `docs/prd/auth/device-code.md`
- 编辑 Provider 时 Client Secret 可选留空（保持原值），前端不回显已存储的 Secret
- State 和 authorization_code 存储在 Redis，一次性使用后删除

---

## 9. 参考资料

- 用户故事：`docs/user-stories/auth/third-party-app.md`
- 用户故事：`docs/user-stories/core/realm-admin.md`
- 用户故事：`docs/user-stories/core/regular-user.md`
- 用户故事：`docs/user-stories/core/admin-realm.md`
- 用户故事：`docs/user-stories/auth/client-app-settings.md`
- 相关 PRD：`docs/prd/core/realm-settings.md`
- 相关 PRD：`docs/prd/integration/client-app.md`
- 相关 PRD：`docs/prd/auth/permissions.md`
- 相关 PRD：`docs/prd/auth/totp.md`
- 相关 PRD：`docs/prd/auth/device-code.md`（Device Authorization Grant）
