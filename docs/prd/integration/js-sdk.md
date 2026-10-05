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

> 说明：本 PRD 即官方 JS 浏览器 SDK（DEC-js-sdk-003）；[docs/prd/integration/custom-user-ui.md](/docs/prd/integration/custom-user-ui.md) D-SCOPE-03 的「不交付官方 JS SDK」不约束本能力。

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
- **页面入口**：SDK 以独立可发布包形式提供（npm 包名 `herald-auth-web`，无 scope——`@herald` 不可用，npm 名 `herald` 已被第三方占用，见 §7 DEC-js-sdk-015）。集成方在自家网页安装并初始化后使用，Herald 不托管集成方页面。
- **关键交互**：初始化配置（Realm/Client App 上下文、可选存储适配器）→ 调用认证生命周期方法（注册/邮箱验证/找回密码/密码登录+多因素/无密码邮箱验证码登录/状态/登出）→ 业务请求由 SDK 自动注入凭证与静默刷新。
- **状态反馈**：会话状态变化（登录/刷新失败/整族吊销/登出）以可订阅的会话事件暴露；各类异常以可编程判别的类型化错误暴露。
- **权限/边界可见性**：需要二因素、需要同意协议、需要重登等情形返回可区分的错误类别；来源未授权导致的 CORS 拒绝通常归入网络错误，Client App 被禁用可能表现为会话失效，便于开发者给出准确提示与跳转。
- **SSR/非浏览器守卫**：无浏览器窗口且未注入存储适配器时，SDK 明确报错并提示注入适配器，不静默误用浏览器存储。

> 框架适配层、产物体积与 tree-shaking、构建工具链等技术细节不在 PRD 承载范围，下沉到技术设计。

---

## 7. 已确认决策

> 本节只收录当前有效的决策与未决问题，记取舍、理由、决策人与重开条件；规则正文只在 §4 定义，DEC/Q 编号保持稳定，供代码注释、测试与跨 PRD 引用追溯。实现级决策（打包、npm 命名、目录布局、OpenAPI 注解、SDK 公开 API 形态等）同样收录于本节，标注「实现层」。

- **DEC-js-sdk-001 · scope.browser-primary**（agent：AskUserQuestion 追问交付范围，用户跳过，按最简且符合用户主句措辞方向裁决）：本轮只交付面向第三方网页的浏览器 SDK（注册/邮箱验证/登录含 2FA/passkey/找回重置/自动刷新/登出/状态）；Node 服务端 SDK 已由 DEC-js-sdk-016 独立交付于 `sdk/node/`。理由：用户原始需求主句明确为 web SDK（登录/token 自动刷新/注册 供第三方网页集成）；“服务器端 sdk 可参考 rust sdk” 为许可性表述（“可以”）；`/api/ext/*` 服务端资源管理已由现有 Rust SDK 覆盖（`backend/sdk/src/lib.rs`）；Rule 2 最小范围 + Rule 4 收敛。落点：§2.1 / §2.2 / §5。重开条件：用户明确要求本轮同时交付 Node 服务端 SDK。
- **DEC-js-sdk-002 · framework.agnostic-core**（agent：AskUserQuestion 追问框架绑定，用户跳过，按最小范围裁决）：本轮只交付框架无关纯 TS 核心；React/Vue 适配层为后续可选项。理由：“供第三方网页集成”未指定框架；核心层最大化兼容；Rule 2 最小范围。落点：§2.1 / §2.2 / §6。重开条件：用户要求本轮即提供特定框架（React/Vue）适配层。
- **DEC-js-sdk-003 · supersedes.custom-user-ui-d-scope-03**（user）：交付官方 JS 浏览器 SDK；`docs/prd/integration/custom-user-ui.md` D-SCOPE-03 的「不交付官方 JS SDK」不约束本能力。理由：用户本次明确要求“提供 web sdk”；后端浏览器 Bearer 模型已完整实现（`backend/api-auth/src/login.rs` 直签 token、`browser_token.rs` 轮换刷新、`identity_middleware.rs` Bearer-only、CORS 按 client-app origin 动态放行 `allow_credentials:false`），具备 SDK 封装条件。落点：§2.1 / §2.2。重开条件：用户撤回 web SDK 需求。
- **DEC-js-sdk-004 · transport.openapi-generated**（agent）：SDK HTTP 层复用后端 OpenAPI 生成管线，fetch 类型化客户端；运行时零依赖。理由：仓库前端已验证此生成路径（`frontend/openapi-ts.config.ts` + `frontend/src/lib/api-generated/`）；手写会与后端契约漂移；生成物经 tree-shaking/精选导出控制体积。落点：§2.3 / §6。重开条件：后端停止导出 OpenAPI 或 `@hey-api/openapi-ts` 不可用。
- **DEC-js-sdk-005 · packaging.location-and-build**（agent）：浏览器 SDK 包位于 `sdk/web`（目录布局统一见 `DEC-js-sdk-017`）；tsup 构建、ES2020+ 目标。理由：仓库当前无 monorepo workspace；Rust SDK 独立 crate 是先例；tsup 是 SDK 库主流最小构建方案；零运行时依赖。落点：§2.1 / §6。重开条件：仓库引入 monorepo workspace，或用户要求并入 frontend。
- **DEC-js-sdk-006 · token.storage-strategy**（agent）：access token 仅内存；refresh token 经可插拔 `TokenStorage` 管理，默认浏览器存储；提供 SSR 安全守卫（规则正文见 §4.1）。理由：复用前端已验证模式（`frontend/src/stores/auth-store.ts` 内存 AT + Zustand persist RT）；服务端轮换 + 复用检测 + 绝对 TTL 已使 localStorage RT 风险可接受（承接 `custom-user-ui` R-FIRSTPARTY-TOKEN / R-FT-PERSIST 的既定风险接受）；可插拔存储支持非浏览器/SSR。落点：§2.1 / §4.1 / §4.2 / §5 / §6。重开条件：用户要求 refresh token 默认不持久化（仅内存/会话级）或改用 sessionStorage。
- **DEC-js-sdk-007 · refresh.semantics**（agent）：刷新语义为单飞 + 轮换 + 原请求单次重放 + 防循环 header，失败清会话并发出 session 事件（规则正文见 §4.1）。理由：复用前端 `frontend/src/lib/api-client.ts` 已验证的单飞/重放逻辑；服务端复用检测兜底（`backend/infra/src/authentication/mod.rs` Lua 脚本）。落点：§4.1 / §4.2。重开条件：无（实现细节，可直接进设计）。
- **DEC-js-sdk-008 · scope.credential-class**（repository-fact + agent）：浏览器 SDK 面向 `CustomUserUi`，不经 PKCE 换 `FirstParty`（规则正文见 §4.1）。理由：后端权限模型：`/login` 直签固定 `CustomUserUi`，受 self-service scope 上限约束（`backend/domain/src/authentication/identity.rs` `CredentialClass`）；FirstParty 仅内置 `admin-web-console` 经 PKCE 换取，第三方无法获取（`backend/api-oauth/src/token.rs` fail-closed）。落点：§2.2 / §4.1 / §6。重开条件：后端开放第三方 FirstParty 获取路径。
- **DEC-js-sdk-010 · api.login-surface-and-email-otp**（agent）：密码登录返回判别分支（成功 / 需二因素（仅 totp、passkey）/ 需同意协议 / OAuth 跳转）；邮箱验证码登录是独立的无密码第一因素流程，非密码登录二因素；登录可携带协议同意标识以通过 consent 门（规则正文见 §4.1）。理由：后端运行时事实：`POST /login` 成功为多分支 200（`backend/api-auth/src/login.rs:394-459` 的 `LoginResponse` 含 `secondFactors`/`consentRequired`/`redirectTo`，仅 totp/passkey 进 secondFactors）；email-otp 是独立无密码登录（`/login/email-otp/send`+`/login/email-otp/verify`），非二因素；consent 是登录返回的子状态。产品意图（覆盖全部登录分支含 email-otp）不变，仅按后端事实对齐 SDK 公开 API 形态。落点：§4.1 / §4.2。重开条件：后端将 email-otp 改为密码登录二因素，或移除 consent 门。
- **DEC-js-sdk-011 · transport.openapi-annotation-corrected**（user 指示修复 + agent 执行与验证，实现层）：后端 auth 登录类端点的 utoipa 200-body 注解已在源头修正为 `BrowserTokenResponse`：`/login`（`login.rs:111`）、`/login/verify-totp`（`verify_totp.rs:136`）、`/login/passkey/verify`（`verify_passkey.rs:230`）、`/login/passkey/2fa/verify`（`verify_passkey.rs:374`），与既有的 `/login/email-otp/verify`（`email_otp.rs:411`）一致；SDK 直接使用 OpenAPI 生成类型，无需客户端包装层覆盖。理由：用户指示在源头修复 stale 注解（优于客户端覆盖）；已验证 `cargo check -p herald-api-auth` 通过，且 `herald-app --export-openapi` 输出中四个端点 200 schema 现均引用 `BrowserTokenResponse`；source-of-truth 修复惠及全部 OpenAPI 消费方（自有前端 + SDK + 未来客户端）。落点：设计/实现层。重开条件：后端回退这些注解为挑战类型（LoginResponse/VerifyTotpResponse/PasskeyVerifyResponse）。
- **DEC-js-sdk-012 · packaging.browser-format**（agent：打包方案经 AskUserQuestion 提问未获答复，按最佳工程判断执行，pending 用户追认，实现层）：浏览器 SDK 产物为 **ESM（npm 主产物）+ 压缩 IIFE bundle（`dist/index.global.js`，暴露 `Herald` 全局，供 CDN `<script>`）**，**不发 CJS**（`package.json` 仅 `exports.import` + `unpkg`/`jsdelivr` 指向 IIFE）。理由：浏览器 SDK；现代工具链（Vite/webpack/Next/esbuild）ESM 原生；CJS 仅服务 Node `require`——属独立的服务端 SDK 领域，发 CJS 会邀请错误用法并模糊浏览器/服务端边界；第三方网页集成常见 CDN `<script>` 一行接入需求；SDK 无模块级可变状态（全 per-instance `createClient`），无 ESM/CJS 双副本状态分裂风险。落点：设计/实现层。重开条件：出现确认需要 CommonJS 互操作的消费者，或用户明确偏好双格式。
- **DEC-js-sdk-013 · api.first-party-token-bridge**（agent：AskUserQuestion 追问迁移策略未获答复，按推荐方案裁决，实现层）：SDK 新增**第一方令牌桥接 API**（纯增量）：`tokens.getAccessToken()/setTokens({accessToken, refreshToken, clientId?})/clear()/bindClientId()`（`setTokens` 可重绑请求体 clientId，均不发 session 事件）、公开单飞 `refresh()`（与 401 拦截器共享 in-flight promise）、`login` 载荷可选透传 `oauthClientId/redirectUri/state` 与 `passkey.loginBegin` 可选透传 `oauth` 对象（后端本就接受；SDK 不做 code 交换，命中时返回 `oauth-redirect` 分支）、consent 结果在每个 agreement 上附 `raw` 原始摘要（snake_case 展示字段）供宿主渲染。理由：Herald 自有前端改为消费 SDK 作为统一令牌引擎，消除与 `frontend/src/lib/api-client.ts` 重复的 Bearer 注入 + 单飞刷新实现；桥接为纯状态注入/读取，不改变第三方 SDK 范围，不违反 DEC-js-sdk-008（PKCE 交换仍在调用方）与 DEC-js-sdk-001（高危操作仍不入 SDK）；`raw` 透传是因为前端 consent UI 渲染后端原始 snake_case 摘要（title/version_no/effective_at），SDK 规约的 `{agreementType, versionId}` 会丢展示字段。落点：设计/实现层。重开条件：用户要求 SDK 内置 PKCE 交换、switch-client 等第一方专属操作。
- **DEC-js-sdk-014 · api.email-otp-send-conflict-branch**（user 评审 DEC-js-sdk-013 迁移偏离项时裁决 + agent 实现，实现层）：`loginWithEmailOtp.send` 返回判别联合 `EmailOtpSendResult`（`sent`｜`conflict`），且 `EmailOtpSendPayload` 增加可选 `agreements`：409 的 `consent_required` / `email_not_registered` 两个控制流结果**不抛错**，作为 `conflict` 分支返回（含 `consentRequired`、带 `raw` 摘要透传的 `agreements`）；其余 409/4xx/5xx 照常抛 `HeraldError`。理由：email-otp send 的 409 是产品语义上的流程分支（auto-register 同意门 / 未注册引导）而非错误，与 DEC-js-sdk-010 对 login 多分支 200 的判别化处理同一哲学；此前实现把 409 体当错误抛出且 `HeraldError` 丢失顶层 `consentRequired`/`agreements` 字段，导致第一方前端无法消费 SDK 的 send（迁移评审中用户裁决：web 端确实用到，应写入 SDK）。落点：设计/实现层。重开条件：后端改变 email-otp 409 契约或新增冲突码。
- **DEC-js-sdk-015 · release.npm-naming**（user：AskUserQuestion 三方案中选定 herald-auth-web + herald-sdk，实现层）：最终 npm 命名：浏览器包 **`herald-auth-web`**（`sdk/web`），Node 服务端包 **`herald-sdk`**（`sdk/node`，与 Rust crate 同名同定位，crates.io/npm 跨 registry 对称）；两者均**无 scope**，以个人账号 timzaak 发布。理由：`@herald` scope 永不可用：npm 用户名/org/包名共用命名空间，`herald` 包名已被第三方占用（maintainer venticco）；用户确认无 herald org、仅能以个人账号发布；候选名经 registry 查询确认全部可用（2026-08-24）。落点：设计/实现层。重开条件：用户获得可用 org scope 并要求迁移（npm 不支持包改名，只能发新包弃旧包）。
- **DEC-js-sdk-016 · packaging.node-sdk**（user：AskUserQuestion 选定「一起做」，实现层）：交付 Node 服务端 SDK：`sdk/node/`（npm `herald-sdk`）**1:1 移植 Rust `backend/sdk`**——`HeraldClient(baseUrl, apiKey, cacheTtlSeconds?)` + `X-API-Key` + `/api/ext/*` 全部方法（权限检查/订阅/积分余额·消费·授予/realm·user·client-app CRUD）+ 权限缓存三件套（per-request TTL 缓存、token 索引失效、300s token 过期启发式）；HTTP 层**手写 fetch、不用 OpenAPI 生成**（与 Rust crate 手写 reqwest 对称，构建无需 Rust 工具链）；产物 **ESM+CJS 双格式**（承接 DEC-js-sdk-012 浏览器/服务端分工），Node ≥18，零运行时依赖。理由：Rust crate 是本 SDK 的 source of truth，手写保持类型与其同步且零依赖；Node 消费者需要 `require` 互操作；发布 workflow（`publish-node-sdk.yml`）因此无需 Rust 步骤，比 web 包轻。落点：设计/实现层。重开条件：后端 `/api/ext/*` 契约变更需同步维护 Rust 与 TS 两处手写类型（漂移风险已接受，与 Rust crate 同等），或决定改用 OpenAPI 生成统一。
- **DEC-js-sdk-017 · packaging.sdk-directory-layout**（user：AskUserQuestion 选定「三个都移」，实现层）：三个 SDK 统一收入 `sdk/` 目录：`sdk/web/`（npm `herald-auth-web`）、`sdk/node/`（npm `herald-sdk`）、`sdk/rust/`（crate `herald-sdk`，自 `backend/sdk/` 迁出）。Rust SDK 因此脱离 backend Cargo workspace 成为独立 crate（Cargo 要求 workspace 成员 hierarchically below root，实验验证报错），依赖版本与 crate 版本改为在 `sdk/rust/Cargo.toml` 手动同步（与 backend `[workspace.dependencies]` 对齐，文件内注释标明）；CI 覆盖补偿：`ci.yml` backend-ci 增挂 sdk/rust fmt+clippy、`backend-test.yml` 增 nextest 步骤、`scripts/push.py` backend area 增 sdk/rust 三件套。理由：用户要求统一 SDK 目录；独立 crate 换取三 SDK 目录对称与独立可发布性；版本漂移代价与 JS SDK 手动同步同等，且 release 脚本已有同步先例。落点：设计/实现层。重开条件：仓库引入 monorepo workspace 或统一的 Rust 依赖管理方案。


---

## 8. 参考资料

- 用户故事来源见 §1 表格
