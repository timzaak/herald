# Google One Tap 登录 产品需求文档 (PRD)

**创建时间**: 2026-07-19
**优先级**: P1
**所属域**: auth

---

## 1. 相关用户故事

> 详细故事与验收标准请查看 `docs/user-stories/` 中对应文档。

### 1.1 相关故事

| US-ID | 标题 | 角色 | 优先级 | 来源 |
|-------|------|------|--------|------|
| US-OT-001 | 通过 One Tap 在第三方应用一键登录 | Regular User | P0 | 候选来源 `.ai/user-stories/auth/google-one-tap.md`（待发布到 `docs/user-stories/`） |
| US-OT-002 | 第三方应用集成 One Tap | 第三方应用开发者 | P0 | 候选来源 `.ai/user-stories/auth/google-one-tap.md`（待发布到 `docs/user-stories/`） |
| US-OT-003 | One Tap 用户与已有账号关联 | Regular User | P1 | 候选来源 `.ai/user-stories/auth/google-one-tap.md`（待发布到 `docs/user-stories/`） |
| US-RU-003 | OAuth 第三方登录 | Regular User | P1 | `docs/user-stories/core/regular-user.md` |
| US-TP-001 | OAuth 授权码登录 | 第三方应用开发者 | P0 | `docs/user-stories/auth/third-party-app.md` |
| US-TP-015 | 第三方 Web SPA 发起 SSO 登录 | 第三方应用开发者 | P1 | `docs/user-stories/auth/third-party-app.md` |

### 1.2 优先级汇总

| 优先级 | 数量 | 关键故事 |
|--------|------|----------|
| P0 | 2 | US-OT-001 One Tap 一键登录、US-OT-002 第三方集成 |
| P1 | 1 | US-OT-003 账号关联 |

---

## 2. 范围界定

### 2.1 包含功能

- **第三方应用页面嵌入 One Tap**：第三方应用在自身网站页面上嵌入 Google One Tap SDK，弹出 Google 账号浮层
- **Herald 后端验证 Google 凭证**：Herald 接收第三方前端传来的 Google ID Token（JWT），验证签名、签发者（issuer）、受众（audience）和有效期
- **用户身份匹配与创建**：验证通过后，使用与现有跳转式 Google 登录相同的匹配策略（open_id → email → 创建）识别或创建 Herald 用户
- **两种会话建立模式**：
  - 直接会话模式：One Tap 用于 Herald 自身前端登录时，Herald 直接建立 session
  - 授权码模式：One Tap 嵌入下游 OAuth Code+PKCE 流程时，Herald 签发授权码返回给第三方应用
- **公开配置暴露 client_id**：Herald 公共配置接口向第三方前端暴露已启用的 Google Provider 的 client_id，使前端可初始化 One Tap SDK

### 2.2 不包含功能 (Out of Scope)

- **Herald 自身登录页的 One Tap 集成**：本轮不在 Herald 自身登录页弹出 One Tap（后续可扩展）
- **非 Google provider 的 One Tap**：不覆盖 Microsoft、Apple 等 provider 的类似无感登录
- **Google Identity Services 的 FedCM 迁移**：使用经典 GIS SDK，不做 FedCM 适配
- **管理员配置 One Tap 开关**：本轮不新增 per-realm 的 One Tap 启停配置，只要 Realm 启用了 Google Provider 即可使用 One Tap
- **移动端原生 App 的 One Tap**：仅覆盖 Web 端

### 2.3 依赖项

- **Google Provider 配置**：Realm 必须已配置并启用 Google OAuth Provider（复用现有 Google Provider 配置能力）
- **Google Cloud Console 配置**：第三方应用域名需加入 Google OAuth Client 的授权域名列表（Authorized JavaScript origins）
- **现有用户匹配基础设施**：复用现有用户匹配能力（open_id → email → 创建）
- **现有下游授权码签发机制**：复用现有下游授权交易标识 + 授权码签发流程（见 [OAuth](oauth.md) §4.1 brokered downstream-state redirect，US-TP-001/015/016）

---

## 3. 需求概述

### 3.1 功能描述

当前第三方应用接入 Herald SSO 时，用户需被重定向到 Herald 登录页，再通过跳转到 Google 完成认证。整个流程涉及多次页面跳转，用户摩擦大。

Google One Tap 允许第三方应用在自己的页面上直接弹出 Google 账号浮层。用户点击后，Google 在浏览器内签发一个 ID Token（JWT），第三方前端将其发送给 Herald 后端。Herald 验证该 Token 的签名和声明（issuer、audience、expiry），确认用户身份后签发会话或授权码。

这使第三方应用能在不离开当前页面的情况下完成用户登录，大幅降低登录摩擦。

### 3.2 关键特性

- **无跳转登录**：用户在第三方应用页面一键完成 Google 认证，无需重定向
- **Herald 作为统一验证方**：所有 Google 凭证由 Herald 后端集中验证，第三方应用不接触 Google client_secret
- **账号匹配一致性**：One Tap 登录与跳转式 Google 登录使用相同的用户匹配策略，确保同一 Google 用户始终关联同一 Herald 账号
- **双模式会话建立**：支持直接 session 模式和下游授权码模式，兼容不同的接入场景

---

## 4. 业务规则与状态

### 4.1 业务规则

- **触发位置**：One Tap 浮层在**第三方应用网站**上弹出，不在 Herald 登录页弹出
- **Provider 启用条件**：Realm 必须配置并启用 Google Provider，否则 Herald 拒绝 One Tap 凭证验证请求
- **凭证验证严格性**：Herald 必须在服务端验证 Google ID Token 的签名（使用 Google JWKS 公钥）、issuer、audience（等于该 Realm 配置的 client_id）和 expiry，不得信任前端传来的任何明文用户信息
- **邮箱验证要求**：Google 返回的凭证中邮箱未验证时，Herald 拒绝创建用户或登录
- **用户匹配策略**：与现有跳转式 Google 登录完全一致——通过 Google 用户 ID（open_id）匹配 → 邮箱匹配 → 创建新用户
- **自动建号受 Realm 注册政策门控（注册政策优先）**：当 Google 凭证未命中已有用户、需要新建账号时，必须先检查当前 Realm 的注册开关（`registration.enabled` / `is_registration_enabled`）。Realm 未开启自动注册时，One Tap 路径**不得**绕过注册政策自动建号，返回注册未开放提示（实现上以 `409 conflict` 表达），引导用户走显式注册入口。已命中已有用户的关联登录不受此门控影响。该原则与邮箱验证码登录、其他 OAuth Provider 一致（见 `docs/prd/auth/email-otp-login.md` §4.1「注册政策优先」、`docs/prd/auth/oauth.md` §4.1）。
- **一次性凭证**：Google ID Token 有有效期（约 1 小时），过期后验证失败
- **登录同意闸门**：直登模式（无 `downstream_state`）命中登录同意闸门（同意缺失或版本过期，见 `docs/prd/core/legal-consent-account-deletion.md` §4.1「登录即同意」，直登不豁免）时，不签发完整会话；响应改为 `consentRequired: true` + 当前生效协议摘要 + 受限会话（无 token 字段）。Google 凭据一次性、不可携带同意重放，补全路径为受限会话提交 `POST /api/legal/{realmId}/consent` 记录同意后再次发起 One Tap 登录
- **共存原则**：One Tap 与跳转式 Google 登录按钮共存，互不影响；用户可通过任一方式登录
- **下游授权码模式绑定**：当请求携带 `downstream_state` 时，必须指向一个已存在、未消费、与当前 realm/client_id/redirect_uri/code_challenge 完整绑定的下游授权事务（与 OAuth brokered redirect 共用校验）

### 4.2 关键状态与异常

- **凭证验证失败**：签名无效、issuer 不符、audience 不匹配、已过期 → 返回认证失败，不创建任何会话或用户
- **Provider 未配置/已禁用**：返回 Provider 未配置错误
- **Google JWKS 不可达**：Herald 无法获取 Google 公钥时返回服务暂时不可用（属基础设施异常，不应静默跳过验证）
- **One Tap 浮层频率**：用户关闭浮层后由 Google SDK 控制不再弹出（Cool-down），Herald 不介入此行为
- **未登录 Google 账号**：浏览器未登录 Google 时浮层不弹出，属正常降级，用户使用其他登录方式
- **邮箱未验证**：Google 凭证中 `email_verified` 为 false 时拒绝创建用户或登录
- **Realm 未开启自动注册**：未注册用户首次通过 One Tap 登录时，若 Realm 注册开关关闭，不创建账号并返回 `409 conflict`（注册未开放），引导用户走显式注册入口；已命中已有用户的登录不受影响

---

## 5. 功能需求

### 5.1 核心需求

1. **Herald 后端新增 Google ID Token 验证能力**：使用 Google JWKS 公钥验证 ID Token 签名，校验 issuer（`accounts.google.com` / `https://accounts.google.com`）、audience（等于 Realm 的 Google client_id）、expiry，提取用户信息（sub、email、email_verified、name、picture）

2. **Herald 后端新增 One Tap 认证端点**：接收第三方前端传来的 Google 凭证 + 可选的下游授权交易标识（`downstream_state`），验证通过后执行用户匹配/创建，并根据是否存在该标识选择直接建立 session 或签发授权码

3. **公开配置暴露 client_id**：Herald 公共配置接口（public-config）向已启用 Google Provider 的 Realm 暴露 Google client_id，使第三方前端可初始化 GIS SDK

4. **下游授权码兼容**：当 One Tap 在 Authorization Code + PKCE 场景中使用时（请求携带 `downstream_state`），验证通过后签发一次性授权码，第三方应用通过现有的 token 端点 + PKCE 验证换取 access_token

5. **用户匹配与现有流程一致**：One Tap 创建/匹配用户时使用现有的三级策略（open_id → email → 创建），确保跳转式和 One Tap 两种方式关联到同一用户

### 5.2 验收目标

- 第三方应用网站嵌入 One Tap 后，已登录 Google 的用户能看到浮层并一键完成登录，全程无页面跳转
- Herald 后端正确验证 Google 凭证的签名、issuer、audience、expiry，拒绝篡改/伪造/过期的凭证
- 未注册用户通过 One Tap 自动创建 Herald 账号并登录成功（仅在 Realm 已开启自动注册时；未开启时返回 `409 conflict` 并引导至显式注册入口）
- 已有 Herald 账号（邮箱一致）的用户通过 One Tap 登录时关联到已有账号，不产生重复账号
- One Tap 在下游 Code+PKCE 场景中，验证通过后正确签发授权码，第三方应用可正常换取 token
- 未配置 Google Provider 的 Realm 返回明确错误

---

## 6. API 相关约束

**适用性**: 适用

- **能力边界**：新增一个认证端点 `POST /api/oauth/{realmId}/google/one-tap`，接收 Google ID Token 凭证，返回会话信息或下游授权码重定向
- **访问控制**：该端点为公开端点（无需已认证身份），但必须验证 Google 凭证的有效性作为访问前提
- **Realm 数据边界**：端点路径包含 realmId，Google 凭证的 audience 必须与该 Realm 配置的 client_id 一致；用户匹配和创建限定在当前 Realm 内
- **兼容性**：与现有 OAuth Code+PKCE 流程（US-TP-001/015/016）兼容，下游授权交易标识机制复用现有实现；与跳转式 Google 登录（US-RU-003）共存
- **安全约束**：
  - client_secret 不出现在任何公开响应中
  - Google 凭证不得被记录在 tracing span 中（遵循现有 OAuth handler 的 instrumentation 治理规范）

> 端点清单、参数 schema、状态码矩阵与错误模型不在 PRD 承载范围，下沉到技术设计。

---

## 7. 前端/交互约束

**适用性**: 适用（第三方应用前端，非 Herald 自身前端）

- **页面入口**：One Tap 由第三方应用在自身页面中初始化，非 Herald 控制渲染位置
- **初始化依赖**：第三方前端需从 Herald 公共配置接口获取 Google client_id 来初始化 GIS SDK
- **关键交互**：
  - 用户看到 One Tap 浮层（由 Google SDK 渲染）
  - 用户点击浮层中的账号 → Google 在浏览器内签发 ID Token → 第三方前端将 Token 发送给 Herald 后端
  - 验证成功 → 第三方前端根据返回结果（session 或授权码重定向）完成登录
- **状态反馈**：
  - 验证失败 → 第三方前端显示错误提示（非敏感信息）
  - 用户关闭浮层 → 浮层消失，不影响其他登录方式
- **降级**：浏览器未登录 Google / Google SDK 加载失败 / 域名未授权 → One Tap 不可见，页面保留常规登录入口
- **权限可见性**：One Tap 浮层的显示条件由 Google SDK 控制（用户需已登录 Google 账号且域名已授权），Herald 不干预

---

## 8. 已确认决策

- **触发位置**：One Tap 在第三方应用网站上弹出，Herald 仅作为后端验证方（非 Herald 登录页）
- **Provider 配置复用**：不新增 One Tap 专用配置开关，Realm 启用 Google Provider 即可使用 One Tap
- **用户匹配策略**：复用现有跳转式 Google 登录的三级匹配策略（open_id → email → 创建）
- **双模式会话**：支持直接 session 模式和下游授权码模式，与现有 SSO 架构一致
- **公开 client_id**：client_id 不是密钥（现有跳转流程已通过 auth URL 明文传递），可在公共配置中暴露

---

## 9. 参考资料

- 用户故事（候选）：`.ai/user-stories/auth/google-one-tap.md`（US-OT-001/002/003，待发布到 `docs/user-stories/`）
- OAuth 第三方登录：[OAuth](oauth.md)（US-RU-003）、`docs/user-stories/auth/third-party-app.md`（US-TP-001/015/016）
- 下游授权码 brokered 流程：[OAuth](oauth.md) §4.1「Herald 作为身份 Broker」
