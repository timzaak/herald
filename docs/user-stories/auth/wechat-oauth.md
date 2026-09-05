# 微信 OAuth 用户故事

> 角色定义见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)

## 租户管理员故事

### 故事 1：WeChat OAuth Provider 配置 [US-WO-001]

**优先级**: P1

**【用户故事】**
**作为**：Realm Admin（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：配置 WeChat OAuth Provider（网站应用微信登录），以便用户可以使用微信扫码登录
**从而**：提供便捷的微信登录选项

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：添加 WeChat OAuth Provider 配置**
```gherkin
Given 我是 realm-1 的管理员
And 我在 Settings -> Providers 页面
When 我点击 "Add Provider" 按钮
And 我选择 Provider Type 为 "WeChat"
And 我填写 OAuth 配置：
  | Client ID     | wx1234567890abcdef |
  | Client Secret | abcdef1234567890 |
  | Scopes        | snsapi_login |
  | Enabled       | true |
And 我提交表单
Then Provider 配置创建成功
And Provider 列表显示 WeChat Provider
And Scope 字段自动设置为 snsapi_login（固定值，不可修改）
```

**场景 2：添加 WeChat Mini Program Provider 配置**
```gherkin
Given 我是 realm-1 的管理员
And 我在 Settings -> Providers 页面
When 我添加 WeChat Mini Program Provider 配置：
  | Provider Type | WeChat Mini Program |
  | Client ID     | wx1234567890abcdef |
  | Client Secret | abcdef1234567890 |
  | Enabled       | true |
Then Provider 配置创建成功
And Scope 字段不显示（小程序不需要配置 scope）
```

**场景 3：启用/禁用 WeChat Provider**
```gherkin
Given 我是 realm-1 的管理员
And 已配置 WeChat Provider
When 我禁用 WeChat Provider
Then WeChat Provider 状态变为 "Disabled"

When 我重新启用 WeChat Provider
Then WeChat Provider 状态变为 "Enabled"
```

**场景 4：编辑 WeChat Provider 配置**
```gherkin
Given 我是 realm-1 的管理员
And 已配置 WeChat Provider
When 我编辑 WeChat Provider 配置
And 修改 Client ID 为 "wx1234567890abcdef-new"
And 保存更改（Client Secret 留空表示不更新）
Then Provider 配置更新成功
And 列表显示新的 Client ID
```

**场景 5：删除 WeChat Provider 配置**
```gherkin
Given 我是 realm-1 的管理员
And 已配置 WeChat Provider
When 我删除 WeChat Provider
And 确认删除
Then Provider 配置删除成功
And 列表不再显示该 Provider
```

**场景 6：Scope 配置验证（网站应用）**
```gherkin
Given 我是 realm-1 的管理员
And 我在添加 WeChat Provider 配置
When 我尝试将 Scope 修改为 "snsapi_userinfo"
Then 系统提示 "Scope 必须为 snsapi_login（网站应用固定值）"
And Scope 字段自动恢复为 snsapi_login
```

---

### 故事 2：WeChat Mini Program Provider 配置 [US-WO-002]

**优先级**: P1

**【用户故事】**
**作为**：Realm Admin（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：配置 WeChat Mini Program Provider，以便小程序用户可以登录
**从而**：支持小程序端的微信登录功能

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：添加 WeChat Mini Program Provider 配置**
```gherkin
Given 我是 realm-1 的管理员
And 我在 Settings -> Providers 页面
When 我添加 WeChat Mini Program Provider 配置：
  | Provider Type | WeChat Mini Program |
  | Client ID     | wx1234567890abcdef |
  | Client Secret | abcdef1234567890 |
  | Enabled       | true |
Then Provider 配置创建成功
And Scope 字段不显示（小程序不需要配置 scope）
```

**场景 2：启用/禁用 WeChat Mini Program Provider**
```gherkin
Given 我是 realm-1 的管理员
And 已配置 WeChat Mini Program Provider
When 我禁用 WeChat Mini Program Provider
Then WeChat Mini Program Provider 状态变为 "Disabled"

When 我重新启用 WeChat Mini Program Provider
Then WeChat Mini Program Provider 状态变为 "Enabled"
```

---

## 租户用户故事

### 故事 3：微信网站应用登录 [US-WO-003]

**优先级**: P1

**【用户故事】**
**作为**：普通用户（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：使用微信扫码登录系统，以便快速访问
**从而**：无需记忆额外密码

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：微信网站应用登录成功（新用户）**
```gherkin
Given Realm realm-1 已配置 WeChat OAuth Provider
And 用户未登录
When 用户在登录页面点击 "微信登录" 按钮
And 用户扫描二维码并授权
Then 用户账号创建成功
And 用户自动登录系统
And 用户信息页面显示已通过微信登录
```

**场景 2：微信网站应用登录成功（已存在用户）**
```gherkin
Given Realm realm-1 已配置 WeChat OAuth Provider
And 用户之前已通过小程序登录
And 用户未登录
When 用户在登录页面点击 "微信登录" 按钮
And 用户扫描二维码并授权
Then 系统匹配到已存在用户
And 用户自动登录系统
And 用户可以使用系统功能
```

**场景 3：用户拒绝授权**
```gherkin
Given 用户点击 "微信登录" 按钮
When 用户在微信授权页面点击 "取消"
Then 系统显示统一的 OAuth 登录失败提示
And 用户停留在登录页面
```

**场景 4：WeChat Provider 未启用**
```gherkin
Given Realm realm-2 未启用 WeChat OAuth Provider
When 用户访问登录页面
Then 不显示 "微信登录" 按钮
```

**场景 5：登录链接过期**
```gherkin
Given 用户发起微信登录
When 用户在超过5分钟后完成授权
Then 系统显示授权码无效或已过期的统一提示，并允许重新发起登录
```

**场景 6：已绑定开放平台的用户登录**
```gherkin
Given Realm realm-1 的 WeChat Provider 已绑定开放平台
And 用户之前已通过小程序登录
And 用户未登录
When 用户在登录页面点击 "微信登录" 按钮
And 用户扫描二维码并授权
Then 系统识别到同一用户（跨应用匹配）
And 用户自动登录系统
And 用户可以继续使用之前的账号数据
```

---

### 故事 4：微信小程序登录 [US-WO-004]

**优先级**: P1

**【用户故事】**
**作为**：小程序用户（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：使用微信账号登录，以便在小程序内访问 Herald 服务
**从而**：无缝的登录体验

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：微信小程序登录成功（新用户）**
```gherkin
Given Realm realm-1 已配置 WeChat Mini Program Provider
And 用户在小程序中打开登录页面
When 用户点击 "微信登录" 按钮
And 用户确认授权
Then 用户账号创建成功
And 用户自动登录系统
And 用户可以访问小程序内的 Herald 服务
```

**场景 2：微信小程序登录成功（已存在用户）**
```gherkin
Given Realm realm-1 已配置 WeChat Mini Program Provider
And 用户之前已通过网站应用微信登录
And 用户在小程序中打开登录页面
When 用户点击 "微信登录" 按钮
And 用户确认授权
Then 系统识别到同一用户（跨应用匹配）
And 用户自动登录系统
And 用户可以继续使用之前的账号数据
```

**场景 3：授权失败**
```gherkin
Given 用户在小程序中打开登录页面
And 用户点击 "微信登录" 按钮
When 授权失败或授权码过期
Then 系统显示统一的微信授权码校验失败提示
And 用户停留在登录页面
```

**场景 4：WeChat Mini Program Provider 未启用**
```gherkin
Given Realm realm-2 未启用 WeChat Mini Program Provider
And 用户在小程序中打开登录页面
When 用户尝试使用微信登录
Then 系统提示 "WeChat Mini Program provider not configured or not enabled"
```

**场景 5：跨应用用户匹配验证**
```gherkin
Given Realm realm-1 的 WeChat Provider 和 WeChat Mini Program Provider 已绑定开放平台
And 用户通过网站应用微信登录并创建账号
And 用户退出登录
When 用户通过小程序登录
Then 系统识别到同一用户
And 用户自动登录系统
And 用户可以继续使用之前的账号数据
```

---

## 相关文档

- **PRD**: [docs/prd/auth/wechat-oauth.md](/docs/prd/auth/wechat-oauth.md)
- **OAuth Provider 配置管理**: [docs/user-stories/auth/oauth-extension.md](/docs/user-stories/auth/oauth-extension.md)
- **普通用户 OAuth 第三方登录**: [docs/user-stories/core/regular-user.md](/docs/user-stories/core/regular-user.md)
