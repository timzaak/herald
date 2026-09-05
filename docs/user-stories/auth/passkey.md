# Passkey 认证用户故事

> 角色定义以目标项目中的 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md) 为准。

---

## 用户故事

### 故事 1：Realm 管理员启用/禁用 Passkey 功能 [US-PK-001]

**优先级**: P0

**【用户故事】**
**作为**：Realm 管理员（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：能够为本 Realm 启用或禁用 Passkey 认证功能
**从而**：让本 Realm 用户可以使用无密码、防钓鱼的 Passkey 登录

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：启用 Passkey 功能**
```gherkin
Given Realm 管理员 admin-realm1 属于 realm-1
When 管理员在"Settings" -> "Security"页面中启用 Passkey
Then Realm realm-1 的 Passkey 功能被启用
And 本 Realm 用户可以注册和使用 Passkey
```

**场景 2：禁用 Passkey 功能**
```gherkin
Given Realm realm-1 已启用 Passkey 功能
When 管理员在"Settings" -> "Security"页面中禁用 Passkey
Then Realm realm-1 的 Passkey 功能被禁用
And 新用户无法注册新的 Passkey
And 已注册 Passkey 的用户仍可回退到密码/TOTP 登录（平滑降级）
```

**场景 3：查看 Passkey 功能状态**
```gherkin
Given 管理员在"Settings" -> "Security"页面
Then 显示当前 Passkey 功能的启用状态（启用/禁用）
```

**场景 4：无法修改其他 Realm 设置（失败场景）**
```gherkin
Given 管理员属于 realm-1
When 尝试访问 realm-2 的 Passkey 设置
Then 系统提示"权限不足"并拒绝访问
```

---

### 故事 2：Realm 管理员强制启用 Passkey [US-PK-002]

**优先级**: P0

**【用户故事】**
**作为**：Realm 管理员（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：能够设置本 Realm 强制使用 Passkey
**从而**：提升本 Realm 整体账户安全性，减少密码泄露风险

**【验收标准】**

**场景 1：启用强制 Passkey 模式**
```gherkin
Given Realm 管理员在"Settings" -> "Security"页面
When 启用"Force Passkey"选项
Then 已注册 Passkey 的用户会被优先引导使用 Passkey 登录
And 未注册 Passkey 的用户下次登录时被引导注册 Passkey
And 所有用户仍可使用密码/TOTP 作为回退
```

**场景 2：禁用强制 Passkey 模式**
```gherkin
Given Realm realm-1 处于"强制 Passkey"模式
When 管理员禁用"Force Passkey"选项
Then 用户可以选择不使用 Passkey 登录
And 已注册 Passkey 的用户仍可继续使用
```

---

### 故事 3：Realm 管理员配置 Passkey 安全策略 [US-PK-003]

**优先级**: P1

**【用户故事】**
**作为**：Realm 管理员（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：能够配置本 Realm 的 Passkey 安全策略
**从而**：在安全性和用户体验之间取得平衡

**【验收标准】**

**场景 1：配置用户验证要求**
```gherkin
Given Realm 管理员在"Settings" -> "Security" -> "Passkey Policy"页面
When 设置用户验证策略为"Preferred"或"Required"
Then 该策略应用于本 Realm 所有 Passkey 注册和认证
And 策略变更后新生的 Passkey 按新策略执行
```

**场景 2：配置允许跨平台 authenticator**
```gherkin
Given Realm 管理员在 Passkey 策略页面
When 启用或禁用"允许跨平台 authenticator"选项
Then 注册流程按该配置接受或拒绝对应的 authenticator 类型
```

---

### 故事 4：用户注册 Passkey [US-PK-004]

**优先级**: P0

**【用户故事】**
**作为**：普通用户（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：能够为我的账户注册 Passkey
**从而**：使用无密码方式登录，提升账户安全性

**【验收标准】**

**场景 1：正常注册 Passkey**
```gherkin
Given Realm realm-1 已启用 Passkey 功能
And 用户已登录
When 用户访问个人资料 -> "Security"页面
And 点击"Add Passkey"按钮
And 按照浏览器提示完成生物识别或设备 PIN 验证
Then 系统成功注册 Passkey
And 显示注册成功的设备名称（用户可修改）
And 该 Passkey 可用于后续登录
```

**场景 2：Realm 未启用 Passkey（失败场景）**
```gherkin
Given Realm realm-2 未启用 Passkey 功能
When 用户访问"Security"页面
Then 不显示"Add Passkey"选项
```

**场景 3：浏览器不支持 WebAuthn（失败场景）**
```gherkin
Given 用户使用的浏览器不支持 WebAuthn
When 用户点击"Add Passkey"按钮
Then 系统提示"当前浏览器不支持 Passkey"
And 建议用户更换浏览器或继续使用密码登录
```

**场景 4：用户取消注册（失败场景）**
```gherkin
Given 用户在注册 Passkey 过程中
When 在浏览器弹窗中点击取消
Then 系统取消注册流程
And 不创建新的 Passkey
And 页面保持原状，可重新发起注册
```

**场景 5：注册多个 Passkey**
```gherkin
Given 用户已注册一个 Passkey
When 再次点击"Add Passkey"
And 使用不同的 authenticator 完成注册
Then 系统允许注册第二个 Passkey
And 两个 Passkey 都可用于登录
```

---

### 故事 5：用户使用 Passkey 直接登录 [US-PK-005]

**优先级**: P0

**【用户故事】**
**作为**：普通用户（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：能够使用 Passkey 直接登录，无需输入密码
**从而**：获得更快、更安全的登录体验

**【验收标准】**

**场景 1：正常 Passkey 直接登录（usernameless）**
```gherkin
Given 用户 user@example.com 已注册 Passkey
And 浏览器支持 WebAuthn
When 用户访问登录页面
And 系统显示 Passkey 登录选项
And 用户选择使用 Passkey 登录
And 按照浏览器提示完成生物识别或设备 PIN 验证
Then 登录成功
And 系统跳转至首页
```

**场景 2：自动填充（conditional UI）触发**
```gherkin
Given 用户已在本浏览器注册 Passkey
When 用户聚焦登录页面的用户名/邮箱输入框
Then 浏览器自动提示可用的 Passkey
And 用户选择 Passkey 并完成验证后登录成功
```

**场景 3：未注册 Passkey 的用户尝试 Passkey 登录（失败场景）**
```gherkin
Given 用户 user@example.com 未注册 Passkey
When 用户点击"Use Passkey"登录选项
And 浏览器未找到可用的 Passkey
Then 系统提示"未找到可用的 Passkey"
And 用户可切换回密码登录
```

**场景 4：验证失败（失败场景）**
```gherkin
Given 用户已注册 Passkey
When 用户在浏览器验证步骤中失败（如取消、生物识别不匹配）
Then 系统提示"Passkey 验证失败"
And 用户可重试或切换回密码登录
```

---

### 故事 6：用户在密码登录后使用 Passkey 作为第二因素 [US-PK-006]

**优先级**: P0

**【用户故事】**
**作为**：普通用户（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：在输入密码登录后，可以使用 Passkey 完成二次验证
**从而**：即使密码泄露，账户仍受 Passkey 保护

**【验收标准】**

**场景 1：正常 Passkey 二次验证流程**
```gherkin
Given 用户 user@example.com 已启用 Passkey 作为第二因素
When 用户输入正确的邮箱和密码
Then 密码验证通过
And 系统显示 Passkey 验证提示
And 用户按照浏览器提示完成 Passkey 验证
Then 登录成功，系统跳转至首页
```

**场景 2：Passkey 验证失败（失败场景）**
```gherkin
Given 用户已启用 Passkey 作为第二因素
When 用户在 Passkey 验证步骤中失败
Then 系统提示"Passkey 验证失败"
And 用户可重试或切换回 TOTP 验证（若已启用 TOTP）
```

**场景 3：未启用 Passkey 作为第二因素**
```gherkin
Given 用户未启用 Passkey 作为第二因素
When 输入密码后
Then 系统不显示 Passkey 验证步骤
And 按现有流程完成登录（可能进入 TOTP 或直接登录）
```

---

### 故事 7：用户查看和重命名已注册 Passkey [US-PK-007]

**优先级**: P0

**【用户故事】**
**作为**：普通用户（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：能够查看和重命名我注册的所有 Passkey
**从而**：了解哪些设备可以登录我的账户，并使用可识别的名称区分不同设备

**【验收标准】**

**场景 1：查看已注册 Passkey 列表**
```gherkin
Given 用户已注册多个 Passkey
When 用户访问个人资料 -> "Security"页面
Then 显示所有已注册 Passkey 的列表
And 显示每个 Passkey 的设备名称、注册时间和最近使用时间
And 显示每个 Passkey 是否为可同步 Passkey（sync passkey）
```

**场景 2：重命名 Passkey**
```gherkin
Given 用户已注册 Passkey
When 用户在列表中点击"编辑名称"
And 输入新的设备名称
Then 该 Passkey 的显示名称更新为新名称
```

---

### 故事 8：用户在无法使用 Passkey 时回退到密码/TOTP [US-PK-008]

**优先级**: P0

**【用户故事】**
**作为**：普通用户（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：在无法使用 Passkey 时能够使用密码或 TOTP 登录
**从而**：避免因为设备丢失或浏览器不兼容而被锁定账户

**【验收标准】**

**场景 1：从 Passkey 登录回退到密码登录**
```gherkin
Given 用户已注册 Passkey
When 用户在 Passkey 登录页面点击"Use password instead"
Then 系统切换到密码登录流程
And 用户输入邮箱和密码后可继续登录
```

**场景 2：Passkey 验证失败后回退**
```gherkin
Given 用户已注册 Passkey
When 用户在 Passkey 验证步骤中失败
Then 系统显示"重试"和"使用密码登录"选项
And 用户选择"使用密码登录"后进入密码登录流程
```

**场景 3：浏览器不支持 WebAuthn**
```gherkin
Given 用户使用的浏览器不支持 WebAuthn
When 用户访问登录页面
Then 系统自动显示密码登录入口
And 不显示 Passkey 登录选项
```

---

### 故事 9：用户删除 Passkey [US-PK-009]

**优先级**: P0

**【用户故事】**
**作为**：普通用户（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：能够删除我不再使用的 Passkey
**从而**：防止旧设备或丢失设备继续访问我的账户

**【验收标准】**

**场景 1：正常删除 Passkey**
```gherkin
Given 用户已注册 Passkey
When 用户在"Security"页面点击"删除"并确认
Then 该 Passkey 被移除
And 系统提示"Passkey 已删除"
And 该 Passkey 不能再用于登录
```

**场景 2：删除最后一个 Passkey 的提示**
```gherkin
Given 用户只注册了一个 Passkey
When 用户尝试删除该 Passkey
Then 系统提示"删除后您将只能使用密码/TOTP 登录"
And 用户确认后才执行删除
```

---

### 故事 10：Realm 管理员查看 Passkey 使用情况统计 [US-PK-010]

**优先级**: P2

**【用户故事】**
**作为**：Realm 管理员（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：能够查看本 Realm 的 Passkey 使用情况统计
**从而**：了解用户采用率并做出安全策略调整

**【验收标准】**

**场景 1：查看 Passkey 启用率**
```gherkin
Given Realm 管理员在"Settings" -> "Security"页面
Then 显示已注册 Passkey 的用户数
And 显示未注册 Passkey 的用户数
And 显示 Passkey 启用率百分比
```

**场景 2：查看 Passkey 登录统计**
```gherkin
Given Realm 管理员在"Settings" -> "Security"页面
Then 显示最近 N 天内使用 Passkey 登录的次数
And 显示使用密码登录的次数
```

---

## 业务规则备注

1. **Passkey 配置级别**：
   - **Realm 级别**：管理员可启用/禁用整个 Realm 的 Passkey 功能，并可设置强制模式。
   - **用户级别**：用户可选择是否注册 Passkey（若 Realm 允许），并管理自己的 Passkey 设备。

2. **多设备支持**：
   - 一个用户可以拥有多个 Passkey credential（多设备）。
   - 用户可以为每个 Passkey 设置可识别的名称。

3. **回退机制**：
   - 系统必须保留密码（和/或 TOTP）作为回退认证方式。
   - 当浏览器不支持 WebAuthn、用户取消验证或设备不可用时，必须提供回退入口。

4. **强制 Passkey 模式**：
   - Realm 管理员可强制所有用户使用 Passkey。
   - 强制模式下，未注册 Passkey 的用户下次登录时被引导注册，但仍保留密码/TOTP 回退以防止账户锁定。

5. **安全规则**：
   - Passkey 注册和认证必须验证当前用户身份。
   - 删除最后一个 Passkey 前需要明确提示用户。
   - Passkey 验证失败不暴露具体原因（统一提示验证失败）。

6. **兼容性说明**：
   - 现代浏览器支持 WebAuthn；IE 不支持。
   - 生产环境必须 HTTPS。
   - Passkey 多设备同步依赖平台生态（Apple iCloud Keychain、Google Password Manager 等）。

---

## 相关文档

- **Passkey 认证 PRD**: [docs/prd/auth/passkey.md](/docs/prd/auth/passkey.md)
- **TOTP 二次认证 PRD**: [docs/prd/auth/totp.md](/docs/prd/auth/totp.md)
- **角色定义**: [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)
