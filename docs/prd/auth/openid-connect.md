# OpenID Connect 最小兼容层 产品需求文档 (PRD)

**创建时间**: 2026-09-15
**优先级**: P1

---

## 1. 相关用户故事

> 详细故事与验收标准请查看 [docs/user-stories/auth/openid-connect.md](/docs/user-stories/auth/openid-connect.md)。

### 1.1 相关故事

**本轮故事**（来源 [docs/user-stories/auth/openid-connect.md](/docs/user-stories/auth/openid-connect.md)）：
- `[US-OC-001]` 通过 issuer 自动发现 OIDC 配置，优先级 P0
  - 角色：第三方应用开发者
  - 摘要：填一个 issuer 地址即可自动发现端点与验签公钥位置，零文档零 SDK 配置
- `[US-OC-002]` 标准 OIDC 客户端完成登录并获得可本地验签的身份令牌，优先级 P0
  - 角色：第三方应用开发者（Grafana/Vault 等现成工具集成方）
  - 摘要：授权码流程登录后拿到带签名 id_token，用公开公钥本地验签
- `[US-OC-003]` 获取已登录用户的身份资料，优先级 P0
  - 角色：第三方应用开发者
  - 摘要：用访问令牌查询标准身份资料（稳定标识、邮箱及验证状态、昵称）
- `[US-OC-004]` 签名公钥轮换对客户端透明，优先级 P1
  - 角色：第三方应用开发者
  - 摘要：密钥轮换经公开公钥自动适配，登录不中断

**复用的已发布故事**（既有授权链路对 OIDC 继续生效）：
- `[US-TP-001]` OAuth 授权码登录 Authorization Code + PKCE (P0)、`[US-TP-015]` 第三方 Web SPA 发起 SSO 登录 (P0)、`[US-TP-016]` 第三方后端用授权码换取令牌 (P0) — 来源 [docs/user-stories/auth/third-party-app.md](/docs/user-stories/auth/third-party-app.md)
- `[US-TP-008]` 配置 Client App 跳转地址白名单 (P0)、`[US-TP-010]` 启用/禁用 Client App (P0) — 来源 [docs/user-stories/auth/client-app-settings.md](/docs/user-stories/auth/client-app-settings.md)
- `[US-RU-008]` 访问第三方应用 (P0)、`[US-RU-010]` 从第三方 Web 应用跳转登录 (P0) — 来源 [docs/user-stories/core/regular-user.md](/docs/user-stories/core/regular-user.md)

### 1.2 优先级汇总

| 优先级 | 数量 | 关键故事 |
|--------|------|----------|
| P0 | 3（本轮）+ 6（复用） | issuer 发现、id_token 签发与本地验签、userinfo 身份资料 |
| P1 | 1（本轮） | 签名公钥轮换透明 |
| P2 | 0 | - |

---

## 2. 范围界定

### 2.1 包含功能

- OIDC 发现能力：第三方仅凭 issuer 地址即可发现授权端点、令牌端点、用户信息能力与验签公钥位置（标准 OIDC discovery）
- 验签公钥发布能力：公开 OIDC 验签公钥（JWKS），供客户端本地验签
- 身份令牌签发：既有 Authorization Code + PKCE 授权流程在授权请求携带 openid 语义时，令牌响应附带可本地验签的 id_token（含稳定用户标识、邮箱与验证状态、昵称等标准身份声明，回显授权请求的 nonce）
- 用户身份资料查询：客户端用访问令牌获取当前用户标准身份资料（标准 userinfo 能力）
- 签名密钥轮换：密钥轮换经公开公钥发布自动生效，保留旧公钥重叠期，对已接入客户端透明
- 标准 OIDC 客户端零 SDK 接入：Grafana、Vault 等开箱即用工具仅填 issuer 即可把 Herald 作为登录选项

### 2.2 不包含功能 (Out of Scope)

- OIDC scope 管理与用户主动同意（consent）授权页：维持"授权自动完成"现状（与 [docs/prd/auth/oauth.md](/docs/prd/auth/oauth.md) §2.2 一致）；本轮身份声明采用最小固定集
- OAuth client_credentials（M2M 机器对机器授权）：独立候选 feature，另行立项
- 标准化 refresh token：令牌刷新沿用现状（server-side token 不支持刷新；浏览器 token 走既有轮换机制）
- RP 发起的登出（RP-initiated logout）与 front/back-channel logout
- 完整身份平台能力：细粒度 scope 体系、动态客户端注册、CIBA 等
- SAML 2.0、SCIM 2.0（独立 feature）
- Herald 作为 OIDC Client 对接上游 IdP 的能力变化（现有 Provider 登录维持不变）

### 2.3 依赖项

- 既有 OAuth Authorization Code + PKCE 流程（authorize / token / 回调）— OIDC 身份层叠加其上
- Client App 系统 — client 身份识别与 redirect_uri 白名单沿用
- 登录同意闸门（[docs/prd/core/legal-consent-account-deletion.md](/docs/prd/core/legal-consent-account-deletion.md)）与二次认证（TOTP / Passkey / Step-up）
- 用户状态机（WaitVerified / Normal / Forbidden / Deleted）
- 签名密钥管理能力（平台级密钥底座，密文静态存储）

---

## 3. 需求概述

### 3.1 功能描述

Herald 的第三方接入原本面向"为 Herald 开发的应用"（自有 SDK + Ext API）；Grafana、Vault 等只支持标准 OIDC 的现成客户端无法使用 Herald 账号登录。本 PRD 在既有 OAuth 2.0 授权码流程之上补齐**最小 OIDC 身份层**：发现、验签公钥、id_token、userinfo。目标是让标准 OIDC 客户端"填一个 issuer 就能接入"，同时不改变 Herald 的 SaaS 底座定位、不引入 scope/consent 体系（DEC-openid-connect-001、DEC-openid-connect-004）。

### 3.2 关键特性

- 叠加而非替代：openid 语义只出现在授权请求中时才触发 id_token 签发，既有非 OIDC 客户端与 Herald SDK 行为完全不变（零回归硬约束）
- 遵循 Realm 隔离模型：OIDC 为 Realm 级能力，issuer 标识具体 Realm，跨 Realm 不串数据
- 身份声明最小固定集：稳定用户标识（sub）、邮箱与验证状态、昵称；不透出管理侧内部字段
- 既有安全规则全部继承：redirect_uri 精确匹配白名单、state 与授权码一次性、PKCE S256、未认证端点 per-IP 限流、账号禁用拒绝、登录同意闸门
- 平台级签名密钥：RS256 签名，密钥为 Realm 无关的平台级资产（Realm 隔离由 issuer 与受众校验保证）；公钥公开可发现，旧公钥保留重叠期

---

## 4. 业务规则与状态

### 4.1 业务规则

**接入与发现：**
- OIDC 能力随 Client App 模型存在，不设独立 OIDC 开关：Realm 内已注册且启用的 Client App 即可被标准客户端使用
- 发现配置必须与 Realm 一一对应（issuer 标识 Realm）；issuer 指向不存在的 Realm 时发现失败，不返回部分配置
- issuer 由 Realm 的公开访问地址派生：Realm 启用自定义域名时使用自定义域名，否则使用站点公开地址；域名切换后 issuer 随之切换
- Client App 禁用后，其 OAuth/OIDC 授权请求全部拒绝（沿用既有规则，实时生效）

**身份令牌（id_token）：**
- 仅当授权请求携带 openid 语义时签发 id_token；不含 openid 语义的既有请求响应保持不变（兼容性硬约束）
- openid 检测只认区分大小写的 `openid` scope token；其余 scope token 一律透传不解析不拒绝
- id_token 的签发时机与授权码一致，同受登录同意闸门约束：同意缺失或版本过期时不签发（沿用 [docs/prd/core/legal-consent-account-deletion.md](/docs/prd/core/legal-consent-account-deletion.md) §4.1 登录即同意规则，不因 OIDC 豁免）
- id_token 声明使用最小固定集：稳定用户标识（sub）、签发者、受众（Client App）、有效期、签发时间、邮箱与验证状态、昵称、nonce 回显（如授权请求携带）
- 授权请求携带 nonce 时回显到 id_token（协议安全参数，供客户端绑定请求防重放）
- 用户账号处于禁用/删除状态时不签发任何令牌（openid 路径在令牌交换时复查用户状态）
- TOTP、Passkey 等二次认证与授权码流程的既有组合在 OIDC 场景下行为不变；全部登录支路（密码、TOTP、Passkey、LDAP、社交登录 broker）均透传 openid 语义

**用户身份资料（userinfo）：**
- 查询需携带有效访问令牌，且该令牌须来自携带 openid 语义的授权流程：令牌无效、过期或已撤销时拒绝（401 语义）；令牌有效但流程未请求 openid 语义时同样拒绝（403 语义），不返回任何资料——身份资料只服务 openid 语义授权的凭证
- 返回资料与 id_token 身份声明一致（同源同值），邮箱验证状态如实呈现，不虚报已验证
- 同时支持标准 userinfo 的 GET 与 POST 两种查询方式
- 响应禁止缓存（身份资料不得被中间层存储后服务给其他用户）

**签名密钥：**
- 验签公钥公开可发现（JWKS），只暴露公钥与非敏感元数据
- 密钥轮换时新密钥即时生效，旧公钥保留 7 天重叠期；重叠期内旧令牌仍可验签
- 轮换为运维侧内部操作，纳入审计；并发轮换冲突时后到者明确失败
- 私钥以密文静态存储，仅公开公钥部分

**审计与可观测：**
- OIDC 身份令牌签发、密钥轮换纳入既有审计链路，与既有 OAuth 授权审计同级
- OIDC 未认证端点纳入既有 per-IP 速率限制体系

### 4.2 关键状态与异常

- **签名密钥状态**: Active（签名并发布）/ Retained（重叠期只发布）/ Retired（不再发布）— 轮换后旧密钥进入 Retained，重叠期满转 Retired 并从公开公钥集中移除
- **发现失败**: issuer 与 Realm 不匹配时明确失败，不静默回退到默认配置
- **令牌拒绝**: 访问令牌无效/过期/撤销或流程未携带 openid 语义时 userinfo 一律拒绝，不返回资料
- **授权拒绝**: 禁用用户、禁用 Client App、白名单外回调均拒绝，错误形态对标准客户端可理解（沿用既有 OAuth 异常处理原则）

---

## 5. 功能需求

### 5.1 核心需求

- 提供标准 OIDC 发现能力：由 issuer 可发现授权端点、令牌端点、userinfo 能力、验签公钥位置与支持的算法
- 提供公开验签公钥能力（JWKS）：包含全部 Active 与重叠期 Retained 密钥的公钥
- 令牌响应支持附带 id_token：授权请求含 openid 语义时，令牌交换响应附带可本地验签的身份令牌
- 令牌交换兼容标准请求编码：既接受标准 OAuth 表单编码请求体（RFC 6749，标准 OIDC 客户端默认形态），也兼容既有 JSON 请求体，双方行为不变
- 提供标准 userinfo 能力：返回与 id_token 同源的身份声明
- 提供密钥轮换能力（运维侧）：轮换操作可审计，轮换过程对客户端透明
- 既有 Herald SDK、Ext API、Device Code、浏览器 token 等接入方式回归不变

### 5.2 验收目标

- 标准 OIDC 客户端（如 Grafana 类工具）仅凭 issuer 配置完成"发现 → 登录 → 本地验签 → 获取用户资料"全流程，零 Herald SDK、零私有文档
- 授权请求不含 openid 语义时，既有 OAuth 客户端与 Herald SDK 的行为与接入前完全一致（零回归）
- 禁用用户、禁用 Client App、白名单外回调在 OIDC 场景下同样被拒绝
- 邮箱验证状态在 id_token 与 userinfo 中如实呈现
- 密钥轮换后新登录正常验签，重叠期内旧令牌仍可验签
- 同意闸门缺失时不签发 id_token，补全同意后重新登录可完成

---

## 6. API 相关约束

**适用性**: 适用

- 新增对外能力均为只读/签发类：发现与验签公钥为公开能力（无用户级敏感数据），userinfo 为访问令牌保护的只读能力，令牌响应为既有令牌交换能力的增量
- 遵循 Realm 隔离模型：issuer 标识 Realm，发现、签发、userinfo 均不得跨 Realm（userinfo 在服务端强制令牌所属 Realm 与路径 Realm 一致）
- 未认证的发现与公钥端点纳入既有 per-IP 速率限制体系（与 `/authorize`、`/token` 同级治理），超限返回限流错误
- userinfo 的访问控制以访问令牌有效性 + openid 语义授权为准，拒绝响应不泄露差异信息（无效令牌与未授权语义不区分具体原因细节）
- 协议接口遵循 OIDC 标准语义（发现文档字段、JWKS 公钥结构、userinfo 查询方式、scope token 大小写敏感）；令牌端点的公共客户端属性（不校验 client secret）在发现文档中如实声明
- 兼容性要求：新增能力不得改变既有 OAuth 端点在非 openid 请求下的行为契约（响应字段仅增量出现）

---

## 7. 前端/交互约束

**适用性**: 适用（最小）

- Herald 登录/授权页无新增用户交互：OIDC 授权请求经既有 OAuth 上下文透传机制进入登录页，登录完成后跳回逻辑不变
- 用户看到的登录体验与既有第三方授权登录一致（含 TOTP/Passkey/同意闸门等既有环节）
- 管理控制台无新增配置界面（不设 OIDC 独立开关；Client App 管理沿用现状）
- 文档站提供标准 OIDC 客户端接入指引（issuer 配置示例、Grafana 类工具接入说明、密钥轮换运维说明）

---

## 8. 已确认决策

| Decision ID | 状态 | 决策项 | 结论 | PRD 落点 |
|---|---|---|---|---|
| `DEC-openid-connect-001` | Applied | Herald 产品定位 | 维持 SaaS 底座定位，不进入完整身份平台赛道；仅检测 `openid` 语义，不建 scope/consent 体系 | §2.2、§3.1、§4.1 |
| `DEC-openid-connect-004` | Applied | 能力范围 | 最小 OIDC 兼容层：id_token + JWKS + discovery + userinfo 叠加既有授权码流程，无 scope/consent 体系、无标准化 refresh；第三方接入边界由"仅为 Herald 开发的应用"扩展为"任意标准 OIDC 客户端亦可接入" | §2.1、§2.2、§3.1 |
| `DEC-openid-connect-005` | Applied | issuer 形态 | 路径式 issuer `{origin}/api/oauth/{realmId}`（origin 随 Realm 自定义域名切换）；发现、公钥、userinfo 能力挂同一 Realm 路径前缀，发现 URL 按 OIDC 标准 append 语义拼接 | §3.2、§4.1 |
| `DEC-openid-connect-006` | Applied | 签名密钥模型 | RS256、平台级密钥（Realm 无关）；私钥密文静态存储；启动自举首把密钥；内部运维端点轮换；7 天重叠期；JWKS 发布 Active 与未到期 Retained | §3.2、§4.1、§4.2 |
| `DEC-openid-connect-007` | Applied | 令牌共存 | 不引入新令牌类型：id_token 为既有令牌响应的增量字段（仅 openid 语义时出现）；userinfo 复用既有浏览器访问令牌认证链；nonce 透传回显 | §2.1、§4.1、§5.1 |

> 追溯说明：`DEC-openid-connect-002`（旧接入边界）、`DEC-openid-connect-003`（旧重启条件）已被 `DEC-openid-connect-004` 取代，不作为当前事实；完整决策账本见 `.ai/decision-log/openid-connect.md`（工作流内部追溯件）。

---

## 9. 参考资料

### 9.1 来源文件
- 用户故事：[docs/user-stories/auth/openid-connect.md](/docs/user-stories/auth/openid-connect.md)（US-OC-001 ~ US-OC-004）
- 复用用户故事：[docs/user-stories/auth/third-party-app.md](/docs/user-stories/auth/third-party-app.md)、[docs/user-stories/auth/client-app-settings.md](/docs/user-stories/auth/client-app-settings.md)、[docs/user-stories/core/regular-user.md](/docs/user-stories/core/regular-user.md)
- 接入指引（文档站）：`docs-web/content/docs/integration/oidc.mdx`（中英双语）

### 9.2 与相关 PRD 的覆盖关系
- [docs/prd/auth/oauth.md](/docs/prd/auth/oauth.md)：OIDC 身份层叠加于其 Authorization Code + PKCE 流程之上，不改变该 PRD 任何既有规则；其 §2.2"无细粒度 scope 授权页、授权自动完成"的边界对 OIDC 层继续适用（本 PRD §2.2 沿用）
- [docs/prd/core/legal-consent-account-deletion.md](/docs/prd/core/legal-consent-account-deletion.md)：登录同意闸门对 OIDC 授权不豁免，id_token 签发时机与授权码一致
- [docs/prd/auth/totp.md](/docs/prd/auth/totp.md)、[docs/prd/auth/passkey.md](/docs/prd/auth/passkey.md)：二次认证与 OIDC 授权流程的既有组合行为不变
