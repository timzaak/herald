# JS 浏览器 SDK（第三方网页集成） 产品需求文档 (PRD)

**创建时间**: 2026-08-12
**优先级**: P0
**所属域**: integration

---

## 1. 相关用户故事

> 详细故事与验收标准请查看 [docs/user-stories/integration/js-sdk.md](/docs/user-stories/integration/js-sdk.md)（SDK 开发者体验维度）与 [docs/user-stories/integration/custom-user-ui.md](/docs/user-stories/integration/custom-user-ui.md)（业务能力基线）。

| US-ID | 标题 | 优先级 | 来源 |
|-------|------|--------|------|
| US-JS-001 | 初始化与跨域接入 | P0 | `docs/user-stories/integration/js-sdk.md` |
| US-JS-002 | 注册与邮箱验证（业务能力引用 US-CUI-001） | P0 | `docs/user-stories/integration/js-sdk.md` |
| US-JS-003 | 找回与重置密码（业务能力引用 US-CUI-003） | P0 | `docs/user-stories/integration/js-sdk.md` |
| US-JS-004 | 登录与多因素编排（业务能力引用 US-CUI-002） | P0 | `docs/user-stories/integration/js-sdk.md` |
| US-JS-005 | 自动静默刷新 | P0 | `docs/user-stories/integration/js-sdk.md` |
| US-JS-006 | 会话状态与登出（业务能力引用 US-CUI-006） | P1 | `docs/user-stories/integration/js-sdk.md` |
| US-JS-007 | 可配置凭证存储 | P1 | `docs/user-stories/integration/js-sdk.md` |
| US-JS-008 | 可区分的错误反馈 | P1 | `docs/user-stories/integration/js-sdk.md` |

业务能力基线（`docs/user-stories/integration/custom-user-ui.md`，已发布）：本 SDK 只覆盖其中的**认证生命周期子集**（注册/邮箱验证/登录+二因素/无密码邮箱验证码登录/找回重置/登出/状态），不覆盖个人中心其余能力（资料、积分、计费、发票、订阅、高危操作）；相关故事 US-CUI-001/002/003/006 为本 SDK 背后的业务验收来源，本 PRD 不复制其验收文本。

---

## 2. 范围界定

### 2.1 包含功能

面向第三方网页的**官方浏览器 SDK**，封装 Herald 认证生命周期，作为独立可发布包交付：

- **初始化与跨域接入**：以所属 Realm 与 Client App 上下文初始化，获得可用客户端；来源未预登记时跨域失败，按网络错误处理并提示检查来源配置。
- **注册与邮箱验证**：封装注册与邮箱验证，验证结果只回跳到 Client App 预登记页面。
- **找回/重置密码**：封装找回密码发起；新密码提交由 Herald 托管重置页承接（SDK 不暴露重置提交方法），重置成功后回跳到 Client App 预登记页面。
- **登录与多因素编排**：封装密码登录及其二因素（TOTP / Passkey）分支编排；提供无密码邮箱验证码登录流程；提供 LDAP 目录登录（`loginWithLdap`，走后端既有 LDAP 登录端点，结果分支编排与密码登录一致，仅 Realm 配置了 LDAP 时可用）；登录可能返回需要同意协议的中间状态。返回登录会话。
- **自动静默刷新**：访问凭证过期时用刷新凭证静默换发并重放原请求；并发合并为单次刷新；防刷新死循环；刷新失效或整族被吊销时清会话并引导重登。
- **会话状态与登出**：查询当前登录状态；登出终止当前会话及其刷新凭证族。
- **可配置凭证存储**：访问凭证仅内存持有；刷新凭证经可插拔存储接口管理，提供安全默认值与仅内存选项；非浏览器/SSR 环境提供安全守卫。
- **可区分的错误反馈**：对网络、鉴权、需要二因素、需要同意协议、需要重登等返回类型化、可编程判别的错误；浏览器 CORS 拒绝无法与网络故障可靠区分。
- **框架无关核心**：零前端框架依赖，React / Vue / 原生页面均可消费（本轮不交付框架适配层）。
- **零运行时依赖**：SDK 产物仅基于原生浏览器能力，不引入运行时第三方包。

### 2.2 不包含功能 (Out of Scope)

- **Node 服务端 SDK**：镜像 Rust SDK（`X-API-Key` + 服务端资源管理）的 `herald-sdk` 已交付于 `sdk/node/`（DEC-js-sdk-016），不属于本浏览器 SDK 的接口范围。
- **框架适配层**：React / Vue 等专用适配层本轮不交付，作为后续可选项（DEC-js-sdk-002）。
- **个人中心其余能力**：资料查看/昵称编辑、积分与交易、充值/购买、发票/订阅等不在本轮 SDK 范围；这些能力仍由 [docs/prd/integration/custom-user-ui.md](/docs/prd/integration/custom-user-ui.md) 承接，集成方可按需自行调用。
- **高危操作与统一重新认证辅助**：改密码、绑定/移除认证器、注销账号及其重新认证编排不在本轮 SDK 范围（DEC-js-sdk-001 认证生命周期范围内未列入）。
- **OAuth/PKCE 换取 FirstParty 凭证**：浏览器 SDK 只封装 `/login` 直签的 `CustomUserUi` 凭证类别；不经 PKCE 换取 `FirstParty`，后者仅 Herald 自有前端使用（DEC-js-sdk-008）。
- **OIDC / 标准化协议接入**：不含 discovery / JWKS / userinfo / id_token（承接 `.ai/decision/openid-connect.md` 的 Park 决策）。
- **后端改动**：浏览器 Bearer 模型、刷新、CORS 均为现有能力，SDK 无需任何后端改动。
- **改动现有 `frontend/` 应用**：SDK 为独立新增包，与自有前端解耦。

> 说明：[docs/prd/integration/custom-user-ui.md](/docs/prd/integration/custom-user-ui.md) 原 D-SCOPE-03「不交付官方 JS SDK」已被 DEC-js-sdk-003 取代，本 PRD 即取代后的官方 JS 浏览器 SDK。

### 2.3 依赖项

- **后端契约（现有，SDK 直接消费）**：浏览器 Bearer token 模型（登录直签、access/refresh 轮换、复用检测吊销家族、绝对有效上限、吊销）、刷新端点、登录/注册/邮箱验证/找回密码/二因素/无密码邮箱验证码登录/登出/状态端点、per-Client App 动态 CORS、Passkey RP 隔离。
- **凭证类别边界**：依赖后端 `CustomUserUi` 凭证类与用户自服务权限上限（`FirstParty` 不可由第三方获取）。
- **Client App 配置**：依赖集成方在 Client App 中预登记允许来源与身份流程回跳目标。
- **OpenAPI 契约同步**：依赖后端 OpenAPI 导出能力，SDK 的 HTTP 类型层与之同源。
- **既有参考实现**：Herald 自有前端的单飞刷新与「内存 access / 可持久化 refresh」存储模式作为 SDK 行为参考（非运行时依赖）。

---

## 4. 业务规则与状态

### 4.1 业务规则

- **凭证类别固定**：浏览器 SDK 只封装 `/login` 直签的 `CustomUserUi` 凭证；不经 PKCE 换取 `FirstParty`。凭证类由服务端 fail-closed 判定，SDK 不声明、不提升凭证类。
- **用户绑定与数据边界**：SDK 持有的 token 绑定单一登录用户；只能访问当前用户自己的数据，跨用户访问由服务端拒绝。
- **权限上限继承**：`CustomUserUi` token 只获得用户自服务权限上限内的能力；管理员能力与未知能力默认拒绝。该上限由服务端授权层执行，不依赖路径，SDK 不绕过。
- **access token 仅内存**：访问凭证永不持久化，仅在内存持有；刷新凭证经可插拔存储管理。
- **刷新语义**：访问凭证过期 → 用刷新凭证静默换发并单次重放原请求；并发请求合并为单次刷新（单飞）；内置防循环；刷新失败（过期/到绝对上限/被吊销）或检测到刷新凭证复用（整族被吊销）→ 清会话并发出重登信号。
- **登录分支语义**：密码登录成功可能直接登录、需要二因素（仅 TOTP 与 Passkey 进入二因素分支）、需要同意协议或触发 OAuth 跳转，SDK 按稳定分支返回可编程判别的登录结果。邮箱验证码登录是无密码的第一因素登录流程，独立于密码登录，不作为密码登录的二因素。LDAP 登录与密码登录共用同一套结果分支语义（凭据换为目录账号的用户名/密码，由服务端 search-then-bind 校验）。
- **来源/CORS 边界**：SDK 不绕过 CORS；当前网页来源必须已在对应 Client App 的允许来源中预登记，否则跨域失败并按网络错误处理。
- **安全回跳继承**：邮箱验证与密码重置只回跳到 Client App 预登记目标；该约束由服务端强制，SDK 如实呈现结果，不接受任意外部回跳地址。
- **Passkey RP 隔离继承**：Passkey 凭证按当前 Client App 来源对应 RP 隔离；来自其他来源或 Herald 原 RP 的凭证不可见、不可用。
- **Client App 禁用联动**：Client App 被禁用后，其浏览器 token 家族联动失效，SDK 会话随之失效。
- **框架无关与零运行时依赖**：SDK 核心不绑定特定前端框架；产物仅基于原生浏览器能力。

### 4.2 关键状态与异常

- **来源未在允许名单** → 跨域请求失败，浏览器会屏蔽 CORS 响应，SDK 通常只能返回 `network` 错误；开发者应检查 Client App 来源登记与浏览器控制台。
- **网络错误 vs 鉴权错误** → SDK 返回可区分错误类型，便于开发者分别处理。
- **登录需要二因素** → SDK 返回需要完成对应二因素挑战（TOTP 或 Passkey）的中间状态，可继续完成。
- **登录需要同意协议** → SDK 返回需要同意协议的中间状态及待同意协议清单；集成方完成协议交互后携带协议同意标识重新调用登录以通过。
- **登录失败（密码错误/限流/人机验证失败）** → 返回可区分错误，便于展示对应提示。
- **访问凭证过期、刷新凭证有效** → 静默刷新并单次重放，用户无感知。
- **并发请求同时过期** → 只发起一次刷新，各请求共享结果并重放。
- **刷新凭证过期/到绝对上限/被吊销** → 无法继续刷新，清会话并引导重登。
- **刷新凭证被复用（整族被吊销）** → 后续刷新全部失败，清会话并引导重登。
- **重放后仍被判需刷新** → 按防循环策略终止，不无限刷新。
- **非浏览器/SSR 环境且未注入存储适配器** → 默认存储不可用，SDK 明确报错要求注入适配器，不静默误用浏览器存储。
- **Client App 被禁用** → 会话失效，后续认证请求处于未登录状态。

---

## 5. 验收目标

- 集成方用 SDK 可完成：初始化 → 注册 → 邮箱验证 → 登录（含二因素与无密码邮箱验证码登录）→ 业务请求 → 自动刷新 → 登出的完整认证生命周期。
- 密码登录按结果分支呈现：直接成功 / 需要二因素（仅 TOTP、Passkey）/ 需要同意协议 / OAuth 跳转；需要同意协议时集成方可携带协议同意标识完成登录。
- 访问凭证过期时静默刷新并单次重放原请求；多个并发请求同时过期时只发起一次刷新。
- 刷新凭证被复用（整族被吊销）、过期或到达绝对上限时，SDK 清除会话并发出可被开发者捕获的重登信号。
- 来源未在 Client App 允许名单时，SDK 返回网络错误；不能从浏览器屏蔽的响应中可靠判别「来源未授权」。
- 邮箱验证与密码重置只回跳到 Client App 预登记页面（服务端强制，SDK 如实呈现）。
- Passkey 凭证按当前来源 RP 隔离，来自其他来源或 Herald 原 RP 的凭证不可见、不可用。
- 访问凭证永不持久化（仅内存）；刷新凭证默认浏览器存储可在页面刷新后恢复会话，也可切到仅内存或自定义存储。
- 非浏览器/SSR 环境未注入存储适配器时，SDK 明确报错而非静默误用浏览器存储。
- SDK 在 React、Vue、原生页面均可消费，不强制绑定框架；产物零运行时第三方依赖。
- 各类异常以可编程判别的类型化错误暴露，开发者可按稳定错误类别分支处理。

---

## 6. 边界与约束

**适用性**: 适用（SDK 消费既有后端契约；端点清单、参数 schema、状态码矩阵不在 PRD 承载范围，下沉到技术设计）

**API / 集成边界:**
- **契约同步**：SDK 的 HTTP 类型层与后端 OpenAPI 导出同源，避免手写契约漂移；具体生成与构建方案下沉到技术设计。

**前端 / 交互边界:**
- **页面入口**：SDK 以独立可发布包形式提供（npm 包名 `herald-auth-web`，无 scope——`@herald` 不可用，npm 名 `herald` 已被第三方占用，Q-js-sdk-002 已裁决，见决策账本 DEC-js-sdk-015）。集成方在自家网页安装并初始化后使用，Herald 不托管集成方页面。
- **关键交互**：初始化配置（Realm/Client App 上下文、可选存储适配器）→ 调用认证生命周期方法（注册/邮箱验证/找回密码/密码登录+多因素/无密码邮箱验证码登录/状态/登出）→ 业务请求由 SDK 自动注入凭证与静默刷新。
- **状态反馈**：会话状态变化（登录/刷新失败/整族吊销/登出）以可订阅的会话事件暴露；各类异常以可编程判别的类型化错误暴露。
- **权限/边界可见性**：需要二因素、需要同意协议、需要重登等情形返回可区分的错误类别；来源未授权导致的 CORS 拒绝通常归入网络错误，Client App 被禁用可能表现为会话失效，便于开发者给出准确提示与跳转。
- **SSR/非浏览器守卫**：无浏览器窗口且未注入存储适配器时，SDK 明确报错并提示注入适配器，不静默误用浏览器存储。

> 框架适配层、产物体积与 tree-shaking、构建工具链等技术细节不在 PRD 承载范围，下沉到技术设计。

---

## 7. 已确认决策

| Decision ID | 状态 | 决策项 | 结论 | PRD 落点 | 来源 |
|---|---|---|---|---|---|
| `DEC-js-sdk-001` | Applied | scope.browser-primary | 本轮只交付面向第三方网页的浏览器 SDK（注册/邮箱验证/登录含 2FA/passkey/找回重置/自动刷新/登出/状态）；Node 服务端 SDK 已由 DEC-js-sdk-016 独立交付于 `sdk/node/` | §2.1 / §2.2 / §5 | `docs/decisions/js-sdk.md` |
| `DEC-js-sdk-002` | Applied | framework.agnostic-core | 本轮只交付框架无关纯 TS 核心；React/Vue 适配层为后续可选项 | §2.1 / §2.2 / §6 | `docs/decisions/js-sdk.md` |
| `DEC-js-sdk-003` | Applied | supersedes.custom-user-ui-d-scope-03 | 本轮交付官方 JS 浏览器 SDK，取代 `docs/prd/integration/custom-user-ui.md` D-SCOPE-03「不交付官方 JS SDK」的表述 | §2.1 / §2.2 | `docs/decisions/js-sdk.md` |
| `DEC-js-sdk-004` | Applied | transport.openapi-generated | SDK HTTP 层复用后端 OpenAPI 生成管线，fetch 类型化客户端；运行时零依赖 | §2.3 / §6 | `docs/decisions/js-sdk.md` |
| `DEC-js-sdk-005` | Superseded | packaging.location-and-build | ~~新建仓库顶层独立包 `sdk-web/`~~ → 目录布局由 `DEC-js-sdk-017` 取代（`sdk/web`）；tsup 构建/ES2020+ 部分仍有效 | §2.1 / §6 | `docs/decisions/js-sdk.md` |
| `DEC-js-sdk-006` | Applied | token.storage-strategy | access token 仅内存；refresh token 经可插拔 `TokenStorage` 管理，默认浏览器存储；提供 SSR 安全守卫 | §2.1 / §4.1 / §4.2 / §5 / §6 | `docs/decisions/js-sdk.md` |
| `DEC-js-sdk-007` | Applied | refresh.semantics | 单飞刷新 + 单次重放 + 防循环 header + 失败清会话发事件 | §4.1 / §4.2 | `docs/decisions/js-sdk.md` |
| `DEC-js-sdk-008` | Applied | scope.credential-class | 浏览器 SDK 面向 `CustomUserUi`，不经 PKCE 换 `FirstParty` | §2.2 / §4.1 / §6 | `docs/decisions/js-sdk.md` |
| `DEC-js-sdk-010` | Applied | api.login-surface-and-email-otp | 密码登录返回判别分支（成功 / 需二因素（仅 totp、passkey）/ 需同意协议 / OAuth 跳转）；邮箱验证码登录是独立的无密码第一因素流程，非密码登录二因素；登录可携带协议同意标识以通过 consent 门 | §4.1 / §4.2 | `docs/decisions/js-sdk.md` |

> 本表只记录带稳定 DEC ID、且影响产品语义的已确认结论。其余实现级决策（OpenAPI 注解修正 DEC-js-sdk-011、浏览器产物打包格式 DEC-js-sdk-012、最终 npm 命名 DEC-js-sdk-015、Node SDK 交付 DEC-js-sdk-016、SDK 目录统一 `sdk/{web,node,rust}` DEC-js-sdk-017 等）保留在 `docs/decisions/js-sdk.md`，不进 PRD。原延期问题 Q-js-sdk-002（最终 npm scope 与是否本轮发布）已裁决：`herald-auth-web`（浏览器，`sdk/web/`）+ `herald-sdk`（Node 服务端，`sdk/node/`，Rust crate `sdk/rust/` 同名对应物），均无 scope。

---

## 8. 参考资料

- 用户故事来源见 §1 表格
- 决策账本：`docs/decisions/js-sdk.md`
