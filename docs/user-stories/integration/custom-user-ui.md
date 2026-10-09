# 自建用户 UI（Custom User UI） 用户故事

> 角色定义见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)

**故事范围**: US-CUI-001 ~ US-CUI-009

**背景与范围承接**: 本组故事承接自建用户 UI（Custom User UI）PRD——集成方可在自家前端自建终端用户的全套 UI：未认证身份流程（注册、登录、找回/重置密码、邮箱验证）+ 登录后完整个人中心（资料、改密码、TOTP、Passkey、注销账号、登出、积分、充值、发票、订阅）。跨域身份解析使用 Bearer token 而非 cookie；集成方自建 UI 经 `/login` 获得 `CustomUserUi` 凭证类。

**与既有用户故事的覆盖关系**：本组故事表达"集成方前端可跨域触达"这些能力，**不复制既有用户故事的验收内容**。具体业务验收（如购买积分包、查看余额的具体行为）仍归属既有 billing/auth 用户故事；本组只验收"跨域自建 UI"这一集成维度的目标。

**安全姿态说明**：主路线使**用户 token 进入集成方前端浏览器**。OAuth 后端换码场景（US-TP-016）保持"token 不经过用户浏览器"原义；浏览器 token 是自建 UI 场景的独立路线，见各故事验收。

---

## 故事 1：集成方前端完成注册与邮箱验证 [US-CUI-001]

**优先级**: P0

**【用户故事】**
**作为**：第三方应用开发者（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：在自家前端（无自家后端）直接完成用户注册和邮箱验证流程
**从而**：不依赖 Herald 自有页面，自建完整注册体验

**【验收标准】**

**场景 1：集成方前端直接提交注册**
```gherkin
Given 集成方前端所属 Client App 已启用、配置允许 origin 与预登记验证结果页，且 Realm 已开启注册
When 集成方前端以该 Client App 上下文跨域提交注册（邮箱/昵称/密码 + 人机验证）
Then Herald 创建用户（按 Realm 配置决定是否需邮箱验证）
And 注册成功后不自动登录，用户需另行登录
```

**场景 2：需邮箱验证时用户能完成验证并看到正确结果**
```gherkin
Given Realm 配置要求邮箱验证
When 用户点击验证邮件中的链接
Then Herald 校验流程绑定的 Realm 与 Client App
And 用户被引导到该 Client App 预登记的验证结果页并看到验证成功结果
And 账户被激活
```

**场景 3：无需邮箱验证时注册后可立即登录**
```gherkin
Given Realm 配置不要求邮箱验证
When 用户完成注册
Then 账户立即激活，用户可前往集成方登录入口登录
```

**场景 4：注册被限流或人机验证失败（失败场景）**
```gherkin
Given 同一来源频繁提交注册
When 触发限流或人机验证不通过
Then Herald 拒绝注册请求并返回相应提示
```

**场景 5：任意回跳地址被拒绝（失败场景）**
```gherkin
Given 请求试图提供未在 Client App 中预登记的验证回跳地址
When 集成方前端提交注册或验证流程
Then Herald 拒绝该回跳地址
And 不向邮件写入请求提供的任意 URL
```

---

## 故事 2：集成方前端完成登录获得浏览器 token [US-CUI-002]

**优先级**: P0

**【用户故事】**
**作为**：第三方应用开发者（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：集成方前端直接用账号密码登录，并获得浏览器可持有的用户 token
**从而**：无自家后端也能在浏览器维持登录态、发起后续跨域调用

**【验收标准】**

**场景 1：用账号密码登录获得浏览器 token**
```gherkin
Given 用户已注册并激活
When 集成方前端跨域提交登录（账号 + 密码 + 人机验证）
Then Herald 校验通过，返回 CustomUserUi 浏览器 token（access token + refresh token，用户绑定）
And 身份解析不依赖 cookie，不向响应写入 Set-Cookie
```

**场景 2：需要二因素时返回临时凭证而非完成登录**
```gherkin
Given 用户已启用 TOTP 或 Passkey 二因素
When 集成方前端提交账号密码登录
Then Herald 返回需二因素验证的临时凭证，不直接签发最终 token
And 前端完成二因素验证后才获得浏览器 token
```

**场景 3：access token 到期时用 refresh token 静默换发**
```gherkin
Given 用户持有短时效 access token 和配套 refresh token
When access token 接近到期
Then 前端用 refresh token 换发新 access token
And Herald 同时换发新 refresh token，旧的 refresh token 作废
And 用户不感知这次刷新
```

**场景 4：refresh token 到达绝对上限后需重新登录**
```gherkin
Given refresh token 达到最大有效时长
When 前端尝试用它换发新 access token
Then Herald 拒绝刷新，前端引导用户重新登录
```

**场景 5：被作废的旧 refresh token 再次被使用，整个 token 家族被吊销（失败场景）**
```gherkin
Given 某个 refresh token 已在换发后作废
When 该旧 refresh token 再次被用于刷新
Then Herald 吊销该次登录签发的全部 refresh token
And 用户需重新登录
```

**场景 6：密码错误或人机验证失败（失败场景）**
```gherkin
Given 用户提交错误的密码或人机验证不通过
When 集成方前端提交登录
Then Herald 拒绝登录并返回相应提示
```

**场景 7：Client App 禁用后凭证失效（失败场景）**
```gherkin
Given Client App 已被禁用
When 前端尝试开始新的身份流程或继续使用该 Client App 签发的浏览器 token
Then Herald 拒绝请求并使该 Client App 的浏览器 token 家族失效
And 其他 Client App 的正常会话不受影响
```

---

## 故事 3：集成方前端完成找回/重置密码 [US-CUI-003]

**优先级**: P0

**【用户故事】**
**作为**：第三方应用开发者（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：在自家前端完成找回密码和重置密码流程
**从而**：用户不离开集成方前端即可恢复账号访问

**【验收标准】**

**场景 1：发起找回密码**
```gherkin
Given 用户忘记密码，且 Client App 已配置预登记密码重置页
When 集成方前端以该 Client App 上下文跨域提交找回密码（邮箱 + 人机验证）
Then Herald 发送重置邮件（为防邮箱枚举，无论邮箱是否存在都返回成功）
```

**场景 2：用户在集成方前端完成重置密码**
```gherkin
Given 用户从重置邮件获得重置凭证
When 集成方前端用该凭证提交新密码
Then Herald 校验凭证有效，更新密码
And 重置成功后不自动登录，用户需另行登录
```

**场景 3：重置凭证无效或已过期（失败场景）**
```gherkin
Given 重置凭证不存在或已过期
When 集成方前端提交新密码
Then Herald 拒绝重置并提示凭证无效
```

**场景 4：密码重置只回跳到预登记页面（边界场景）**
```gherkin
Given 密码重置流程绑定有效的 Realm 与 Client App
When 用户点击重置邮件中的链接
Then 用户只被引导到该 Client App 预登记的密码重置页
And 请求提供的任意外部回跳地址不生效
```

---

## 故事 4：集成方前端查看资料并修改昵称 [US-CUI-004]

**优先级**: P1

**【用户故事】**
**作为**：第三方应用开发者（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：登录后的用户能在集成方前端查看自己的资料并修改昵称
**从而**：在自建个人中心展示和维护用户基本信息

**【验收标准】**

**场景 1：用浏览器 token 查看自己的资料**
```gherkin
Given 用户已登录并持有 CustomUserUi 浏览器 token
When 集成方前端跨域携带 token 查看资料
Then Herald 返回当前登录用户的资料（邮箱、昵称、状态）
```

**场景 2：用浏览器 token 修改自己的昵称**
```gherkin
Given 用户已登录并持有 CustomUserUi 浏览器 token
When 集成方前端提交新的昵称
Then Herald 更新并返回当前登录用户的资料
```

**场景 3：未登录或 token 失效时访问资料被拒绝（失败场景）**
```gherkin
Given 请求未携带有效浏览器 token
When 集成方前端尝试查看资料
Then Herald 拒绝并提示未授权
```

> 备注：头像上传或编辑不在本故事范围。

---

## 故事 5：集成方前端完成高危安全操作（改密码 / 二因素 / 注销账号） [US-CUI-005]

**优先级**: P0

**【用户故事】**
**作为**：第三方应用开发者（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：登录后的用户能在集成方前端完成改密码、管理二因素认证（TOTP/Passkey）、注销账号
**从而**：自建个人中心的安全模块与 Herald 自有前端能力对齐

**【验收标准】**

**场景 1：用浏览器 token 修改密码**
```gherkin
Given 用户已登录并持有 CustomUserUi 浏览器 token
And 用户已通过账户绑定的密码、TOTP 或要求用户验证的 Passkey 完成针对改密码的重新认证
When 用户在集成方前端提交新密码并消费该重新认证结果
Then Herald 更新密码
And 该重新认证结果不可再次使用
```

**场景 2：用浏览器 token 启用/禁用 TOTP**
```gherkin
Given 用户已登录并持有 CustomUserUi 浏览器 token
And 用户已完成针对绑定或移除认证器的重新认证
When 用户在集成方前端发起 TOTP 启用（验证动态码）或禁用并消费重新认证结果
Then Herald 执行对应操作并返回结果（恢复码等）
```

**场景 3：用浏览器 token 注册/删除/重命名 Passkey**
```gherkin
Given 用户已登录并持有 CustomUserUi 浏览器 token，且当前 Client App origin 已获准
When 用户完成重新认证后发起 Passkey 注册或删除
Then Herald 以当前 origin 的 host 作为 RP ID 执行 WebAuthn 流程
And credential 只归属当前 RP
When 用户仅重命名自己的 Passkey
Then Herald 完成重命名且不额外要求重新认证
```

**场景 4：用浏览器 token 注销账号（不可逆）**
```gherkin
Given 用户已登录并持有 CustomUserUi 浏览器 token
And 用户已通过账户已绑定因子完成针对注销账号的重新认证
When 用户提交注销账号并消费该重新认证结果
Then Herald 执行注销（匿名化、取消订阅、清除会话），且操作不可恢复
```

**场景 5：高危操作缺少有效重新认证时被拒绝（失败场景）**
```gherkin
Given 重新认证结果缺失、过期、已消费或绑定到其他目标操作
When 集成方前端提交改密码、绑定/移除认证器或注销账号
Then Herald 拒绝操作并要求重新认证
```

**场景 6：浏览器凭证不能执行管理员能力（失败场景）**
```gherkin
Given CustomUserUi 浏览器 token 绑定的用户同时拥有管理员角色
When 该浏览器 token 请求管理员或未归类能力
Then Herald 根据浏览器凭证权限上限拒绝请求
And 拒绝结果不依赖请求 URL 的前缀
```

**场景 7：不同 RP 的 Passkey 不混用（边界场景）**
```gherkin
Given 用户在 Herald 原 RP 或另一个 Client App RP 注册过 Passkey
When 当前 Client App 发起 Passkey 登录、二因素或凭证管理
Then Herald 不返回也不使用其他 RP 的 credential
And 当前 RP 没有可用 credential 时允许回退到既有密码或 TOTP 流程
```

---

## 故事 6：集成方前端完成登出 [US-CUI-006]

**优先级**: P1

**【用户故事】**
**作为**：第三方应用开发者（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：用户能在集成方前端登出，吊销当前浏览器 token
**从而**：自建个人中心提供完整的会话结束能力

**【验收标准】**

**场景 1：用浏览器 token 登出并吊销**
```gherkin
Given 用户已登录并持有 CustomUserUi 浏览器 token
When 用户在集成方前端点击登出
Then Herald 吊销该 token（及其 refresh token 家族）
And 前端清除本地持有的 token
```

**场景 2：登出后该 token 不可再用（失败场景）**
```gherkin
Given 某 token 已通过登出吊销
When 集成方前端继续携带该 token 调用
Then Herald 拒绝并提示未授权
```

---

## 故事 7：集成方前端完成积分与交易查看 [US-CUI-007]

**优先级**: P0

**【用户故事】**
**作为**：第三方应用开发者（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：登录后的用户能在集成方前端查看积分余额和交易历史
**从而**：自建个人中心的积分模块

**【验收标准】**

**场景 1：用浏览器 token 查看自己的积分余额**
```gherkin
Given 用户已登录并持有 CustomUserUi 浏览器 token，且 Realm 开启积分可见
When 集成方前端跨域携带 token 查看余额
Then Herald 返回当前登录用户的积分余额（按账户分组）
```

**场景 2：用浏览器 token 查看自己的交易历史**
```gherkin
Given 用户已登录并持有 CustomUserUi 浏览器 token
When 集成方前端跨域携带 token 查看交易历史
Then Herald 返回当前登录用户的交易记录
```

**场景 3：用 token 访问他人积分被拒绝（失败场景）**
```gherkin
Given CustomUserUi 浏览器 token 绑定用户 A
When 该 token 被用于访问用户 B 的积分
Then Herald 拒绝，只能访问当前登录用户自己的数据
```

---

## 故事 8：集成方前端完成充值/购买 [US-CUI-008]

**优先级**: P0

**【用户故事】**
**作为**：第三方应用开发者（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：登录后的用户能在集成方前端查看套餐、发起充值/购买并轮询支付状态
**从而**：自建充值/购买模块，复用既有 billing 能力

**【验收标准】**

**场景 1：用浏览器 token 查看购买选项（套餐/价目）**
```gherkin
Given 用户已登录并持有 CustomUserUi 浏览器 token
When 集成方前端跨域携带 token 查看购买选项
Then Herald 返回可用套餐和价目
```

**场景 2：用浏览器 token 发起充值/购买**
```gherkin
Given 用户已登录并持有 CustomUserUi 浏览器 token
When 用户在集成方前端选择套餐并发起购买
Then Herald 创建支付尝试并返回支付流程入口
And 支付最终确认由支付提供商页面承接（Herald 浏览器 token 只负责发起，不直接完成支付）
```

**场景 3：用浏览器 token 轮询支付状态**
```gherkin
Given 用户已发起支付尝试
When 集成方前端跨域携带 token 轮询该尝试状态
Then Herald 返回当前支付状态
```

> 备注：具体购买/支付业务验收（如积分到账、订阅生效）仍归属既有 billing 用户故事（US-PU-006 等）；本故事只验收"集成方前端跨域发起与轮询"这一集成维度。

---

## 故事 9：集成方前端完成发票与订阅查看 [US-CUI-009]

**优先级**: P1

**【用户故事】**
**作为**：第三方应用开发者（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：登录后的用户能在集成方前端查看发票、申请开票、查看订阅
**从而**：自建个人中心的发票与订阅模块

**【验收标准】**

**场景 1：用浏览器 token 查看我的发票列表与详情**
```gherkin
Given 用户已登录并持有 CustomUserUi 浏览器 token，且 Realm 开启发票可见
When 集成方前端跨域携带 token 查看发票
Then Herald 返回当前登录用户的发票列表与详情
```

**场景 2：用浏览器 token 申请开票**
```gherkin
Given 用户已登录并持有 CustomUserUi 浏览器 token
When 用户在集成方前端对某笔交易申请开票
Then Herald 受理申请并返回处理状态
```

**场景 3：用浏览器 token 查看我的订阅**
```gherkin
Given 用户已登录并持有 CustomUserUi 浏览器 token，且 Realm 开启订阅可见
When 集成方前端跨域携带 token 查看订阅
Then Herald 返回当前登录用户的订阅信息
```

---

## 备注

### 安全姿态
- 跨域身份解析使用 Bearer token，不依赖 cookie。OAuth 后端换码场景（US-TP-016）保持"token 不经过用户浏览器"原义；本组故事描述的浏览器 token 是集成方自建 UI 场景的独立路线。

### 责任边界
- token 进入前端后，XSS 防护与 token 存储策略由集成方前端负责；Herald 通过 `CustomUserUi` 凭证权限上限、短时效 access token、旋转 refresh token、复用检测与吊销能力限制爆炸半径。CORS 不作为授权机制，高危操作必须重新认证。
- refresh token 的浏览器存储、多标签页并发和失败恢复契约由后续集成文档承接。

### 与既有故事的关系
- 本组故事不复制既有用户故事验收内容；具体业务行为验收仍归属既有 billing/auth 用户故事（US-PU-001/002/006、US-CB-005/006、US-TO-*、US-PK-*、US-RU-014 等）。

### 相关文档
- PRD：[docs/prd/integration/custom-user-ui.md](/docs/prd/integration/custom-user-ui.md)
- 冲突承接用户故事：[docs/user-stories/auth/third-party-app.md](/docs/user-stories/auth/third-party-app.md)（US-TP-015/016）
- 冲突承接 PRD：[docs/prd/auth/oauth.md](/docs/prd/auth/oauth.md)（§2.2 Token 撤销）
