# JS 浏览器 SDK（第三方网页集成）用户故事

> 角色定义参考 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)。
>
> 本组故事表达「集成方前端开发者通过官方浏览器 SDK 接入 Herald 认证生命周期」这一**开发者体验维度**。
> 故事只验收 SDK 作为接入工具的目标行为；背后的业务能力验收仍归属既有用户故事（ principally [docs/user-stories/integration/custom-user-ui.md](/docs/user-stories/integration/custom-user-ui.md) 的 US-CUI 组），本组在相关故事中显式引用，不复制其验收文本。

## 用户故事

### 故事 1：初始化与跨域接入 [US-JS-001]

**优先级**: P0

**【用户故事】**
**作为**：第三方应用开发者（集成方前端开发者，详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：安装并初始化官方浏览器 SDK，仅需提供所属 Realm 与 Client App 上下文即可获得可用客户端
**从而**：在我的网页（React / Vue / 原生页面均可）中快速接入 Herald 的认证能力，无需自建跨域与凭证底层

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：初始化成功**
```gherkin
Given 开发者已在其网页项目中安装 SDK，并已在 Client App 配置中预登记当前网页的站点来源
When 开发者使用所属 Realm 与 Client App 上下文初始化 SDK
Then SDK 返回一个可用的客户端实例，后续认证流程方法可被调用
```

**场景 2：来源未预登记时给出可区分错误**
```gherkin
Given 当前网页的站点来源未在对应 Client App 的允许来源中登记
When 开发者初始化 SDK 并发起一次认证请求
Then SDK 返回可与其他错误区分的「来源未授权」错误
And 错误信息明确引导开发者去 Client App 配置中登记来源
```

**场景 3：框架无关**
```gherkin
Given 开发者的网页基于 React、Vue 或原生 JS 中的任意一种
When 开发者按同一套初始化方式接入 SDK
Then SDK 均可正常工作，不强制绑定某一前端框架
```

**场景 4：非浏览器/SSR 环境安全守卫**
```gherkin
Given SDK 运行在无浏览器窗口（如服务端渲染）的环境中且未显式注入存储适配器
When 开发者初始化 SDK
Then SDK 报错并明确提示需注入存储适配器，而不是静默崩溃或误用浏览器存储
```

---

### 故事 2：注册与邮箱验证 [US-JS-002]

**优先级**: P0

**【用户故事】**
**作为**：第三方应用开发者（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：通过 SDK 提供注册与邮箱验证方法，由 SDK 处理 Client App 上下文与验证结果回跳
**从而**：在我的网页实现注册流程而无需手工拼装身份流程上下文

> 业务能力验收引用 [docs/user-stories/integration/custom-user-ui.md](/docs/user-stories/integration/custom-user-ui.md) [US-CUI-001]。

**【验收标准】**

**场景 1：注册成功并发起邮箱验证**
```gherkin
Given 开发者已初始化 SDK，且终端用户提供了有效的注册信息
When 开发者调用 SDK 的注册方法
Then SDK 返回注册受理结果，并提示后续需完成邮箱验证
```

**场景 2：邮箱验证后只回到预登记页面**
```gherkin
Given Client App 已预登记邮箱验证结果页
When 终端用户从邮件完成邮箱验证
Then 流程只引导到该 Client App 预登记的结果页
And 任意外部回跳地址不被接受（该约束由服务端强制，SDK 如实呈现结果）
```

**场景 3：注册信息不合规**
```gherkin
Given 终端用户提交的注册信息不符合规则（如邮箱格式错误、密码强度不足）
When 开发者调用 SDK 的注册方法
Then SDK 返回可区分的校验错误，便于开发者向终端用户展示具体原因
```

---

### 故事 3：找回与重置密码 [US-JS-003]

**优先级**: P0

**【用户故事】**
**作为**：第三方应用开发者（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：通过 SDK 发起找回密码并提交重置，由 SDK 处理 Client App 上下文与重置页回跳
**从而**：在我的网页实现找回密码流程而无需自建流程上下文

> 业务能力验收引用 [docs/user-stories/integration/custom-user-ui.md](/docs/user-stories/integration/custom-user-ui.md) [US-CUI-003]。

**【验收标准】**

**场景 1：发起找回成功**
```gherkin
Given 开发者已初始化 SDK，且终端用户提供了已注册账号的邮箱
When 开发者调用 SDK 的找回密码方法
Then SDK 返回受理结果，提示重置链接已发送
```

**场景 2：重置链接只回到预登记页面**
```gherkin
Given Client App 已预登记密码重置页
When 终端用户从邮件打开重置入口并提交新密码
Then 流程只引导到该 Client App 预登记的重置页
And 任意外部回跳地址不被接受（由服务端强制，SDK 如实呈现结果）
```

**场景 3：重置令牌失效或过期**
```gherkin
Given 终端用户使用的重置令牌已失效或过期
When 开发者调用 SDK 提交重置
Then SDK 返回可区分的错误，便于引导终端用户重新发起找回
```

---

### 故事 4：登录与多因素编排 [US-JS-004]

**优先级**: P0

**【用户故事】**
**作为**：第三方应用开发者（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：调用 SDK 登录方法，由 SDK 编排密码与二因素（TOTP / Passkey）各分支、提供无密码邮箱验证码登录，并返回登录会话
**从而**：用一套调用即可实现跨域登录，无需自建多因素分支判断

> 业务能力验收引用 [docs/user-stories/integration/custom-user-ui.md](/docs/user-stories/integration/custom-user-ui.md) [US-CUI-002]。

**【验收标准】**

**场景 1：仅密码登录成功**
```gherkin
Given 终端用户账号未开启二因素
When 开发者调用 SDK 密码登录方法并传入正确账号密码
Then SDK 返回登录成功，终端用户进入已登录状态
```

**场景 2：进入二因素分支**
```gherkin
Given 终端用户账号已绑定 TOTP 或 Passkey 之一
When 开发者调用 SDK 密码登录方法并传入正确账号密码
Then SDK 返回需要完成对应二因素挑战（TOTP 或 Passkey）的中间状态
And 开发者可继续调用对应的验证方法完成登录
```

**场景 3：无密码邮箱验证码登录**
```gherkin
Given 终端用户希望用邮箱验证码登录，且其邮箱已注册
When 开发者调用 SDK 的无密码邮箱验证码登录流程（先发送验证码、再校验）
Then 校验通过后 SDK 返回登录成功，终端用户进入已登录状态
And 该流程独立于密码登录，不被当作密码登录的二因素
```

**场景 4：登录需要同意协议**
```gherkin
Given 终端用户账号有待同意的用户协议或隐私政策
When 开发者调用 SDK 密码登录方法
Then SDK 返回需要同意协议的中间状态及待同意协议清单
And 集成方完成协议交互后携带协议同意标识重新调用，登录可继续
```

**场景 5：Passkey 归属隔离**
```gherkin
Given 当前 Client App 的站点来源对应独立的 Passkey 凭证范围
When 终端用户在该来源下使用 Passkey
Then 只能使用归属当前来源的凭证，来自其他来源或 Herald 原有范围的凭证不可见、不可用
```

**场景 6：登录失败可区分错误**
```gherkin
Given 终端用户输入错误密码、触发限流或人机验证失败
When 开发者调用 SDK 登录方法
Then SDK 返回可区分的错误类型，便于开发者向终端用户展示对应提示
```

> 补充：SDK 另提供企业目录（LDAP）登录 `loginWithLdap`，目录凭据由后端 LDAP 登录端点（search-then-bind）校验，登录结果与密码登录共用同一套多分支编排（成功令牌 / 二因素挑战 / 协议同意 / OAuth 回跳）。

---

### 故事 5：自动静默刷新 [US-JS-005]

**优先级**: P0

**【用户故事】**
**作为**：第三方应用开发者（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：登录后由 SDK 自动刷新过期的访问凭证，开发者无需编写任何刷新逻辑
**从而**：终端用户在凭证有效期内持续无感登录，避免并发刷新风暴与刷新死循环

**【验收标准】**

**场景 1：过期后静默刷新并重放**
```gherkin
Given 终端用户已登录，且访问凭证已过期但刷新凭证仍有效
When 开发者发起业务请求
Then SDK 自动用刷新凭证换发新访问凭证
And 原请求被自动重放一次并成功返回，终端用户无感知
```

**场景 2：并发请求只触发一次刷新**
```gherkin
Given 终端用户已登录，且多个业务请求几乎同时遇到凭证过期
When 这些并发请求同时需要刷新
Then SDK 只发起一次刷新，所有并发请求共享该结果并各自重放
```

**场景 3：不进入刷新死循环**
```gherkin
Given 某次刷新后重放的请求仍被判定需要刷新
When SDK 处理该请求
Then 不会无限重复刷新，按既定防循环策略终止
```

**场景 4：刷新凭证失效或被吊销引导重登**
```gherkin
Given 刷新凭证已过期、到达绝对有效上限，或因被复用导致整族被吊销
When 终端用户的会话无法继续刷新
Then SDK 清除当前会话并发出需要重新登录的明确信号，开发者可据此引导终端用户重新登录
```

---

### 故事 6：会话状态与登出 [US-JS-006]

**优先级**: P1

**【用户故事】**
**作为**：第三方应用开发者（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：通过 SDK 查询当前登录状态并执行登出
**从而**：在网页中正确展示登录态并在用户主动登出时彻底结束会话

> 业务能力验收引用 [docs/user-stories/integration/custom-user-ui.md](/docs/user-stories/integration/custom-user-ui.md) [US-CUI-006]。

**【验收标准】**

**场景 1：查询登录状态**
```gherkin
Given 终端用户已通过 SDK 登录
When 开发者调用 SDK 的状态查询方法
Then SDK 返回反映当前登录状态的结果
```

**场景 2：登出后彻底失效**
```gherkin
Given 终端用户已登录
When 开发者调用 SDK 的登出方法
Then 当前会话及其刷新凭证族被终止
And 随后的认证请求处于未登录状态
```

---

### 故事 7：可配置凭证存储 [US-JS-007]

**优先级**: P1

**【用户故事】**
**作为**：第三方应用开发者（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：能为 SDK 注入自定义的凭证存储实现，并在浏览器场景获得安全默认值
**从而**：按自身安全策略或运行环境（如仅内存、自定义存储）管理刷新凭证

**【验收标准】**

**场景 1：默认浏览器存储可用**
```gherkin
Given 开发者未注入自定义存储，且运行在普通浏览器环境
When 终端用户登录后刷新页面
Then 刷新凭证按默认浏览器存储恢复，终端用户保持登录（访问凭证本身不落盘）
```

**场景 2：注入自定义存储**
```gherkin
Given 开发者注入了一个自定义存储实现
When 终端用户登录
Then 刷新凭证按该自定义存储读写，而非默认浏览器存储
```

**场景 3：仅内存存储不持久化**
```gherkin
Given 开发者选择了仅内存的存储方式
When 终端用户登录后刷新或关闭页面
Then 刷新凭证不被保留，终端用户需重新登录
```

---

### 故事 8：可区分的错误反馈 [US-JS-008]

**优先级**: P1

**【用户故事】**
**作为**：第三方应用开发者（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：SDK 对各类异常返回类型化、可区分的错误
**从而**：在我的网页中按错误类型给出准确的终端用户提示与跳转

**【验收标准】**

**场景 1：网络/跨域错误与鉴权错误可区分**
```gherkin
Given 请求因来源未授权或网络问题失败
When SDK 返回错误
Then 该错误可与「凭证无效」「权限不足」等鉴权类错误明确区分
```

**场景 2：需要二因素、需要同意协议与需要重新登录可区分**
```gherkin
Given 登录返回需要二因素或需要同意协议，或会话因刷新失效需要重新登录
When SDK 返回错误或状态
Then 这些情形可被开发者明确区分，分别触发对应交互
```

**场景 3：错误携带可编程判别信息**
```gherkin
Given SDK 返回任意错误
When 开发者在代码中判断错误类型
Then 可通过稳定的错误类别（而非解析文案）进行分支处理
```

---

## 相关文档

- **JS 浏览器 SDK PRD**：[docs/prd/integration/js-sdk.md](/docs/prd/integration/js-sdk.md)
- **业务能力来源**：[docs/user-stories/integration/custom-user-ui.md](/docs/user-stories/integration/custom-user-ui.md)（US-CUI-001/002/003/006）
- **相关已发布 PRD**：[docs/prd/integration/custom-user-ui.md](/docs/prd/integration/custom-user-ui.md)
- **决策账本**：`.ai/decision-log/js-sdk.md`
