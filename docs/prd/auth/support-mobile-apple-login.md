# 苹果手机 App native 登录 产品需求文档 (PRD)

**创建时间**: 2026-08-04
**优先级**: P1
**所属域**: auth

---

## 1. 相关用户故事

> 详细故事与验收标准请查看 `docs/user-stories/auth/support-mobile-apple-login.md`。

### 1.1 相关故事

| US-ID | 标题 | 角色 | 优先级 | 来源 |
|-------|------|------|--------|------|
| US-AL-001 | 在 iOS App 内使用 Apple 账号一键登录 | Regular User | P0 | `docs/user-stories/auth/support-mobile-apple-login.md` |
| US-AL-002 | 接入方在 iOS App 中集成 Apple native 登录 | 第三方应用开发者 | P0 | `docs/user-stories/auth/support-mobile-apple-login.md` |
| US-AL-003 | Apple native 登录与已有账号关联 | Regular User | P1 | `docs/user-stories/auth/support-mobile-apple-login.md` |
| US-RU-003 | OAuth 第三方登录 | Regular User | P0 | `docs/user-stories/core/regular-user.md` |
| US-TP-001 | OAuth 授权码登录 | 第三方应用开发者 | P0 | `docs/user-stories/auth/third-party-app.md` |
| US-TP-015 | 第三方 Web SPA 发起 SSO 登录 | 第三方应用开发者 | P0 | `docs/user-stories/auth/third-party-app.md` |
| US-TP-016 | 第三方后端用授权码换取令牌 | 第三方应用开发者 | P0 | `docs/user-stories/auth/third-party-app.md` |
| US-OE-001 | OAuth Provider 配置管理 | Realm Admin | P0 | `docs/user-stories/auth/oauth-extension.md` |

### 1.2 优先级汇总

| 优先级 | 数量 | 关键故事 |
|--------|------|----------|
| P0 | 2 | US-AL-001 iOS 内 Apple 登录、US-AL-002 接入方集成 |
| P1 | 1 | US-AL-003 账号关联 |

> 注：基线故事（US-RU-003、US-TP-001/015/016、US-OE-001）仅作依赖引用，不计入本 feature 的优先级分布。

---

## 2. 范围界定

### 2.1 包含功能

- **iOS App 内触发 Apple 原生授权**：接入方（第一方或第三方）的 iOS App 在 App 内调用苹果系统原生授权（Sign in with Apple native SDK），用户在系统弹窗中确认后，App 拿到 Apple 签发的身份凭证（identityToken），提交给 Herald 后端校验
- **Herald 后端校验 Apple 身份凭证**：Herald 接收 iOS App 传来的 Apple identityToken（JWT），在服务端校验签名、签发者、受众和有效期，提取 Apple 用户唯一标识（sub）与邮箱信息
- **用户身份匹配与创建**：校验通过后，使用与现有 Apple web 跳转登录一致的匹配策略（union_id 不适用 Apple → Apple sub 优先 → 邮箱 → 创建）识别或创建 Herald 用户；保证同一 Apple 用户在 web 与 native 两条路径关联到同一账号
- **两种会话建立模式**（完全对齐 Google One Tap）：
  - 直接会话模式：iOS App 对应 Herald 中的第一方 Client App 时，Herald 直接签发该 App 的会话
  - 下游授权码模式：iOS App 通过 Authorization Code + PKCE 接入（第三方 Client App）时，Herald 签发一次性授权码，接入方再凭 PKCE 换取令牌
- **Apple Provider 配置复用**：复用现有 Apple Provider 配置（Client ID、启用状态），Realm 启用 Apple Provider 即可使用 native 登录，不新增 native 专用配置项

### 2.2 不包含功能 (Out of Scope)

- **Herald 自有 iOS App**：Herald 无自建 iOS App 计划，本能力为纯后端能力；iOS 端由接入方自行实现，不在本仓库范围
- **Herald Web SPA 改动**：Herald Web 前端（管理控制台 / 登录页）无任何改动，Apple Provider 的现有配置表单字段（Client ID、scopes、启用状态）对 native 路径已足够
- **Apple web 跳转登录的 client_secret 自动签发缺陷修复**：现有 Apple web redirect 路径的 JWT client_secret 运行时签发与续签问题独立，不在本能力范围；native 路径不调 Apple token 端点、不使用 client_secret，天然绕开此问题
- **通过 authorizationCode 换 Apple 上游 refresh_token**：本能力不代理调用 Apple 上游接口，不存 Apple 访问令牌或刷新令牌；只校验 identityToken 完成身份落账
- **Apple web redirect 登录本身**：web 跳转式 Apple 登录属既有能力，不在本能力范围；native 与之并存且关联同一 Apple 用户
- **macOS / iPadOS / Android 的 Apple 登录**：仅覆盖 iOS App native 场景

### 2.3 依赖项

- **Apple Provider 配置**：Realm 必须已配置并启用 Apple OAuth Provider（复用现有 Apple Provider 配置能力，US-OE-001）
- **Apple Developer 配置**：接入方的 iOS App 需在 Apple Developer 后台启用 Sign in with Apple capability，并配置与 Herald Realm Apple Client ID（Service ID）一致的受众
- **现有用户匹配基础设施**：复用现有用户匹配能力（open_id → email → 创建）
- **现有下游授权码签发机制**：下游授权码模式复用现有下游授权交易标识 + 授权码签发流程（见 [OAuth](oauth.md) §4.1 brokered downstream-state，US-TP-001/015/016）
- **现有直接会话签发机制**：直接会话模式复用现有 BrowserTokenSet 会话签发能力

---

## 3. 需求概述

### 3.1 功能描述

接入方（第一方或第三方）的 iOS App 希望在 App 内提供 Apple 登录，用户通过苹果系统原生弹窗完成 Apple 授权后，无需跳转浏览器即可建立 Herald 会话或换取下游授权码。

当前 Apple 登录仅有 web 跳转路径（OAuth Authorization Code redirect），不适合 iOS 原生场景：用户需离开 App、走浏览器重定向、再返回 App，体验割裂，且 Apple web redirect 路径依赖 client_secret（有运行时签发与每 6 个月续签的运维负担）。

本能力提供 Apple native 登录端点：iOS App 用苹果原生 SDK（`ASAuthorizationAppleIDProvider`）拿到 Apple 签发的 identityToken（JWT），提交给 Herald，由 Herald 在服务端集中校验凭证（JWKS 签名 + 签发者 + 受众 + 有效期），确认用户身份后签发直接会话或下游授权码。Herald 不接触任何 Apple 密钥，不调 Apple token 端点。

架构上与 Google One Tap（[google-one-tap.md](google-one-tap.md)）完全同构：接收上游签发的 JWT 凭证 → 服务端校验 → 用户匹配 → 双分支签发。

### 3.2 关键特性

- **App 内无跳转登录**：用户在 iOS App 内通过系统弹窗完成 Apple 授权，全程不离开 App
- **Herald 作为统一凭证校验方**：所有 Apple identityToken 由 Herald 后端集中校验，iOS App 不接触任何 Apple 密钥，不调 Apple token 端点、不使用 client_secret
- **账号匹配一致性**：native 登录与 Apple web 跳转登录使用相同的用户匹配策略，确保同一 Apple 用户在两条路径关联到同一 Herald 账号
- **双模式会话建立**：支持直接会话模式（第一方 Client App）和下游授权码模式（第三方 Client App，Code+PKCE），与现有 SSO 架构一致
- **邮箱缺失不阻断建号**：针对 Apple native 凭证在非首次授权时恒不返回邮箱的特性，采用占位邮箱策略，确保存量用户首次 native 登录不因邮箱缺失而失败

---

## 4. 业务规则与状态

### 4.1 业务规则

- **触发位置**：Apple 原生授权在 **接入方的 iOS App 内** 由苹果系统弹窗触发，不在 Herald Web 登录页触发；Herald 仅作为后端凭证校验与会话签发方
- **Provider 启用条件**：Realm 必须配置并启用 Apple Provider，否则 Herald 拒绝校验 native 凭证
- **凭证校验严格性**：Herald 必须在服务端校验 Apple identityToken 的签名（使用 Apple JWKS 公钥）、签发者（`https://appleid.apple.com`）、受众（等于该 Realm 配置的 Apple Client ID）和有效期，不得信任 App 传来的任何明文用户信息
- **用户匹配策略**：与现有 Apple web 跳转登录完全一致——通过 Apple 用户唯一标识（sub）匹配 → 邮箱匹配 → 创建新用户；Apple 不提供 union_id（与微信不同）
- **自动建号受 Realm 注册政策门控（注册政策优先）**：当 Apple identityToken 未命中已有用户、需要新建账号时，必须先检查当前 Realm 的注册开关（`registration.enabled` / `is_registration_enabled`）。Realm 未开启自动注册时，native 路径**不得**绕过注册政策自动建号，返回注册未开放提示（实现上以 `409 conflict` 表达），引导用户走显式注册入口。已命中已有用户的关联登录不受此门控影响。该原则与邮箱验证码登录、其他 OAuth Provider 一致（见 `docs/prd/auth/email-otp-login.md` §4.1「注册政策优先」、`docs/prd/auth/oauth.md` §4.1）。
- **邮箱处理规则**（与 Apple web 跳转登录有意不同，详见 §8 DEC-005）：
  - Apple 中转邮箱（`@privaterelay.appleid.apple.com`）是合法可收信地址，作真实邮箱处理，不生成占位邮箱
  - 凭证未返回邮箱 + Apple 用户唯一标识未命中已有 provider 记录（首次建号）→ 生成 `{sub}@apple.placeholder` 占位邮箱并标记未验证后建号（对齐微信占位邮箱策略）
  - 凭证未返回邮箱 + Apple 用户唯一标识命中已有 provider 记录（存量用户后续登录）→ 不依赖邮箱，直接靠唯一标识匹配
- **不调 Apple token 端点**：native 路径只校验 identityToken，不使用 authorizationCode 换 Apple 上游令牌，不使用 client_secret
- **不存上游令牌**：遵循项目「社交登录只做身份落账」设计原则，不存 Apple access_token / refresh_token / id_token
- **一次性凭证**：Apple identityToken 是有有效期的 JWT，过期后校验失败
- **共存原则**：native 登录与 Apple web 跳转登录按钮共存，互不影响；与现有其他 Provider（Google、GitHub、Facebook、WeChat 等）共存
- **下游授权码模式绑定**：当请求携带下游授权交易标识时，必须指向一个已存在、未消费、与当前 realm / client_id / redirect_uri / code_challenge 完整绑定的下游授权事务（与 OAuth brokered redirect 共用校验）

### 4.2 关键状态与异常

- **凭证校验失败**：签名无效、签发者不符、受众不匹配、已过期 → 返回认证失败，不创建任何会话或用户
- **Provider 未配置/已禁用**：返回 Apple Provider 未配置错误
- **Apple JWKS 不可达**：Herald 无法获取 Apple 公钥时返回服务暂时不可用（属基础设施异常，不得静默跳过校验）
- **用户取消 Apple 授权**：用户在 iOS 系统弹窗中取消或不允许授权 → App 不向 Herald 提交凭证，Herald 不介入
- **未在 Apple Developer 启用 capability**：iOS App 未配置 Sign in with Apple capability → 系统弹窗无法拉起，属接入方配置问题，不在 Herald 范围
- **Realm 未开启自动注册**：未注册用户首次通过 native 登录时，若 Realm 注册开关关闭，不创建账号并返回 `409 conflict`（注册未开放），引导用户走显式注册入口；已命中已有用户的登录不受影响

---

## 5. 功能需求

### 5.1 核心需求

1. **Herald 后端 Apple identityToken 校验能力**：使用 Apple JWKS 公钥校验 identityToken 签名（RS256），校验签发者（`https://appleid.apple.com`）、受众（等于 Realm 的 Apple Client ID）、有效期，提取用户信息（sub、email、email_verified）

2. **Herald 后端 Apple native 认证端点**：接收 iOS App 传来的 Apple 身份凭证 + Client App 标识 + 可选的下游授权交易标识，校验通过后执行用户匹配/创建（含邮箱缺失时的占位邮箱补全），并根据是否存在下游标识选择直接签发会话或签发一次性授权码

3. **双分支会话建立**：
   - 直接会话分支：请求未携带下游授权交易标识时，签发与请求中 Client App 绑定的会话
   - 下游授权码分支：请求携带下游授权交易标识时，校验该事务并签发一次性授权码，接入方凭 PKCE 换取令牌

4. **用户匹配与现有流程一致**：native 创建/匹配用户时使用现有的匹配策略（open_id → email → 创建），确保 native 与 web 跳转两条路径关联到同一用户

5. **Apple Provider 配置复用**：native 路径只读取 Apple Provider 的 Client ID（用于受众校验）和启用状态，复用现有 `oauth_provider_config` 配置与现有 Realm Admin 管理界面，不新增配置项

### 5.2 验收目标

- iOS App 内用户通过苹果系统弹窗完成 Apple 授权后，提交 identityToken 给 Herald，Herald 校验通过并签发该 App 的会话或下游授权码，全程不离开 App
- Herald 后端正确校验 Apple identityToken 的签名、签发者、受众、有效期，拒绝篡改/伪造/过期的凭证
- 未注册用户通过 native 登录自动创建 Herald 账号并登录成功（仅在 Realm 已开启自动注册时；未开启时返回 `409 conflict` 并引导至显式注册入口）；首次建号且邮箱缺失时仍能建号（占位邮箱、未验证），不被拒绝
- 已有 Herald 账号的用户（之前通过 Apple web 跳转或其他方式注册）通过 native 登录时关联到已有账号，不产生重复账号
- native 登录在下游 Code+PKCE 场景中，校验通过后正确签发一次性授权码，接入方可凭 PKCE 换取令牌
- 未配置 Apple Provider 的 Realm 返回明确错误
- Apple JWKS 不可达时返回服务暂时不可用，不静默跳过校验

---

## 6. API 相关约束

**适用性**: 适用

- **能力边界**：新增一个 Apple native 认证端点，接收 Apple identityToken 凭证，返回会话信息或下游授权码重定向
- **访问控制**：该端点为公开端点（无需已认证身份），但必须校验 Apple identityToken 的有效性作为访问前提；下游授权码分支必须校验下游授权交易标识的合法性
- **Realm 数据边界**：端点路径包含 realmId，Apple identityToken 的受众必须与该 Realm 配置的 Apple Client ID 一致；用户匹配和创建限定在当前 Realm 内
- **兼容性**：与现有 OAuth Code+PKCE 流程（US-TP-001/015/016）兼容，下游授权交易标识机制复用现有实现；与 Apple web 跳转登录（US-RU-003）、Google One Tap、其他 Provider 共存；不依赖、不修改 Apple web redirect 的 client_secret 路径
- **安全约束**：
  - client_secret 不出现在任何公开响应中（native 路径本身不使用 client_secret）
  - Apple identityToken 不得被记录在 tracing span 中（遵循现有 OAuth handler 的 instrumentation 治理规范）
  - 会话建立后遵循现有 session 安全策略（如客户端绑定）

> 端点清单、参数 schema、状态码矩阵与错误模型不在 PRD 承载范围，下沉到技术设计。

---

## 7. 前端/交互约束

**适用性**: 不适用（Herald 自身前端）

Herald Web SPA（管理控制台 / 登录页）无任何改动。Apple Provider 的现有配置表单字段（Client ID、scopes、启用状态）对 native 路径已足够，无需新增字段。

iOS App 内的 Apple 原生授权弹窗由苹果系统渲染、接入方 iOS 代码触发，不在 Herald 控制范围，亦不在本仓库范围。

---

## 8. 已确认决策

| Decision ID | 状态 | 决策项 | 结论 | PRD 落点 | 来源 |
|---|---|---|---|---|---|
| `DEC-support-mobile-apple-login-001` | Applied | 客户端归属与端点分支 | 端点同时支持第一方（直接 session）与第三方（下游 Code+PKCE）双分支，完全对齐 Google One Tap 形态 | §2.1、§3.1、§4.1、§5.1、§6 | `.ai/decision-log/support-mobile-apple-login.md` |
| `DEC-support-mobile-apple-login-002` | Applied | client_secret 范围 | 仅做 native 路径（只校验 identityToken，不调 Apple token 端点、不使用 client_secret）；不修 Apple web redirect 的 JWT client_secret 自动签发缺陷 | §2.2、§3.1、§4.1、§6 | `.ai/decision-log/support-mobile-apple-login.md` |
| `DEC-support-mobile-apple-login-003` | Applied | 前端范围 | Herald Web SPA 无改动，纯后端能力；iOS App 由接入方自行实现，不在本仓库 | §2.1、§2.2、§7 | `.ai/decision-log/support-mobile-apple-login.md` |
| `DEC-support-mobile-apple-login-005` | Applied | 邮箱缺失建号策略 | Apple identityToken 邮箱为空且 open_id 未命中时，生成 `{sub}@apple.placeholder` 占位邮箱、标记未验证后建号（对齐微信占位邮箱范式）；Apple 中转邮箱作真实邮箱处理；与 Apple web redirect 拒绝建号的行为有意不同 | §4.1、§5.1、§5.2 | `.ai/decision-log/support-mobile-apple-login.md` |

> DEC-004（`verify_apple_id_token` 增加 `jwks_url` 参数、AppState 增加 `apple_jwks_url`）为 D2 工程取舍，属技术设计范畴，不改变产品语义，故不在本 PRD 记录；详见决策账本与技术预研报告。

---

## 9. 参考资料

- 用户故事：`docs/user-stories/auth/support-mobile-apple-login.md`（US-AL-001/002/003）
- 依赖的基线用户故事：`docs/user-stories/core/regular-user.md`（US-RU-003）、`docs/user-stories/auth/third-party-app.md`（US-TP-001/015/016）、`docs/user-stories/auth/oauth-extension.md`（US-OE-001）
- 架构参照 PRD：[Google One Tap 登录](google-one-tap.md)
- 占位邮箱范式参照 PRD：[微信 OAuth](wechat-oauth.md) §4.1
- OAuth 第三方登录基线：[OAuth](oauth.md)（§2.1 Apple 作为 web redirect SSO Provider、§4.1 brokered downstream-state redirect）
- 决策账本：`.ai/decision-log/support-mobile-apple-login.md`
- 技术预研：`.ai/tech-research/support-mobile-apple-login.md`
- 角色定义：`docs/user-stories/_roles.md`
