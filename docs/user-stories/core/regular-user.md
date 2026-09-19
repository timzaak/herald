# 普通用户 用户故事

> 角色定义见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)

## 用户故事

### 故事 1：账号注册 [US-RU-001]

**优先级**: P0

**【用户故事】**
**作为**：普通用户（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：能够在开启注册的 Realm 中注册账号
**从而**：获得系统访问权限

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：正常注册成功（不需要邮箱验证）**
```gherkin
Given Realm realm-1 已开启用户自注册功能
And 该 Realm 不要求邮箱验证
When 用户访问注册页面
And 输入邮箱、密码（符合密码策略）、确认密码、用户名（可选）
And 完成人机验证（若启用）
Then 注册成功，账号可立即使用
And 页面跳转到 Dashboard 或 redirect 页面
```

**场景 2：注册成功需要邮箱验证**
```gherkin
Given Realm realm-1 已开启用户自注册功能
And 该 Realm 要求邮箱验证
When 用户访问注册页面
And 输入邮箱、密码（符合密码策略）、确认密码、用户名（可选）
And 完成人机验证（若启用）
Then 注册成功，但账号需要邮箱验证后才能使用
And 系统发送验证邮件到用户邮箱
And 页面跳转到邮箱验证页面
And 显示"请查收验证邮件"的提示信息
And 用户无法登录，直到完成邮箱验证
```

**场景 3：邮箱验证成功**
```gherkin
Given 用户账号处于待验证状态
And 邮箱中包含验证链接
When 用户点击邮件中的验证链接
Then 验证成功，账号可以正常使用
And 显示"邮箱验证成功"的提示信息
```

**场景 4：重新发送验证邮件**
```gherkin
Given 用户在邮箱验证页面
When 用户点击"重新发送验证邮件"按钮
And 距离上次发送超过 60 秒
Then 系统重新发送验证邮件到用户邮箱
And 显示"验证邮件已发送"的提示信息
```

**场景 5：重新发送验证邮件频率限制**
```gherkin
Given 用户在邮箱验证页面
And 距离上次发送不足 60 秒
When 用户点击"重新发送验证邮件"按钮
Then 系统提示"请等待 60 秒后再试"
And 不发送新的验证邮件
```

**场景 6：未验证账号尝试登录**
```gherkin
Given 用户账号处于待验证状态
When 用户在登录页面输入邮箱和密码
Then 系统提示"邮箱未验证或账号未激活"
And 登录失败
And 用户停留在登录页面
```

**场景 7：邮箱格式验证失败**
```gherkin
Given 用户在注册页面
When 输入无效邮箱格式
Then 系统提示"请输入有效的邮箱地址"
```

**场景 8：密码不符合策略**
```gherkin
Given Realm 配置密码策略为最少 8 位、必须包含大小写字母、数字和特殊字符
When 用户在注册页面输入不符合策略的密码
Then 系统提示"密码必须包含大小写字母、数字和特殊字符"
```

**场景 9：两次密码不一致**
```gherkin
Given 用户在注册页面
When 输入密码和确认密码不一致
Then 系统提示"两次密码不一致"
```

**场景 10：邮箱已存在**
```gherkin
Given 某邮箱已注册
When 用户使用相同邮箱再次注册
Then 系统提示"该邮箱已被注册"
```

**场景 11：Realm 未开启注册**
```gherkin
Given Realm 未开启用户自注册功能
When 用户访问该 Realm 的注册页面
Then 系统提示"该 Realm 未开启注册功能"或隐藏注册入口
```

**场景 12：人机验证失败**
```gherkin
Given Realm 开启了人机验证
When 用户未完成人机验证就提交注册表单
Then 系统提示"请完成人机验证"
```

---

### 故事 2：账号登录 [US-RU-002]

**优先级**: P0

**【用户故事】**
**作为**：普通用户（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：能够使用邮箱和密码登录系统
**从而**：访问授权的第三方应用

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：正常登录成功**
```gherkin
Given 用户已注册账号并完成邮箱验证
When 用户在登录页面输入正确的邮箱和密码
And 系统自动关联当前 Client App
And 完成人机验证（若启用）
Then 登录成功，系统重定向到对应页面
```

**场景 2：密码错误**
```gherkin
Given 用户账号已注册
When 用户输入错误密码
Then 系统提示"邮箱或密码错误"，停留在登录页面
```

**场景 3：邮箱不存在**
```gherkin
Given 输入的邮箱未注册
When 用户使用该邮箱登录
Then 系统提示"邮箱或密码错误"
```

**场景 4：账号被禁用**
```gherkin
Given 用户账号已被管理员禁用
When 用户尝试登录
Then 系统提示"账号已被禁用，请联系管理员"
```

**场景 5：人机验证失败**
```gherkin
Given Realm 开启了人机验证
When 用户未完成人机验证就点击登录
Then 系统提示"请完成人机验证"
```

---

### 故事 3：OAuth 第三方登录 [US-RU-003]

**优先级**: P1

**【用户故事】**
**作为**：普通用户（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：能够使用第三方账号（Google、GitHub、Facebook、Apple）登录
**从而**：无需记忆额外密码

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：Google 登录成功**
```gherkin
Given Realm 已配置 Google OAuth
When 用户在登录页面点击"Continue with Google"
And 在 Google 页面授权
Then 用户登录成功
```

**场景 2：GitHub 登录成功**
```gherkin
Given Realm 已配置 GitHub OAuth
When 用户在登录页面点击"Continue with GitHub"
And 在 GitHub 页面授权
Then 用户登录成功
```

**场景 3：用户拒绝授权**
```gherkin
Given 用户点击"Continue with Google"
When 在授权页面点击"取消"
Then 系统提示"授权失败，请重试"
```

**场景 4：OAuth 未启用时不显示按钮**
```gherkin
Given Realm 未配置某个 OAuth Provider
When 用户访问登录页面
Then 不显示该 Provider 的登录按钮
```

**场景 5：OAuth 登录时 Email 关联**
```gherkin
Given 第三方账号返回的邮箱已在本 Realm 注册
When 用户通过该第三方账号登录
Then 系统自动关联到已有用户账号
```

---

### 故事 4：修改个人密码 [US-RU-004]

**优先级**: P0

**【用户故事】**
**作为**：普通用户（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：能够修改自己的登录密码
**从而**：定期更新密码保护账户安全

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：正常修改密码成功（两步验证）**
```gherkin
Given 用户已登录
When 用户访问个人资料页面并发起修改密码
And 先完成身份重新验证（当前密码 / TOTP / Passkey 三者之一）
Then 系统签发与本次操作绑定的短时验证票据
And 用户凭该票据提交新密码（符合密码策略）、确认密码
Then 密码修改成功，下次登录需使用新密码
```

**场景 2：身份重新验证失败**
```gherkin
Given 用户已登录并发起修改密码
When 输入错误的当前密码（或 TOTP / Passkey 验证失败）
Then 系统提示验证失败
And 不签发验证票据，无法进入密码修改步骤
```

**场景 3：新密码不符合策略**
```gherkin
Given Realm 配置了密码策略
When 用户输入不符合策略的新密码
Then 系统提示"密码必须符合安全策略"
```

**场景 4：两次新密码不一致**
```gherkin
Given 用户在密码修改页面
When 输入新密码和确认密码不一致
Then 系统提示"两次新密码不一致"
```

---

### 故事 5：查看个人资料 [US-RU-005]

**优先级**: P1

**【用户故事】**
**作为**：普通用户（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：能够查看自己的个人资料
**从而**：了解自己的账户信息

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：查看个人资料成功**
```gherkin
Given 用户已登录
When 用户访问个人资料页面
Then 显示邮箱、昵称、账号状态
```

**场景 2：邮箱为只读字段**
```gherkin
Given 用户在个人资料页面
Then 邮箱字段为只读，不可修改
```

**场景 3：账号状态为只读字段**
```gherkin
Given 用户在个人资料页面
Then 状态字段为只读，显示当前状态
```

---

### 故事 6：修改个人昵称 [US-RU-006]

**优先级**: P1

**【用户故事】**
**作为**：普通用户（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：能够修改自己的昵称
**从而**：个性化显示名称

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：正常修改昵称成功**
```gherkin
Given 用户已登录，当前昵称为"User"
When 用户进入个人资料页面
And 修改昵称为"John Doe"并保存
Then 昵称更新成功
```

**场景 2：昵称为可选字段**
```gherkin
Given 用户在个人资料页面
When 清空昵称并保存
Then 更新成功，昵称为空
```

**场景 3：昵称长度限制**
```gherkin
Given 用户在个人资料页面
When 输入昵称超过 50 个字符
Then 系统提示"昵称最多 50 个字符"
```

---

### 故事 7：退出登录 [US-RU-007]

**优先级**: P0

**【用户故事】**
**作为**：普通用户（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：能够安全退出登录
**从而**：保护账户安全

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：正常退出登录成功**
```gherkin
Given 用户已登录
When 用户点击"Logout"按钮
Then 系统清除登录状态并重定向到登录页面
```

**场景 2：退出后无法访问受保护资源**
```gherkin
Given 用户已退出登录
When 尝试访问需要认证的页面
Then 系统重定向到登录页面
```

---

### 故事 8：访问第三方应用 [US-RU-008]

**优先级**: P1

**【用户故事】**
**作为**：普通用户（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：能够使用 Herald 账号登录第三方应用
**从而**：获得单点登录（SSO）体验

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：通过 OAuth 重定向登录第三方应用**
```gherkin
Given 第三方应用已接入 Herald
And 用户在第三方应用页面点击"使用 Herald 登录"
When 用户被重定向到 Herald 登录页面
And 用户输入正确的邮箱和密码登录成功
Then 用户被重定向回第三方应用
And 第三方应用完成认证
```

**场景 2：Session 过期后重新登录**
```gherkin
Given 用户的登录会话已过期
When 用户访问第三方应用
Then 第三方应用将用户重定向到 Herald 登录页面
```

---

### 故事 9：认证重定向流程 [US-RU-009]

**优先级**: P1

**【用户故事】**
**作为**：所有用户（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：系统能够根据我的认证状态和权限智能重定向到适当的页面
**从而**：获得流畅的用户体验

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：未认证用户访问根路径**
```gherkin
Given 用户未登录
When 用户访问系统根路径
Then 系统重定向到 admin realm 的登录页面
```

**场景 2：未认证用户访问受保护路由**
```gherkin
Given 用户未登录
When 用户访问管理页面或个人资料页面
Then 系统重定向到对应 realm 的登录页面
```

**场景 3：拥有管理权限的用户登录重定向**
```gherkin
Given 用户拥有管理权限
When 用户登录成功
Then 系统重定向到管理后台
```

**场景 4：无管理权限的用户登录重定向**
```gherkin
Given 用户没有管理权限
When 用户登录成功
Then 系统重定向到个人资料页面
```

**场景 5：无管理权限的用户访问管理后台**
```gherkin
Given 用户已登录但没有管理权限
When 用户尝试访问管理后台
Then 系统重定向到个人资料页面
```

**场景 6：退出登录并重定向**
```gherkin
Given 用户已登录
When 用户点击退出登录按钮
Then 系统清除登录状态并重定向到登录页面
```

**业务规则**：
1. 根路径始终重定向到 admin realm 的登录页面
2. 登录成功后的重定向取决于用户拥有的权限，而非具体角色
3. 拥有任意管理权限的用户可访问管理后台，无管理权限的用户将被重定向到个人中心

---

### 故事 10：从第三方 Web 应用跳转登录 [US-RU-010]

**优先级**: P1

**【用户故事】**
**作为**：普通用户（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：从第三方 Web 应用跳转到 Herald 完成认证后自动返回第三方应用
**从而**：无需在第三方应用中输入密码，无缝使用第三方服务

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：从第三方跳转登录成功后自动返回**
```gherkin
Given 用户在第三方应用页面点击"使用 Herald 登录"
And Herald 登录页面显示了 OAuth 上下文信息（来源应用名称等）
When 用户输入正确的邮箱和密码完成登录
Then 页面自动跳转回第三方应用
And 用户可以正常使用服务
```

**场景 2：TOTP 二次认证后自动返回第三方**
```gherkin
Given 用户已启用 TOTP 二次认证
And 用户从第三方应用跳转到 Herald 登录
When 用户输入正确的密码后，系统要求输入 TOTP 验证码
And 用户输入正确的 TOTP 验证码
Then 验证通过后页面自动跳转回第三方应用
```

**场景 3：登录失败停留在 Herald 登录页**
```gherkin
Given 用户从第三方应用跳转到 Herald 登录
When 用户输入错误的密码
Then 页面停留在 Herald 登录页，显示错误提示
And OAuth 上下文信息保持不变，用户可以重试
```

**场景 4：OAuth 参数不完整时显示错误**
```gherkin
Given 用户通过某种方式访问到 Herald 登录页
And URL 中只有部分 OAuth 参数
When 页面加载时
Then 系统显示错误提示，告知用户 OAuth 参数不完整
And 不静默降级为普通登录
```
