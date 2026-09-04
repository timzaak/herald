# 自建用户 UI（Custom User UI） 产品需求文档 (PRD)

**创建时间**: 2026-07-16
**优先级**: P1
**所属域**: integration

---

## 1. 相关用户故事

> 详细故事与验收标准请查看 `docs/user-stories/` 中对应文档。

### 1.1 相关故事

本功能用户故事（`docs/user-stories/integration/custom-user-ui.md`）：

- `[US-CUI-001]` 集成方前端完成注册与邮箱验证，P0 — 第三方应用开发者
- `[US-CUI-002]` 集成方前端完成登录获得浏览器 token，P0 — 第三方应用开发者
- `[US-CUI-003]` 集成方前端完成找回/重置密码，P0 — 第三方应用开发者
- `[US-CUI-004]` 集成方前端查看资料并修改昵称，P1 — 第三方应用开发者
- `[US-CUI-005]` 集成方前端完成高危安全操作（改密码/二因素/注销账号），P0 — 第三方应用开发者
- `[US-CUI-006]` 集成方前端完成登出，P1 — 第三方应用开发者
- `[US-CUI-007]` 集成方前端完成积分与交易查看，P0 — 第三方应用开发者
- `[US-CUI-008]` 集成方前端完成充值/购买，P0 — 第三方应用开发者
- `[US-CUI-009]` 集成方前端完成发票与订阅查看，P1 — 第三方应用开发者

本组故事表达"集成方前端可跨域触达"这些能力，不复制既有用户故事的验收内容。具体业务验收仍归属既有 billing/auth 用户故事；本组只验收"跨域自建 UI"这一集成维度的目标。

既有可引用用户故事（表达"同一业务能力，Herald 内部前端经凭证授权调用"）：

- 注册/登录/资料：`docs/user-stories/core/regular-user.md`（US-RU-001/002/004/005/007/014）
- TOTP：`docs/user-stories/auth/totp.md`（US-TO-002/003/004/005）
- Passkey：`docs/user-stories/auth/passkey.md`（US-PK-004~009）
- 积分/交易：`docs/user-stories/billing/points-user.md`（US-PU-001/002）、`docs/user-stories/billing/credit-bucket.md`（US-CB-005/006）
- 购买：`docs/user-stories/billing/points-package-purchase.md`（US-PU-006）
- 发票：`docs/user-stories/billing/invoice.md`（US-IV-008/011）
- 订阅：`docs/user-stories/billing/subscription.md`（US-BI-006/009）
- OAuth 后端换码：`docs/user-stories/auth/third-party-app.md`（US-TP-015/016，该组保留"后端换码"原义，浏览器 token 路线见本 PRD）

### 1.2 优先级汇总

| 优先级 | 数量 | 关键故事 |
|--------|------|----------|
| P0 | 6 | US-CUI-001/002/003/005/007/008 |
| P1 | 3 | US-CUI-004/006/009 |

---

## 2. 范围界定

### 2.1 包含功能

集成方可在自家前端（无自家后端）跨域自建**终端用户的全套 UI**，覆盖未认证身份流程与登录后完整个人中心。Herald 自有前端变为可选参考实现，不再是唯一前端。

**未认证身份流程（公开端点跨域开放）**：

- **注册 + 邮箱验证**：集成方前端以 Client App 上下文提交注册；邮箱验证完成后引导到该 Client App 预登记的验证结果页。回跳目标由服务端配置解析，不接受请求提供任意 URL。
- **登录签发浏览器 token**：集成方前端用账号密码登录，Herald 签发跨域浏览器 token（access + refresh）而非设 cookie；二因素（TOTP/Passkey）流程同步支持。
- **找回/重置密码**：集成方前端以 Client App 上下文发起找回密码、提交重置；重置链接引导到该 Client App 预登记的重置页。

**登录后个人中心（浏览器 token 跨域调用，完整用户自服务权限）**：

- **资料查看与昵称编辑**：查看当前用户资料（邮箱/昵称/状态），并复用现有能力修改昵称；头像编辑不在本轮范围。
- **高危安全操作**：改密码、启用/禁用/验证 TOTP、注册/删除/重命名 Passkey、注销账号（不可逆）。修改密码、绑定或移除认证器、注销账号必须先完成独立的重新认证；仅重命名 Passkey 不要求重新认证。
- **登出**：吊销当前浏览器 token 及其 refresh token 家族。
- **积分/交易**：查看当前用户积分余额（按账户分组）、交易历史。
- **充值/购买**：查看购买选项（套餐/价目）、发起购买、轮询支付状态；支付最终确认由支付提供商页面承接。
- **发票/订阅**：查看发票列表/详情、申请开票、查看我的订阅。

**跨域基础设施**：

- **双轨浏览器凭证类**：浏览器 token 分两类——`FirstParty`（Herald 自有前端，经 Authorization Code + PKCE 换取，执行完整 RBAC）与 `CustomUserUi`（集成方自建 UI，经 `/login` 直接签发，受用户自服务权限上限约束）。两者均为用户绑定、可吊销、旋转 refresh token 续期；公开 `clientId` 或 Origin 单独都不能把受限 token 提升为 FirstParty。
- **Bearer 鉴权分支**：身份解析接受 `Authorization: Bearer`，按凭证类产出身份与上下文；不再依赖 cookie session。
- **浏览器凭证权限上限**：`CustomUserUi` token 只获得明确归类的用户自服务权限，管理员能力和未知能力默认拒绝。该上限由授权层执行，不依赖 URL 前缀。CORS 不参与权限判定。
- **per-Client App 允许 origin**：Client App 新增允许 origin 白名单，按请求 Origin 动态放行跨域；精确匹配。
- **per-Client App 身份流程回跳**：Client App 配置邮箱验证与密码重置的预登记回跳目标；邮件中的服务端状态绑定 Realm、Client App 和流程类型。
- **Passkey RP 隔离**：获准的 Client App HTTPS origin 使用其 host 作为 WebAuthn RP ID，凭证按 RP 保存、查询和验证；既有 Passkey 继续归属原 Herald RP，不跨 RP 复用。
- **统一重新认证**：高危操作消费短时、单次、绑定用户/Client App/目标操作的重新认证结果；可使用账户已绑定的密码、TOTP 或要求用户验证的 Passkey 完成。
- **token 生命周期（旋转 refresh token）**：短时效 access token（浏览器内存持有）+ 旋转 refresh token（每次刷新换发新 RT、旧 RT 作废）+ 复用检测（旧 RT 被再次使用时吊销整个 token 家族）+ refresh token 绝对有效上限 + 吊销能力。
- **未认证身份端点防护**：人机验证（Turnstile）按当前请求绑定的 Client App 的 Turnstile 配置执行（Turnstile 配置在 Client App 级），维持 IP/identifier 限流，跨域开放后不新增 client 维度限流。

### 2.2 不包含功能 (Out of Scope)

- **跨域会话 cookie**：浏览器 token 不走 cookie，规避 CSRF。身份解析不再读取 cookie。
- **管理员能力对 CustomUserUi 开放**：`CustomUserUi` token 的权限上限不包含管理员能力；管理员 dashboard、RBAC、Realm 配置和用户管理等仍需 `FirstParty` 凭证。此边界不依赖 URL 前缀。
- **OAuth PKCE 作为自建 UI 登录入口**：集成方自建 UI 的登录入口经 `/login` 签发 `CustomUserUi` token。OAuth Authorization Code + PKCE 链路用于签发 `FirstParty` token（Herald 自有前端），不作为自建 UI 的登录入口；两类凭证不混用。
- **publishable key 独立标识用户**：浏览器 token 必须用户绑定，不存在匿名 publishable 凭证。
- **出站 webhook**：支付状态由前端轮询查询。
- **完整 OIDC 接入标准化**：见 `docs/prd/integration/`（独立规划）。
- **服务端静默滑动续期**：业界标准用旋转 refresh token。
- **隐藏 iframe silent auth**：依赖第三方 cookie。
- **JWT 本地校验（首期）**：首期用不透明 session token + Redis；JWT 形态为后续可选增强。
- **官方 JS/浏览器 SDK**：原「本轮不交付」（D-SCOPE-03）已由 [js-sdk PRD](/docs/prd/integration/js-sdk.md) 取代（DEC-js-sdk-003）；官方 JS 浏览器 SDK 现作为独立能力交付，封装认证生命周期子集。
- **头像编辑能力**：本轮只复用现有昵称编辑，不新增头像上传或编辑。
- **refresh token 浏览器集成契约**：refresh token 的浏览器存储位置、多标签页并发、网络失败恢复与重试规则由后续集成文档承接；服务端旋转、复用检测、绝对上限和吊销在 Scope 内。

### 2.3 依赖项

- 复用现有 `/login`（含二因素）、`/register`、`/reset_password`、`/verify_email` 端点；这些已是公开端点，跨域开放仅需 CORS 放行 + 身份流程回跳配置。
- 复用现有 Authorization Code + PKCE `/token` 端点签发 `FirstParty` token。
- 复用现有用户资料、认证器管理、积分、购买、发票和订阅自服务能力；是否允许由用户自服务权限决定，不以路由前缀分类。
- 复用现有身份解析与鉴权链路（新增 Bearer 分支与凭证类判定）。
- 复用现有 Client App 实体与自定义域名 host→realm 映射基础设施。
- 依赖现有 CORS 能力（从单 origin 改 per-Client App 动态谓词）。
- 依赖 Client App 配置扩展、Passkey RP 归属迁移和统一重新认证能力。

---

## 3. 需求概述

### 3.1 功能描述

让集成方在自家前端（无自家后端）自建终端用户的全套 UI——从注册、登录、找回密码，到登录后的完整个人中心。Herald 从"提供唯一前端"转变为"提供完整 API 面 + 可选参考实现前端"，集成方按自身品牌与交互自建整套用户体验。

补齐 SaaS 底座的"完整用户 UI 自建"集成路径，降低集成摩擦，契合 Herald"账号+计费+积分一站式 SaaS 底座"定位。

### 3.2 关键特性

- 未认证身份流程（注册/登录/找回密码）跨域开放，登录签发浏览器 token 而非 cookie。
- 双轨浏览器凭证类：`FirstParty`（PKCE 换取、完整 RBAC）与 `CustomUserUi`（`/login` 签发、用户自服务权限上限）。
- 登录后 `CustomUserUi` token 跨域调用完整用户自服务权限集合（含经重新认证的高危写）；管理员能力与未知能力默认拒绝。
- 浏览器可持有、用户绑定、可吊销、旋转 refresh token 续期的用户 token。
- per-Client App 允许 origin 动态 CORS 放行。

---

## 4. 业务规则与状态

### 4.1 业务规则

- **凭证类区分**：浏览器 token 分 `FirstParty` 与 `CustomUserUi` 两类，互不混用。`FirstParty` 只能由内置保留 Client App 经 Authorization Code + PKCE 换取；`CustomUserUi` 由 `/login` 直接签发。
- **用户绑定**：两类浏览器 token 都必须绑定单一登录用户，不可为 realm/client 级凭证。
- **权限控制**：`CustomUserUi` token 的权限上限为明确归类的用户自服务能力（资料/改密码/TOTP/Passkey/注销账号/登出/积分/交易/购买/发票/订阅）；管理员能力和未知能力默认拒绝。`FirstParty` token 执行完整 RBAC，不受该上限约束。该规则由授权层执行，不依赖路由名称。
- **数据边界**：浏览器 token 只能访问当前登录用户自己的数据；跨用户访问拒绝。
- **未认证身份端点**：注册/登录/找回密码/重置密码/邮箱验证为公开端点，跨域开放后人机验证（Turnstile）按当前请求绑定的 Client App 的配置执行，维持限流防护，不新增 client 维度限流。
- **origin 精确匹配**：Client App 允许 origin 必须精确、可信，禁止通配或不安全形式。
- **CORS 非授权边界**：Origin/CORS 只控制浏览器跨域响应；即使 Origin 缺失或可伪造，服务端仍须独立验证 token 的用户、Realm、Client App、用途和权限。
- **登录签发**：跨域登录成功签发浏览器 token（access + refresh），不设 cookie；二因素流程同步支持。
- **生命周期（旋转 refresh token）**：短时效 access token（内存）+ 旋转 refresh token（每次刷新换发新 RT、旧 RT 作废）+ 复用检测（旧 RT 再用吊销整个家族）+ RT 绝对有效上限。
- **吊销**：可即时吊销浏览器 token（access token 或其 refresh token 家族）；吊销不误伤同一用户其他正常凭证/会话。
- **CORS 形态**：非通配 origin + `allow_credentials(false)`；认证走 Bearer token（`Authorization` 头），无需 credentialed 请求。
- **注销账号不可逆**：浏览器 token 调用注销账号后，后果（匿名化、取消订阅、清除会话）不可恢复。
- **高危操作重新认证**：修改密码、绑定或移除 TOTP/Passkey、注销账号前，必须使用账户已绑定的密码、TOTP 或要求用户验证的 Passkey 完成重新认证；重新认证结果短时、单次并绑定目标操作。Passkey 重命名不属于高危操作。
- **Passkey RP 隔离**：Passkey 只在其注册 RP 下可见和可用；Client App origin 之间及 Client App 与 Herald 原 RP 之间不共享 credential。
- **安全回跳**：邮箱验证和密码重置只回跳到 Client App 预登记目标；服务端状态绑定 Realm、Client App 与流程类型，拒绝任意外部回跳地址。
- **支付边界**：浏览器 token 只负责发起购买与轮询状态，支付最终确认由支付提供商页面承接。

### 4.2 关键状态与异常

- **跨域 origin 未配置 / 不在白名单** → 拒绝跨域请求（覆盖身份端点与用户面）。
- **CustomUserUi token 请求管理员或未归类能力** → 无论 URL 前缀及用户是否同时拥有管理员角色，均因凭证权限上限而拒绝。
- **token 访问非当前用户数据** → 拒绝（身份混淆）。
- **access token 过期** → 拒绝并提示需刷新；前端用 refresh token 静默换发后重试，用户不感知。
- **refresh token 过期 / 到达绝对上限 / 被吊销** → 拒绝刷新，引导重新登录。
- **refresh token 复用（旧 RT 再次被使用）** → 吊销整个 token 家族，后续刷新全部失败，引导重新登录。
- **token 被吊销 / 伪造 / 凭证类不匹配** → 拒绝并提示未授权/凭证类不匹配。
- **登录密码错误 / 人机验证失败 / 限流** → 拒绝登录并返回相应提示。
- **高危操作重新认证失败** → 拒绝操作且不消费无效结果，要求用户重新完成认证。
- **Client App 被禁用** → 其浏览器 token 联动失效（与既有 API Key 联动一致）。
- **Passkey 来自其他 RP** → 当前 Client App 不返回、不使用该 credential；没有当前 RP credential 时走既有密码/TOTP 回退。
- **高危操作缺少、过期、已消费或目标不匹配的重新认证结果** → 拒绝并要求重新认证。
- **身份邮件请求的 Client App 无效、已禁用或回跳未登记** → 拒绝启动流程；邮件落地时再次校验绑定状态。
- **注销账号完成** → 账户不可恢复，当前及后续 token 全部失效。
- **FirstParty 凭证类由 PKCE 换取且内置保留 Client App 标记决定**：普通 Client App 即便完成 PKCE 也不会升级为 FirstParty。

---

## 5. 功能需求

### 5.1 核心需求

**未认证身份流程**：

- **FR-1（注册与邮箱验证）**：集成方前端以有效 Client App 上下文跨域提交注册；邮箱验证完成后引导到该 Client App 预登记的验证结果页，拒绝任意回跳 URL。
- **FR-2（登录签发浏览器 token）**：集成方前端用账号密码跨域登录，Herald 签发 `CustomUserUi` 浏览器 token（access + refresh）而非设 cookie；二因素（TOTP/Passkey）流程同步支持。
- **FR-3（找回/重置密码）**：集成方前端以有效 Client App 上下文跨域发起找回密码、提交重置；重置链接只引导到该 Client App 预登记的重置页。

**登录后个人中心（CustomUserUi token 跨域）**：

- **FR-4（资料查看与昵称编辑）**：用浏览器 token 查看当前用户资料，并修改当前用户昵称。
- **FR-5（高危安全操作）**：用浏览器 token 完成改密码、TOTP 启用/禁用/验证、Passkey 注册/删除/重命名、注销账号；其中改密码、绑定或移除认证器、注销账号必须消费有效的重新认证结果。
- **FR-6（登出）**：用浏览器 token 登出，吊销当前 token 及其 refresh token 家族。
- **FR-7（积分/交易）**：用浏览器 token 查看当前用户积分余额（按账户分组）与交易历史。
- **FR-8（充值/购买）**：用浏览器 token 查看购买选项、发起购买、轮询支付状态。
- **FR-9（发票/订阅）**：用浏览器 token 查看发票列表/详情、申请开票、查看我的订阅。

**跨域基础设施**：

- **FR-10（Bearer 鉴权与凭证类判定）**：身份解析接受 `Authorization: Bearer`，按凭证类产出身份与上下文；`CustomUserUi` 授权层只授予用户自服务权限，管理员与未知能力默认拒绝；`FirstParty` 执行完整 RBAC。
- **FR-11（origin 白名单）**：Realm 管理员可为 Client App 配置允许 origin 列表，精确匹配、即时生效、禁止不安全形式。
- **FR-12（token 生命周期）**：短时效 access token（内存）+ 旋转 refresh token（每次刷新换发新 RT、旧 RT 作废）+ 复用检测 + RT 绝对有效上限。
- **FR-13（复用检测）**：旧 refresh token 被再次使用时，吊销该 token 家族。
- **FR-14（吊销）**：可即时吊销浏览器 token（access token 或其 refresh token 家族），吊销不误伤其他正常凭证。
- **FR-15（Passkey RP 隔离）**：Passkey credential 按实际 RP 保存、查询和验证；既有 credential 归属原 Herald RP，不跨 Client App origin 复用。
- **FR-16（统一重新认证）**：为高危操作提供短时、单次且绑定用户、Client App 和目标操作的重新认证结果。
- **FR-17（安全回跳）**：邮箱验证与密码恢复状态绑定 Client App，并只使用预登记回跳目标。
- **FR-18（Client App 禁用联动）**：Client App 禁用后拒绝其新身份流程，并使其浏览器 token 家族失效；不得影响其他 Client App 的正常会话。

### 5.2 验收目标

- 集成方前端在无自家后端的情况下，可完成注册→邮箱验证→登录→个人中心全套流程。
- 登录签发 `CustomUserUi` 浏览器 token（非 cookie），access token 到期可静默刷新，refresh token 到绝对上限需重新登录。
- 旧 refresh token 复用导致整个 token 家族被吊销。
- 集成方前端可跨域完成高危安全操作；缺少有效重新认证时全部拒绝。
- 集成方前端可跨域查看积分/交易、发起充值/购买、轮询支付、查看发票/订阅。
- 未配置为允许 origin 的域名跨域请求被拒绝（覆盖身份端点与用户面）。
- `CustomUserUi` token 请求管理员或未归类能力被拒绝，包括 token 所属用户同时拥有管理员角色的情况。
- 用 token 访问非当前用户数据被拒绝。
- 注销账号后该用户全部 token 失效且操作不可恢复。
- 不同 Client App origin 的 Passkey 相互隔离，既有 Herald RP credential 不被扩展到其他 RP。
- 找回密码/邮箱验证只引导到对应 Client App 预登记页面，任意回跳 URL 被拒绝。
- 禁用 Client App 后，其新身份流程和既有浏览器 token 立即失败，其他 Client App 不受影响。
- `FirstParty` token 只能由内置保留 Client App 经 PKCE 换取，普通 Client App 即便完成 PKCE 也不升级为 FirstParty。

---

## 6. API 相关约束

**适用性**: 适用

- **凭证类边界**：`FirstParty` 经 Authorization Code + PKCE 换取，执行完整 RBAC；`CustomUserUi` 经 `/login` 签发，只获得用户自服务权限。两类互不混用，不能用 URL 前缀推导授权结果。
- **访问控制原则**：浏览器 token 绑定用户、Realm、Client App 和凭证用途；身份解析按凭证类产出身份与上下文。handler 继续执行当前用户/Realm/RBAC 检查，授权层额外执行 `CustomUserUi` 凭证权限上限；新增能力默认拒绝。
- **租户/realm 数据边界**：浏览器 token 只能访问当前登录用户自己的数据；Client App 禁用时其浏览器 token 联动失效。
- **未认证身份端点防护**：人机验证（Turnstile）按当前请求绑定的 Client App 的配置执行（Client App 级配置），维持限流，不新增 client 维度限流。
- **CORS 兼容性**：非通配 origin + `allow_credentials(false)`（Bearer token 经 `Authorization` 头传递，无需 credentialed 请求），从单 origin 改 per-Client App 动态放行。
- **Passkey 兼容性**：Client App HTTPS origin 对应独立 RP；credential 按 RP 隔离，既有 credential 保持原 RP 归属。
- **高危操作**：修改密码、绑定或移除认证器、注销账号必须消费重新认证结果；重新认证支持账户已绑定的密码、TOTP 或要求用户验证的 Passkey。
- **邮件流程**：验证/重置流程携带服务端生成的 Client App 绑定状态，回跳目标只能取自预登记配置。

> 端点清单、参数 schema、状态码矩阵与迁移细节不在 PRD 承载范围，下沉到技术设计。

---

## 7. 前端/交互约束

**适用性**: 适用

- **页面入口**：集成方在自家前端自建全套用户 UI（可使用官方 [JS 浏览器 SDK](/docs/prd/integration/js-sdk.md) 封装认证生命周期，或用标准 `fetch` + `Authorization: Bearer` 自行实现）；页面布局与交互由集成方决定，Herald 不托管这些页面。Herald 自有前端变为可选参考实现。
- **未认证流程入口**：注册/登录/找回密码页面由集成方自建，直接调 Herald 公开端点；登录成功返回 `CustomUserUi` token set。
- **登录后入口**：个人中心（资料/安全/积分/充值/发票/订阅）由集成方自建，用浏览器 token 跨域调用。
- **状态反馈**：token 失效时引导重新登录；origin 未配置、权限不足、Passkey RP 不匹配或需要重新认证时返回可区分的错误；注销账号不可逆需明确确认。
- **Herald 自有前端**：经 Authorization Code + PKCE 换取 `FirstParty` token，access token 内存持有、refresh token 与 PKCE 状态持久化、启动时刷新恢复、统一 client 注入 Bearer、单次 401 静默刷新、token-only 登出。作为可选参考实现保留。

> 官方 JS 浏览器 SDK 已作为独立能力交付，见 [js-sdk PRD](/docs/prd/integration/js-sdk.md)（原「本轮不交付官方 JS SDK」由 DEC-js-sdk-003 取代）。

---

## 8. 已确认决策

- **D-SEC-01（安全姿态）**：选定"跨域 + 浏览器持有用户 token"主路线。token 明确进入集成方前端，身份解析不再依赖 cookie；传输与凭证模型类比业界标准（浏览器持 token + Bearer + 不走跨域 cookie + 旋转 refresh token）。
- **D-CRED-01（双轨凭证类）**：浏览器 token 分 `FirstParty` 与 `CustomUserUi` 两类。`FirstParty` 由内置保留 Client App（数据库内部标记，不进入 Admin/Ext API DTO）经 Authorization Code + PKCE 换取，执行完整 RBAC；`CustomUserUi` 由 `/login` 直接签发，受用户自服务权限上限约束。普通 Client App 即便完成 PKCE 也不升级为 FirstParty。判定在服务端 fail-closed，不接受请求体声明凭证类。
- **D-SCOPE-FULL（用户自服务权限全覆盖）**：`CustomUserUi` token 获得完整用户自服务权限集合，含经重新认证的高危写；管理员能力和未知能力默认拒绝。授权依据是主体、Realm、凭证用途与权限上限，不是路径。即使 token 所属用户拥有管理员角色，`CustomUserUi` 凭证也不能调用管理员能力。
- **D-LOGIN-01（登录链路）**：集成方自建 UI 登录入口经 `/login` 签发 `CustomUserUi` token，跨域场景不设 cookie；二因素流程同步支持。OAuth Authorization Code + PKCE 链路用于签发 `FirstParty` token（Herald 自有前端），不作为自建 UI 的登录入口。
- **D-TOK-01（生命周期 = 旋转 refresh token）**：短时效 access token（内存）+ 旋转 refresh token（每次刷新换发新 RT、旧 RT 作废）+ 复用检测（旧 RT 再用吊销整个家族）+ RT 绝对有效上限。
- **D-TOK-02（吊销）**：浏览器 token 变体支持即时吊销。OAuth PRD §2.2 原"Token 撤销（当前不支持）"对浏览器 token 变体不再成立（见 `docs/prd/auth/oauth.md` §2.2 修订），server-side token 维持原状。
- **D-PROTECT-01（身份端点防护，Client App 级 Turnstile）**：未认证身份端点跨域开放后，人机验证（Turnstile）按当前请求绑定的 Client App 的 Turnstile 配置执行（Turnstile 配置在 Client App 级，不再由 Realm 承载，见 [docs/prd/core/realm-settings.md](../core/realm-settings.md) §3.1/§8）；维持 IP/identifier 限流，不新增 client 维度限流。
- **D-AUTHZ-01（权限边界）**：CORS 不是授权机制。`CustomUserUi` token 只获得用户自服务权限上限，管理员与未知能力默认拒绝；新增能力必须显式归类。
- **D-PASSKEY-01（RP 隔离）**：获准 Client App HTTPS origin 使用自身 host 作为 RP ID，credential 按 RP 隔离；既有 credential 继续归属原 Herald RP。
- **D-REAUTH-01（高危操作确认）**：改密码、绑定或移除 TOTP/Passkey、注销账号必须先完成短时单次重新认证；可使用已绑定密码、TOTP 或要求用户验证的 Passkey。仅重命名 Passkey 不要求重新认证。
- **D-RETURN-01（安全回跳）**：邮箱验证与密码恢复绑定 Client App，只使用预登记回跳目标；不接受任意 URL。
- **D-CLIENT-01（禁用联动）**：Client App 禁用后拒绝其新身份流程并吊销其浏览器 token 家族，不影响其他 Client App。
- **D-RESP-01（责任边界）**：token 进入前端后，XSS 防护与 token 存储策略由集成方前端负责；Herald 通过权限上限、短时效 access token、旋转 refresh token、复用检测和吊销限制爆炸半径。refresh token 的浏览器存储、并发和失败恢复契约由后续集成文档承接。
- **D-SCOPE-03（不交付官方 JS SDK）**：原决策「本轮不交付官方 JS SDK；集成方用标准 `fetch` + `Authorization: Bearer`」**已由 DEC-js-sdk-003 取代**。官方 JS 浏览器 SDK 已作为独立能力交付，封装认证生命周期子集，见 [js-sdk PRD](/docs/prd/integration/js-sdk.md)。

---

## 9. 参考资料

- 冲突承接 PRD：[docs/prd/auth/oauth.md](/docs/prd/auth/oauth.md)（§2.2 Token 撤销）
- 冲突承接用户故事：[docs/user-stories/auth/third-party-app.md](/docs/user-stories/auth/third-party-app.md)（US-TP-015/016）
- 本功能用户故事：[docs/user-stories/integration/custom-user-ui.md](/docs/user-stories/integration/custom-user-ui.md)
- 相关 PRD：[Client App](/docs/prd/integration/client-app.md)、[SDK](/docs/prd/integration/sdk.md)、[Users](/docs/prd/core/users.md)、[积分](/docs/prd/billing/points.md)、[积分账户](/docs/prd/billing/credit-bucket.md)、[订阅](/docs/prd/billing/subscription.md)、[发票](/docs/prd/billing/invoice.md)、[TOTP](/docs/prd/auth/totp.md)、[Passkey](/docs/prd/auth/passkey.md)、[White-label](/docs/prd/core/ui-custom.md)
