# OAuth 与第三方集成产品需求文档 (PRD)

**创建时间**: 2025-01-10
**优先级**: P0

---

## 1. 相关用户故事

> 详细故事与验收标准请查看 `docs/user-stories/` 中对应文档。

| US-ID | 标题 | 优先级 | 来源 |
|-------|------|--------|------|
| — | 配置 OAuth Provider（无专属故事，配置入口随 Realm 设置交付） | P0 | — |
| US-TP-001 | OAuth 授权码登录 Authorization Code + PKCE | P0 | `docs/user-stories/auth/third-party-app.md` |
| US-TP-002 | 验证用户登录状态 | P0 | `docs/user-stories/auth/third-party-app.md` |
| US-TP-003 | 检查用户权限 | P0 | `docs/user-stories/auth/third-party-app.md` |
| US-TP-006 | 处理异常情况 | P1 | `docs/user-stories/auth/third-party-app.md` |
| US-TP-007 | 会话管理 | P1 | `docs/user-stories/auth/third-party-app.md` |
| US-BI-007 | 第三方应用查询订阅状态（SDK 集成） | P0 | `docs/user-stories/billing/subscription.md` |
| US-TP-015 | 第三方 Web SPA 发起 SSO 登录 | P0 | `docs/user-stories/auth/third-party-app.md` |
| US-TP-016 | 第三方后端用授权码换取令牌 | P0 | `docs/user-stories/auth/third-party-app.md` |
| US-RU-008 | 访问第三方应用 | P0 | `docs/user-stories/core/regular-user.md` |
| US-RU-010 | 从第三方 Web 应用跳转登录 | P0 | `docs/user-stories/core/regular-user.md` |
| — | API Key 管理 | P0 | `docs/user-stories/core/admin-realm.md` |
| US-TP-008 | 配置 Client App 跳转地址白名单 | P0 | `docs/user-stories/auth/client-app-settings.md` |
| US-TP-010 | 启用/禁用 Client App | P0 | `docs/user-stories/auth/client-app-settings.md` |

---

## 2. 范围界定

### 2.1 包含功能

- OAuth Provider 配置管理（Google、GitHub、Facebook、Apple、Discord、WeChat、WeChat Mini Program）及 Provider 启用/禁用控制
- Authorization Code + PKCE 流程（OAuth 2.1 推荐模式），支持第三方 SPA 发起授权请求
- 用户在 Herald 登录页完成认证后生成 authorization_code，通过 redirect_uri 回传第三方
- 第三方后端用 authorization_code + code_verifier 换取 access_token
- State 校验（防 CSRF）和 authorization_code 一次性使用（防重放）
- redirect_uri 白名单精确匹配（origin + port 一致）
- TOTP 二次认证流程中保持 OAuth 上下文
- 前端登录页透传 OAuth 参数，处理后端返回的 redirectTo 跳转
- 第三方 API 认证（API Key 方式），支持用户登录状态验证、权限检查和订阅状态查询；Ext API 另提供 Realm（创建/列表/查询）、User（创建/列表/查询）、Client App（创建/列表/查询）、Billing（订阅计划/分配查询）、Points（余额查询/消费/交易查询）管理能力（详见各自独立 PRD）
- API Key 绑定到特定 Client App（Client App Scope），普通 Client App 的 API Key 仅能访问该 App 所属资源，Admin API Client 的 Key 可跨 App 访问
- API Key 轮换（Rotate），生成新密钥并立即失效旧密钥
- API Key Realm 隔离，API Key 使用统计
- OAuth 2.0 Device Authorization Grant (RFC 8628)，详见独立 PRD `docs/prd/auth/device-code.md`
- Herald 作为 OAuth Client 的 SSO 登录，通过 `/api/oauth/{realmId}/{provider}/login` 和 `/{provider}/callback` 路径实现第三方 Provider 登录
- **OpenID Connect 兼容身份层（叠加在上述 Authorization Code + PKCE 核心端点上）**：`/authorize` 接受可选 `scope`/`nonce` 参数，`/token` 在流程携带字面 `openid` scope 时额外签发 RS256 `id_token`；并新增 discovery、JWKS 与 `userinfo` 端点。完整语义（claim 集、签名密钥轮换、自定义域名 issuer 重定位等）由独立 PRD [openid-connect.md](openid-connect.md) 承载，本 PRD 只声明基础授权码流的语义

### 2.2 不包含功能 (Out of Scope)

- Refresh Token（server-side token 当前不支持令牌刷新）；浏览器 token 变体支持旋转 refresh token（见 [自建用户 UI](/docs/prd/integration/custom-user-ui.md) D-TOK-01）
- Token 撤销：server-side token 当前不支持撤销；浏览器 token 变体支持即时吊销（见 [自建用户 UI](/docs/prd/integration/custom-user-ui.md) D-TOK-02）
- OAuth 2.0 Scope 管理（没有细粒度 scope 授权页面；唯一例外是 OIDC 身份层识别字面 `openid` token 以触发 id_token 签发，其余 scope 一律原样透传，见 [openid-connect.md](openid-connect.md)）
- 用户主动授权/拒绝授权页面（当前授权自动完成，用户无需手动批准）
- Implicit Flow（已被 OAuth 2.1 废弃）
- API Key 管理界面（后续优化）
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

## 4. 业务规则与状态

### 4.1 业务规则

**OAuth Provider 管理:**
- Provider 配置为 Realm 级别资源，仅 Realm Admin 可管理
- 每个 Realm 可配置多个 OAuth Provider（Google、GitHub、Facebook、Apple、Discord、WeChat、WeChat Mini Program）
- Provider 可独立启用/禁用；禁用的 Provider 不在登录页显示
- Provider 配置包含 Client ID、Client Secret、Scopes 和启用状态；各 Provider Type 有默认 Scopes 配置
- 编辑 Provider 时 Client Secret 为可选（留空表示保持原值）；前端不应显示已存储的 Client Secret
- 删除 Provider 需要二次确认
- WeChat Provider Scope 仅允许 `snsapi_login`；WeChat Mini Program 不使用 Scope

**OAuth 授权流程:**
- 第三方 SPA 必须使用 Authorization Code + PKCE 流程，不支持 Implicit Flow
- Client App 必须存在且已启用，redirect_uri 必须在白名单中精确匹配（origin + port 完全一致；第一方 Client App——内置管理控制台/用户账户中心——例外，其回调固定为 Herald 自有前端路由）；redirect_uri 仅允许 http/https 协议，拒绝协议相对 URL 与 `javascript:` 等危险协议，生产环境强制 HTTPS（非生产环境允许 http，如 localhost 回调）
- 授权码在签发时绑定 client_id、redirect_uri 与 code_challenge，令牌交换时逐一校验
- Google One Tap 与 Apple 原生（Sign in with Apple）直连登录由专属 PRD 承载（[google-one-tap.md](google-one-tap.md)、[support-mobile-apple-login.md](support-mobile-apple-login.md)），不经本 PRD 的 authorize/code 交换流
- State 和 authorization_code 必须一次性使用，验证后立即删除
- PKCE 的 code_challenge 必须使用 S256 方法（SHA256）
- 无 OAuth 参数时，登录行为与现有普通登录完全一致
- `/authorize` 接受可选 `scope` 与 `nonce` 参数（存入授权事务状态，随授权码记录传递）；唯一的 scope 语义是识别**字面且区分大小写**的 `openid` token——命中时 `/token` 在 access_token 之外额外签发 RS256 `id_token`（回显 `nonce`），未命中时响应完全不含 `id_token` 字段；其余 scope token 不做任何解释，原样透传。OIDC 专属语义（claim 集、 userinfo、签名密钥）见 [openid-connect.md](openid-connect.md)
- OAuth 参数不完整时（缺少任意一项），应显示错误提示，不静默降级为普通登录
- 未认证 OAuth 端点实施 per-IP 速率限制，超限返回 429：`/authorize` 与 `/token` 一档；发起 Provider 登录（含上游 JWKS/code2session 拉取）与 Device Authorization Grant 的 authorize 端点更低一档（阈值均为后端统一常量管理的运行默认值，非对外契约，第三方集成方须处理 429）

**第三方 API 接入:**
- 第三方应用使用 API Key（通过 X-API-Key header）认证，与 session token 认证体系分离
- API Key 绑定到特定 realm，只能访问所属 realm 的资源
- API Key 可绑定到特定 Client App（Client App Scope），绑定后只能访问该 Client App 所属资源
- Admin API Client（`admin-api-client`）的 API Key 不受 Client App Scope 限制，可跨 App 访问
- 未绑定 Client App 的 API Key 也不受 Client App Scope 限制
- API Key 支持轮换（Rotate），生成新密钥，旧密钥立即失效，返回新明文密钥（仅展示一次）；轮换主动驱逐旧密钥的认证缓存条目（更新前后各一次以防竞态回填；驱逐失败时旧密钥最长残留缓存 TTL，认证侧因 Redis 不可用本就 fail closed，缓存 TTL 仅作兜底——见 api-key-roles PRD 同条说明）
- API Key 有启用/禁用和过期时间控制
- 记录 API Key 最后使用时间（节流更新：每分钟最多一次写库）
- 无效或缺失 API Key 返回 401；过期或禁用 API Key 返回 401
- 无效 session token 在权限检查时返回 `allowed: false`，而非报错

**Herald OAuth Client SSO 登录:**
- Herald 作为 OAuth Client 通过 `/api/oauth/{realmId}/{provider}/login` 发起第三方 Provider 授权
- 回调路径 `/{provider}/callback` 接收 Provider 授权结果，创建或关联 OAuth 用户账户，完成 SSO 登录
- 通用跳转式链路实际服务 Google、GitHub、Facebook、Apple、Discord；`wechat` 在通用端点白名单中为死条目（同形专属路由 `/wechat/login` 优先匹配，微信网站登录由专属路由承载）；`wechat_miniprogram` 不经通用登录端点：Provider 未配置或已禁用凭据时返回 404，已配置时因不生成授权 URL 返回 400，登录走专用 code2session 端点
- OAuth 账户通过 open_id 关联用户；未命中 provider 身份时才按 Email 匹配。回调是由一次性 state 约束的未认证入口，不以浏览器中是否另有 Herald 会话作为关联依据；Email 命中既有账号时 Provider 返回的邮箱必须已验证，未验证邮箱不得用于关联既有账号（防止经 Provider 未验证邮箱接管既有密码账号，如 GitHub 非主邮箱）。唯一例外是由已验签 provider subject 确定性生成且完全匹配的内部占位邮箱，用于恢复“账号已创建但 provider link 未落账”的失败重试
- Facebook 是上述邮箱验证要求的一个已评审例外：Facebook 不向第三方应用暴露邮箱级验证标志（Graph 的 `verified` 字段是已废弃的账号级徽章，非邮箱验证信号），`email` 权限下 Graph 返回的邮箱即账号的 Facebook 已确认主邮箱，故 Facebook 邮箱一律按已验证处理；该例外的边界是 Graph 必须实际返回邮箱——未返回邮箱时本次登录失败，不进入 Email 关联路径
- **自动建号受 Realm 注册政策门控（注册政策优先）**：当 Provider 凭证未命中已有用户、需要新建账号时，必须先检查当前 Realm 的注册开关（`registration.enabled` / `is_registration_enabled`）。Realm 未开启自动注册时，OAuth 路径**不得**绕过注册政策自动建号，返回注册未开放提示（实现上以 `409 conflict` 表达），引导用户走显式注册入口。已命中已有用户的关联登录不受此门控影响。注册政策还包括可选的注册邮箱域白名单（`registration.allowed_domains`，见 `docs/prd/core/realm-settings.md`）：配置后，Provider 邮箱域名不在白名单内时建号同样返回 `409 conflict`；白名单为空表示不限。该原则与邮箱验证码登录一致（见 `docs/prd/auth/email-otp-login.md` §4.1「注册政策优先」），对所有 OAuth Provider（Google、GitHub、Facebook、Apple、Discord、WeChat 等）统一适用

**Herald 作为身份 Broker（brokered downstream-state redirect）:**
- 当第三方 Client App 已在 Herald `/authorize` 发起自身的 Authorization Code + PKCE 授权事务时，可在跳转 `/api/oauth/{realmId}/{provider}/login` 时携带 `downstream_state` 参数，将该事务标识传递给 Herald
- `downstream_state` 必须指向一个已存在、未消费、与当前 realm/client_id/redirect_uri/code_challenge 完整绑定的下游授权事务；校验失败拒绝发起 Provider 授权
- Provider 回调 `/{provider}/callback` 时，若上下文携带有效的 `downstream_state`，Herald 不为该用户创建 Herald 自身会话，而是消费该下游 state（一次性，GETDEL 语义）并签发一个一次性 `authorization_code`，重定向回下游 Client App 的 `redirect_uri`（携带 `code` 与 `state`）
- 下游授权分支同样执行登录同意闸门（与第一方直登分支同规则，见 `docs/prd/core/legal-consent-account-deletion.md` §4.1「登录即同意」）：签发前先评估同意状态，同意缺失或版本过期时不签发授权码、**不消费** `downstream_state`（保持未消费，用户经受限会话记录同意后重新走 Provider 登录即可完成下游授权），响应改为 `consentRequired: true` + 当前生效协议摘要
- 下游 Client App 随后通过既有 `/token` 端点 + PKCE 校验换取令牌，与普通 Authorization Code + PKCE 流程一致
- 该流程使 Herald 在充当 OAuth Client（对接 Google 等 IdP）的同时充当下游 Client App 的身份 Broker，把 IdP 认证结果转换为下游可用的授权码

**第一方直登分支（无 downstream_state）:**
- Provider 回调 / Google One Tap / Apple 原生登录 / WeChat 直登在无下游上下文时为第一方直登：校验通过后为用户签发第一方浏览器 token family（完整会话）
- 直登分支同样执行登录同意闸门（见 `docs/prd/core/legal-consent-account-deletion.md` §4.1「登录即同意」，直登不豁免）：同意缺失或版本过期时不签发完整会话，响应改为 `consentRequired: true` + 当前生效协议摘要 + 受限会话（仅资料读取/注销账户/退出登录 scope，无 token 字段）；因 provider 凭据一次性、不可携带同意重放登录，补全路径为受限会话显式记录同意（`POST /api/user/consent`）后重新触发登录入口
- **闸门降级口径**：上述各分支（下游授权、第一方直登、设备授权码流）的同意状态/生效协议查询发生存储故障时，闸门按 legal-consent PRD §4.1 的既定取舍 fail-open 放行并记录告警（可用性优先），不视为同意已记录

**TOTP + OAuth 兼容:**
- TOTP 临时会话中保存 OAuth 上下文（oauth_client_id、redirect_uri、state）
- TOTP 验证成功后检查临时会话中的 OAuth 字段，有 OAuth 字段时走同样的 authorization_code 生成逻辑

**异常处理:**

> 下列"提示"为**用户可见语义**（前端落地页呈现的友好文案口径），不是后端错误消息的逐字契约；后端实际返回英文错误消息/错误码，端点级错误模型以技术设计为准。

- 用户拒绝授权（OAuth provider 按 RFC 6749 §4.1.2.1 回调携带 error/error_description、无 code）：回调端点接受该错误形态——下游授权分支消费 pending 的 `downstream_state` 后以 302 重定向回下游 `redirect_uri`（携带 `error` 与 `state`，由下游自行呈现）；第一方直登分支（无下游上下文）返回 200 JSON 拒绝体（错误码 + 友好信息，与成功响应同形以便落地页统一渲染），不签发任何会话或 token
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
- **state token**: 一次性，校验后立即失效（Redis 删除），TTL 5 分钟。`/authorize` 播种 state 使用 SET NX 语义：state 值已存在未消费的 pending 事务时拒绝该次 authorize（400），防止已知受害者 state 值的攻击者在登录完成前用自身 client_id/redirect_uri/PKCE 重播种（state fixation）；客户端应为每次流程生成新的随机 state
- **OAuth 上下文参数**: oauthClientId、redirectUri、state 三项必须完整才触发 OAuth 流程

---

## 5. 验收目标

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

## 6. 边界与约束

**适用性**: 适用（API 与前端/交互边界合并陈述）

**API / 集成边界:**
- OAuth Provider 管理接口为 Realm 级别资源，仅 Realm Admin 可访问
- 第三方 OAuth 授权涉及 authorize（Client App 校验和登录页重定向）与 token（授权码校验和令牌签发）两个核心能力
- 权限检查接口支持 batch 模式（多个 rules）；订阅查询接口在无订阅时返回 free tier 信息
- Client App 禁用时拒绝所有 OAuth 授权请求
- API Key 轮换需要 `api_keys.manage` 权限；轮换端点路径无 realm 段，realm 由 admin 会话钉定
- Herald OAuth Client SSO 路径用于 Herald 自身通过第三方 Provider 登录（发起授权 + 回调处理）
- OpenID Connect 兼容端点叠加在本 PRD 的核心端点上（discovery、JWKS、`userinfo`；流程携带 `openid` scope 时 `/token` 返回 `id_token`，并接受 RFC 6749 form-urlencoded 请求体），端点契约与错误语义见 `docs/prd/auth/openid-connect.md`
- OAuth 2.0 Device Authorization Grant 完整实现（RFC 8628），端点包含 authorize、token、verify、confirm，详见 `docs/prd/auth/device-code.md`
- 详细端点契约、认证方式和错误模型下沉到技术设计或接口说明文档

**前端 / 交互边界:**
- Provider 配置入口在 Settings 页面的 Providers Tab，与 Turnstile、Registration 并列；列表以表格形式展示名称、Client ID、状态（Enabled/Disabled 用不同颜色 Badge 区分）、Scopes 和操作按钮（编辑、启用/禁用切换、删除）
- 新增/编辑 Provider 通过对话框表单完成，字段包含 Provider Type（下拉选择）、Client ID、Client Secret（编辑时可选，提示"留空保持不变"）、Scopes（多选）、Enabled 开关
- 删除 Provider 需要二次确认交互
- 登录页动态加载已启用的 Provider 列表，展示为独立的登录按钮
- 登录页 search schema 须支持 OAuth 上下文参数（oauthClientId、redirectUri、state）；OAuth 参数完整（三项都存在）时提交登录须一并传给后端，不完整时显示错误提示，不静默降级为普通登录
- 后端返回 redirectTo 时直接跳转第三方 callback（不经前端安全重定向检查，安全性由后端白名单保证）；TOTP 完成后同样支持 redirectTo 跳转
- 涉及第三方接入时，明确区分 Herald 后台完成的流程和第三方应用/外部平台完成的流程

---

## 7. 已确认决策

- 采用 Authorization Code + PKCE 替代旧 Implicit Flow，符合 OAuth 2.1 标准
- OAuth Provider 配置独立于 Realm Config（key-value），使用独立的 Provider 实体管理
- 命名使用 "Provider" / "Identity Provider"，避免与 OAuth Config 技术术语混淆
- redirect_uri 白名单采用精确匹配策略（origin + port），不使用前缀匹配；仅允许 http/https 协议，拒绝协议相对 URL 与 `javascript:` 等危险协议，生产环境强制 HTTPS（非生产环境允许 http，供 localhost 开发/演示回调）
- 第三方 API 认证使用独立 API Key 体系，与 session token 分离
- State 和 authorization_code 存储在 Redis，一次性使用后删除

---

## 8. 参考资料

- 相关 PRD：`docs/prd/core/realm-settings.md`、`docs/prd/integration/client-app.md`、`docs/prd/auth/permissions.md`、`docs/prd/auth/totp.md`、`docs/prd/auth/device-code.md`（Device Authorization Grant）、`docs/prd/auth/openid-connect.md`（叠加在本 PRD 核心端点上的 OIDC 身份层：discovery/JWKS/userinfo、id_token、签名密钥轮换）
- 用户故事来源见 §1 表格
