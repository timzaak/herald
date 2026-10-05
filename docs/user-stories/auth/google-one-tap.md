# Google One Tap 登录用户故事

> 角色定义以 [统一角色定义](../_roles.md) 为准。

## 用户故事

### 故事 1：通过 One Tap 在第三方应用一键登录 [US-OT-001]

**优先级**: P0

**作为**：普通用户（详见 [统一角色定义](../_roles.md)）  
**我希望**：在第三方应用页面上看到 Google One Tap 浮层，点击一次即可完成登录，无需跳转到 Herald 登录页  
**从而**：减少登录操作步骤，不被页面跳转打断当前任务

**场景 1：已注册用户通过 One Tap 登录成功**
```gherkin
Given 第三方应用已接入 Herald 并启用了 Google One Tap
And 用户已在 Herald 拥有账号（邮箱与 Google 账号一致）
And 用户的浏览器已登录 Google 账号
When 用户在第三方应用页面看到 One Tap 浮层并点击
Then 用户直接登录成功，无需离开当前页面
And 第三方应用页面显示用户已登录状态
```

**场景 2：One Tap 未注册用户自动创建账号（Realm 已开启自动注册）**
```gherkin
Given 用户在 Herald 系统中没有账号
And 用户的浏览器已登录 Google 账号
And 当前 Realm 已开启自动注册
When 用户在第三方应用页面点击 One Tap 浮层
Then 系统自动为该 Google 账号创建 Herald 用户
And 用户登录成功，第三方应用页面显示用户已登录状态
```

**场景 2b：Realm 未开启自动注册时拒绝自动建号**
```gherkin
Given 用户在 Herald 系统中没有账号
And 当前 Realm 未开启自动注册
When 用户在第三方应用页面点击 One Tap 浮层
Then 系统不自动创建账号，返回注册未开放提示（409 conflict）
And 引导用户走显式注册入口
```

**场景 3：用户关闭 One Tap 浮层**
```gherkin
Given 用户在第三方应用页面看到 One Tap 浮层
When 用户点击浮层右上角的关闭按钮
Then 浮层消失，不影响用户继续浏览第三方应用
And 在一段时间内不重复弹出（由 Google SDK 控制频率）
```

**场景 4：未登录 Google 账号时浮层不显示**
```gherkin
Given 用户的浏览器未登录任何 Google 账号
When 用户访问第三方应用页面
Then One Tap 浮层不弹出（Google SDK 无可展示的账号）
And 页面上仍显示常规登录入口
```

**场景 5：第三方应用页面不存在或域名未授权（失败场景）**
```gherkin
Given 第三方应用域名未在 Google OAuth 配置的授权域名列表中
When 用户访问该页面
Then One Tap 浮层不弹出
And 浏览器控制台显示 Google 授权域名错误
```

---

### 故事 2：第三方应用集成 One Tap [US-OT-002]

**优先级**: P0

**作为**：第三方应用开发者（详见 [统一角色定义](../_roles.md)）  
**我希望**：在我的应用页面上嵌入 Google One Tap，用户完成 Google 认证后通过 Herald 后端验证并建立会话  
**从而**：提供比跳转登录更低摩擦的登录体验，同时保持 Herald 统一的用户管理和安全控制

**场景 1：第三方应用嵌入 One Tap 并通过 Herald 后端验证**
```gherkin
Given 第三方应用已接入 Herald SSO
And Herald 中该 Realm 已配置 Google OAuth Provider
When 第三方应用在页面中初始化 Google One Tap SDK
And 用户点击 One Tap 后，第三方应用将 Google 签发的凭证发送给 Herald 后端
Then Herald 校验 Google 凭证有效（含签名、签发者、受众），拒绝被篡改或受众不符的凭证
And Herald 确认用户身份并返回会话或授权码
And 第三方应用根据返回结果完成登录
```

**场景 2：第三方应用使用 Authorization Code + PKCE 时的 One Tap**
```gherkin
Given 第三方应用通过 Authorization Code + PKCE 流程接入 Herald
And 用户通过 One Tap 完成 Google 认证
When 第三方应用将 Google 凭证和下游授权交易标识一起发送给 Herald
Then Herald 验证 Google 凭证通过后签发授权码
And 第三方应用使用授权码和 PKCE code_verifier 换取 access_token
```

**场景 3：Google 凭证被篡改或伪造（失败场景）**
```gherkin
Given 第三方应用发送给 Herald 的 Google 凭证签名无效
When Herald 后端验证签名失败
Then Herald 拒绝该凭证，返回认证失败
And 第三方应用登录失败，不创建任何会话
```

**场景 4：Google 凭证已过期（失败场景）**
```gherkin
Given 第三方应用发送给 Herald 的 Google 凭证已超过有效期
When Herald 后端验证凭证有效期
Then Herald 拒绝该凭证，返回认证失败
```

**场景 5：Google 凭证的受众（audience）不匹配（失败场景）**
```gherkin
Given 第三方应用发送给 Herald 的 Google 凭证中的受众字段与 Herald 该 Realm 配置的 Google Client ID 不一致
When Herald 后端验证受众
Then Herald 拒绝该凭证，返回认证失败
```

**场景 6：Realm 未配置 Google Provider（失败场景）**
```gherkin
Given Herald 中该 Realm 未配置或已禁用 Google OAuth Provider
When 第三方应用尝试使用 One Tap 凭证向 Herald 发起认证请求
Then Herald 返回错误，提示 Google Provider 未配置
And 不创建任何会话或用户
```

---

### 故事 3：One Tap 用户与已有账号关联 [US-OT-003]

**优先级**: P1

**作为**：普通用户（详见 [统一角色定义](../_roles.md)）  
**我希望**：通过 One Tap 登录时，系统能识别我已有的 Herald 账号（即使之前用密码或其他方式注册）  
**从而**：不会因为使用 One Tap 而产生重复账号

**场景 1：One Tap 邮箱与已有账号邮箱一致**
```gherkin
Given 用户之前用邮箱密码在 Herald 注册了账号
And 该邮箱与 Google 账号邮箱一致
When 用户在第三方应用通过 One Tap 登录
Then 系统识别到已有账号，将 Google 身份关联到该账号
And 用户登录成功，使用同一账号
```

**场景 2：One Tap 的 Google 用户 ID 与已有 OAuth 关联一致**
```gherkin
Given 用户之前已通过跳转式 Google 登录关联了 Herald 账号
When 用户在第三方应用通过 One Tap 登录（同一 Google 账号）
Then 系统通过 Google 用户 ID 识别到已有关联
And 用户登录成功，使用同一账号
```

**场景 3：One Tap 邮箱未验证时拒绝（失败场景）**
```gherkin
Given Google 返回的凭证表明用户邮箱未验证
When 用户在第三方应用通过 One Tap 登录
Then Herald 拒绝创建账号或登录
And 返回错误提示邮箱未验证
```

---

## 备注

### 业务规则

> 业务规则的唯一定义点是 PRD §4（触发位置、后端凭证验证、用户匹配策略、注册政策门控、一次性凭证、共存原则）：[docs/prd/auth/google-one-tap.md](/docs/prd/auth/google-one-tap.md)。本文场景中的数值口径（Google 凭证有效期）以 PRD 为准。

### 与现有用户故事的关系

- 扩展 [US-RU-003](../core/regular-user.md) OAuth 第三方登录，提供更低摩擦的 Google 登录入口
- 与 [US-TP-001](third-party-app.md) OAuth 授权码登录兼容，One Tap 可嵌入下游 Code+PKCE 流程
- 用户匹配逻辑复用现有 OAuth 回调的 `find_or_create_user` 策略
