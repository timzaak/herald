# 邮箱验证码登录 用户故事

> 角色定义以目标项目中的 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md) 为准。

---

## 用户故事

### 故事 1：用户用邮箱验证码登录已有账号 [US-EO-001]

**优先级**: P0

**【用户故事】**
**作为**：普通用户（Regular User，详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：在 ai-agent-app 手机端用邮箱收到的验证码登录，而不必记忆或输入密码
**从而**：降低再次登录的摩擦，快速回到应用

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：已注册且已激活的用户用验证码登录成功**
```gherkin
Given 用户 user@example.com 在当前 Realm 已注册并处于正常/已验证状态
And Realm 管理员已为该 Realm 启用邮箱验证码登录（首期运营只为 ai-agent-app 对应 Realm 打开）
When 用户在登录入口选择"邮箱验证码登录"
And 输入邮箱并完成人机验证（若启用）
And 在限定时间内收到并输入正确的验证码
Then 登录成功
And 会话按客户端会话方向（Bearer token）建立，而非依赖浏览器 cookie
And 用户进入应用首页或指定回跳页
```

**场景 2：验证码错误**
```gherkin
Given 用户已请求发送验证码
When 用户输入错误的验证码
Then 系统提示"验证码错误或已失效"
And 登录不成功，用户可重新输入
And 达到连续错误上限后该次验证码作废，需要重新发送
```

**场景 3：验证码过期**
```gherkin
Given 用户已请求发送验证码
When 用户在验证码有效期之后才提交
Then 系统提示"验证码已失效"
And 用户需要重新发送验证码
```

**场景 4：发送频率限制**
```gherkin
Given 用户针对同一邮箱连续请求发送验证码
When 两次请求间隔低于限流阈值
Then 系统提示"请稍后再试"
And 不发送新的验证码
```

**场景 5：账号被禁用**
```gherkin
Given 用户的账号已被禁用
When 用户提交正确的验证码
Then 登录被拒绝
And 系统提示账号已被禁用
```

**场景 6：验证码被一次性消费后不能重复使用**
```gherkin
Given 用户已用某验证码成功登录
When 同一验证码被再次提交
Then 用户看到"验证码已失效"的提示
And 登录不成功，需要重新发送验证码
```

---

### 故事 2：未注册邮箱验证成功后自动注册 [US-EO-002]

**优先级**: P0

**【用户故事】**
**作为**：首次进入 ai-agent-app 的手机用户（Regular User，详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：用邮箱验证码一次操作完成登录和注册
**从而**：不必先走单独的注册流程再登录

**【验收标准】**

**场景 1：未注册邮箱完成验证并自动创建激活账户**
```gherkin
Given 输入的邮箱在当前 Realm 不存在对应账号
And Realm 已启用邮箱验证码登录与自动注册（首期运营只为 ai-agent-app 对应 Realm 打开）
When 用户通过"邮箱验证码登录"输入邮箱、完成同意表达并验证成功
Then 系统自动创建账户
And 账户处于已验证/已激活状态（不要求用户再走单独的邮箱验证）
And 会话按客户端会话方向（Bearer token）建立
And 用户进入应用首页或指定回跳页
```

**场景 2：自动注册前未表达同意则不创建账户**
```gherkin
Given 用户使用未注册邮箱发起验证码登录
When 用户未完成当前 Realm 生效协议的同意表达
Then 用户看到"需先同意协议才能继续"的提示，无法发送验证码
And 不创建任何账户
```

**场景 3：Realm 不允许自动注册时退回提示**
```gherkin
Given 当前 Realm 未开启自动注册
When 用户使用未注册邮箱发起验证码登录
Then 系统不创建账户
And 提示用户该邮箱未注册或引导到显式注册入口
```

**场景 4：自动注册创建的账户可与既有登录方式共存**
```gherkin
Given 用户经自动注册创建了账户
When 用户随后设置密码或绑定 Passkey/TOTP
Then 这些认证方式作为后续可选登录入口
And 账户身份与自动注册时一致
```

---

### 故事 3：Realm 管理员配置邮箱验证码登录与自动注册 [US-EO-003]

**优先级**: P1

**【用户故事】**
**作为**：Realm 管理员（Realm Admin，详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：能为本 Realm 启用或关闭邮箱验证码登录，并单独控制自动注册
**从而**：在验证价值的同时控制滥用与误注册风险

**【验收标准】**

**场景 1：启用邮箱验证码登录**
```gherkin
Given Realm 管理员在"Settings" -> "Security"页面
When 管理员启用"邮箱验证码登录"
Then 本 Realm 用户可在登录页看到邮箱验证码登录入口
```

**场景 2：单独控制自动注册**
```gherkin
Given Realm 已启用邮箱验证码登录
When 管理员启用或关闭"未注册邮箱自动注册"
Then 启用时未注册邮箱验证成功后创建账户
And 关闭时未注册邮箱只能得到未注册提示，不自动创建账户
```

**场景 3：关闭邮箱验证码登录后平滑降级**
```gherkin
Given Realm 已启用邮箱验证码登录并有用户使用过
When 管理员关闭该功能
Then 登录页不再展示邮箱验证码入口
And 已注册用户仍可使用密码 / TOTP / Passkey 登录
```

**场景 4：无法修改其他 Realm 设置**
```gherkin
Given 管理员属于 realm-1
When 尝试访问 realm-2 的邮箱验证码登录设置
Then 系统提示权限不足并拒绝访问
```

---

## 业务规则

> 业务规则的唯一定义点是 PRD §4（首期范围、邮箱身份与自动注册、同意模型、凭证定位与认证方式共存、per-Realm 开关、防滥用与防枚举、关键状态与异常）：[docs/prd/auth/email-otp-login.md](/docs/prd/auth/email-otp-login.md)。本文场景中的数值口径（验证码有效期、发送频率与尝试次数上限）以 PRD 为准。

---

## 相关文档

- PRD：[docs/prd/auth/email-otp-login.md](/docs/prd/auth/email-otp-login.md)
- 会话方向承接：[docs/prd/integration/custom-user-ui.md](/docs/prd/integration/custom-user-ui.md)（Bearer access/refresh token）
- 协议同意模型：[docs/prd/core/legal-consent-account-deletion.md](/docs/prd/core/legal-consent-account-deletion.md)（注册即同意 / 登录即同意）
- 既有可引用用户故事：
  - [docs/user-stories/core/regular-user.md](/docs/user-stories/core/regular-user.md)（US-RU-001 注册、US-RU-002 登录）
  - [docs/user-stories/auth/passkey.md](/docs/user-stories/auth/passkey.md)（共存与增强定位）
- 角色定义：[docs/user-stories/_roles.md](/docs/user-stories/_roles.md)
