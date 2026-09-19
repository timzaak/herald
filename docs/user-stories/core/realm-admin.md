# Realm Admin 用户故事

> 角色定义见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)

## 用户故事

### 故事 1：Realm 隔离访问 [US-RA-001]

**优先级**: P0

**【用户故事】**
**作为**：Realm Admin（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：只能访问自己 Realm 的资源
**从而**：确保多租户系统的安全性和数据隔离

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：访问自己 Realm 的资源**
```gherkin
Given 我是 realm-1 的管理员
When 我访问 realm-1 的用户管理页面
Then 我可以看到 realm-1 的所有用户
And 我可以管理这些用户
```

**场景 2：不能访问其他 Realm 的资源**
```gherkin
Given 我是 realm-1 的管理员
When 我尝试访问 realm-2 的用户管理页面
Then 系统拒绝访问并显示权限不足提示
And 显示错误消息："Access denied: You do not have permission to access this realm"
```

---

### 故事 2：角色定义管理 [US-RA-002]

**优先级**: P0

**【用户故事】**
**作为**：Realm Admin（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：管理角色定义，以便定义不同的用户角色
**从而**：灵活控制用户权限

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：创建角色定义**
```gherkin
Given 我是 realm-1 的管理员
When 我在角色管理页面点击 "Create Role" 按钮
And 我填写角色信息：
  | Name        | user-admin       |
  | Description | 用户管理员        |
And 我提交表单
Then 角色定义创建成功
And 系统显示成功消息："Role 'user-admin' created successfully"
```

**场景 2：查看角色定义列表**
```gherkin
Given 我是 realm-1 的管理员
When 我访问角色管理页面
Then 我看到角色定义列表
And 列表包含默认角色：
  | Name         | Description     |
  | realm-admin  | Realm 管理员     |
  | user         | 普通用户        |
And 列表包含我创建的自定义角色
```

**场景 3：编辑角色定义**
```gherkin
Given 我是 realm-1 的管理员
And 已存在角色 "user-admin"
When 我编辑该角色的描述为 "高级用户管理员"
Then 角色定义更新成功
And 列表显示更新后的描述
```

**场景 4：删除角色定义**
```gherkin
Given 我是 realm-1 的管理员
And 已存在角色 "temp-role" 且未被使用
When 我删除该角色
Then 角色定义删除成功
And 列表不再显示该角色
```

---

### 故事 3：权限定义管理 [US-RA-003]

**优先级**: P0

**【用户故事】**
**作为**：Realm Admin（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：管理权限定义，以便定义系统权限
**从而**：精确控制用户可访问的资源

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：创建权限定义**
```gherkin
Given 我是 realm-1 的管理员
When 我在权限管理页面点击 "Create Permission" 按钮
And 我填写权限信息：
  | Name        | users.delete       |
  | Description | 删除用户权限        |
And 我提交表单
Then 权限定义创建成功
```

**场景 2：查看权限定义列表**
```gherkin
Given 我是 realm-1 的管理员
When 我访问权限管理页面
Then 我看到权限定义列表
And 列表包含默认权限
And 列表包含我创建的自定义权限
```

---

### 故事 4：为角色分配权限 [US-RA-004]

**优先级**: P0

**【用户故事】**
**作为**：Realm Admin（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：为角色分配权限，以便控制角色可访问的资源
**从而**：实现基于角色的权限控制

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：为角色分配权限**
```gherkin
Given 我是 realm-1 的管理员
And 已存在角色 "user-admin"
When 我在角色详情页面选择 "user-admin" 角色
And 我为该角色分配权限 users.view 和 users.manage
And 我保存更改
Then 权限分配成功
And "user-admin" 角色拥有对应的权限
```

**场景 2：查看角色的权限**
```gherkin
Given 我是 realm-1 的管理员
When 我查看 "user-admin" 角色的详情
Then 我看到该角色拥有的所有权限
And 权限列表清晰展示每个权限项
```

**场景 3：移除角色权限**
```gherkin
Given 我是 realm-1 的管理员
And "user-admin" 角色拥有 "users.delete" 权限
When 我移除该权限
Then "user-admin" 角色不再拥有 "users.delete" 权限
And 该角色的用户无法删除用户
```

**场景 4：权限层级自动生效**
```gherkin
Given 我是 realm-1 的管理员
And 已存在角色 "user-admin"
When 我为该角色仅分配 "users.manage" 权限
And 我保存更改
Then "user-admin" 角色自动拥有 users.view 和 users.manage 权限
And "user-admin" 角色可以执行所有用户操作：创建、查看、编辑、删除等
```

**场景 5：低权限不覆盖高权限**
```gherkin
Given 我是 realm-1 的管理员
And 已存在角色 "viewer"
When 我为该角色仅分配 "users.view" 权限
And 我保存更改
Then "viewer" 角色仅拥有 users.view 权限
And "viewer" 角色不能执行任何修改操作（编辑、删除、创建等）
```

**场景 6：不同资源的权限互不影响**
```gherkin
Given 我是 realm-1 的管理员
And 已存在角色 "user-and-client-admin"
When 我为该角色仅分配 "users.manage" 权限
Then 该角色仅拥有 users 资源的完整权限
And 该角色不能访问其他资源（如 clients）的管理操作
```

---

### 故事 5：查看角色权限 [US-RA-005]

**优先级**: P0

**【用户故事】**
**作为**：Realm Admin（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：查看角色的权限，以便了解角色能力
**从而**：更好地管理和审计权限

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：查看单个角色的权限**
```gherkin
Given 我是 realm-1 的管理员
When 我在角色列表中点击 "realm-admin" 角色
Then 我看到该角色的详情页面
And 页面显示角色名称、描述和全部权限列表
```

**场景 2：批量查看所有角色的权限**
```gherkin
Given 我是 realm-1 的管理员
When 我访问角色管理页面
Then 我看到角色列表
And 每个角色显示其权限数量
And 我可以点击角色查看详细权限
```

---

### 故事 6：用户角色分配 [US-RA-006]

**优先级**: P0

**【用户故事】**
**作为**：Realm Admin（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：为用户分配角色，以便控制用户权限
**从而**：实现灵活的用户权限管理

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

> **说明**：本故事的权限模型同样适用于 API Key 角色分配，详见 [US-RA-016](#故事-16api-key-角色管理-us-ra-016)。两者共享相同的角色选择和保存交互模式。

**场景 1：为用户分配角色**
```gherkin
Given 我是 realm-1 的管理员
And 已存在用户 "user@example.com"
And 已存在角色 "user-admin"
When 我在用户详情页面访问 "Roles" 标签
And 我为该用户分配 "user-admin" 角色
And 我保存更改
Then 角色分配成功
And 用户 "user@example.com" 拥有 "user-admin" 角色的所有权限
```

**场景 2：为用户分配多个角色**
```gherkin
Given 我是 realm-1 的管理员
And 已存在用户 "advanced-user@example.com"
And 已存在角色 "user-admin" 和 "billing-admin"
When 我为该用户分配两个角色
And 我保存更改
Then 角色分配成功
And 用户拥有两个角色的所有权限（并集）
```

**场景 3：移除用户角色**
```gherkin
Given 我是 realm-1 的管理员
And 用户 "user@example.com" 拥有 "user-admin" 角色
When 我移除该角色
Then 用户不再拥有 "user-admin" 角色的权限
And 用户只保留剩余角色的权限
```

**场景 4：查看用户的角色**
```gherkin
Given 我是 realm-1 的管理员
When 我查看用户 "user@example.com" 的详情
Then 我看到该用户的角色列表
And 角色列表显示角色名称和分配时间
```

---

### 故事 7：权限策略管理 [US-RA-007]

**优先级**: P0

**【用户故事】**
**作为**：Realm Admin（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：管理权限策略，以便定义资源访问规则
**从而**：实现细粒度的权限控制

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：创建权限策略**
```gherkin
Given 我是 realm-1 的管理员
When 我在权限策略页面点击 "Create Policy" 按钮
And 我填写策略信息：
  | Role    | user-admin   |
  | Resource| users        |
  | Action  | manage       |
And 我提交表单
Then 权限策略创建成功
And 拥有 "user-admin" 角色的用户可以管理用户资源
```

**场景 2：查看权限策略列表**
```gherkin
Given 我是 realm-1 的管理员
When 我访问权限策略页面
Then 我看到权限策略列表
And 列表包含管理员角色的默认策略
And 列表包含我创建的自定义策略
```

> 注：当前无 Realm 级全量策略列表端点；策略列表按角色聚合展示（先选角色，再查看该角色的策略）。

**场景 3：删除权限策略**
```gherkin
Given 我是 realm-1 的管理员
And 已存在策略："user-admin 可以 manage users"
When 我删除该策略
Then 策略删除成功
And 拥有 "user-admin" 角色的用户无法再管理用户资源
```

---

### 故事 8：订阅套餐管理 [US-RA-008]

**优先级**: P0

**【用户故事】**
**作为**：Realm Admin（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：管理订阅套餐，以便为用户提供不同的订阅选项
**从而**：实现灵活的订阅计费模式

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：创建订阅套餐**
```gherkin
Given 我是 realm-1 的管理员
When 我在 Billing 管理页面点击 "Create Plan" 按钮
And 我填写套餐信息：
  | Name         | basic                |
  | Title        | 基础版               |
  | Description  | 适合小型团队          |
  | Type         | monthly              |
  | Price        | 1000                 |
  | Currency     | USD                  |
And 我提交表单
Then 订阅套餐创建成功
And 套餐列表显示新创建的套餐
```

**场景 2：分配套餐到 Client App**
```gherkin
Given 我是 realm-1 的管理员
And 已存在套餐 "basic"
And 已存在 Client App "mobile-app"
When 我在套餐管理页面点击 "Assign" 按钮
And 我选择 "mobile-app"
And 我保存分配
Then 套餐分配成功
And "mobile-app" 的用户可以看到 "basic" 套餐
```

**场景 3：查看订阅列表**
```gherkin
Given 我是 realm-1 的管理员
When 我访问订阅管理页面
Then 我看到订阅列表
And 列表包含用户、套餐、状态、计费周期等信息
```

---

### 故事 9：权限层级验证 [US-RA-009]

**优先级**: P0

**【用户故事】**
**作为**：Realm Admin（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：系统能自动应用权限层级规则，以便简化角色权限配置
**从而**：减少手动配置多个权限的工作量

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：Manage 权限自动覆盖所有操作**
```gherkin
Given 我是 realm-1 的管理员
And 已存在角色 "admin"
When 我为该角色仅分配 "users.manage" 权限
And 用户 "admin-user" 拥有 "admin" 角色
When 该用户尝试执行以下操作：
  | 操作 | 预期结果 |
  | 查看用户列表 | 成功 |
  | 编辑用户信息 | 成功 |
  | 删除用户 | 成功 |
  | 重置用户密码 | 成功 |
Then 所有符合预期的操作都能正常执行
```

**场景 2：验证层级关系不影响低权限角色**
```gherkin
Given 我是 realm-1 的管理员
And 已存在角色 "viewer"
And 我为该角色仅分配 "users.view" 权限
And 用户 "viewer-user" 拥有 "viewer" 角色
When 该用户尝试执行以下操作：
  | 操作 | 预期结果 |
  | 查看用户列表 | 成功 |
  | 编辑用户信息 | 失败 |
  | 删除用户 | 失败 |
Then 只有查看操作能正常执行
```

---

### 故事 10：查看 Dashboard 用户活跃概览 [US-RA-010]

**优先级**: P1

**【用户故事】**
**作为**：Realm Admin（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：在 Admin Dashboard 首屏看到本 Realm 的用户核心指标（总用户数、新增用户数、活跃用户数）
**从而**：快速了解 Realm 的用户增长与活跃趋势，无需进入多个子页面分别查看

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：Dashboard 正常展示用户指标**
```gherkin
Given 我是 realm-1 的管理员
And realm-1 下有用户和近期的登录活动
When 我访问 realm-1 的管理后台首页
Then 页面首屏显示 3 张指标卡片：总用户数、最近 7 天新增用户数、最近 7 天活跃用户数
And 每张卡片显示对应的数值
```

**场景 2：新 Realm 无数据时的空态**
```gherkin
Given 我是新创建的 realm-2 的管理员
And realm-2 下没有任何用户
When 我访问 realm-2 的管理后台首页
Then 3 张指标卡片均显示数值 0
```

---

### 故事 11：查看 Dashboard 认证趋势图 [US-RA-011]

**优先级**: P1

**【用户故事】**
**作为**：Realm Admin（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：在 Dashboard 上看到最近 30 天的认证趋势图（登录成功和失败次数）
**从而**：发现异常登录波动，及时响应安全问题

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：正常展示认证趋势**
```gherkin
Given 我是 realm-1 的管理员
And realm-1 在最近 30 天内有认证事件记录
When 我访问 realm-1 的管理后台首页
Then 页面显示最近 30 天的认证趋势图
And 图表包含两条线：登录成功次数和登录失败次数
And 图表按天聚合，X 轴为日期
```

**场景 2：无认证事件时的空态**
```gherkin
Given 我是 realm-1 的管理员
And realm-1 在最近 30 天内没有任何认证事件
When 我访问 realm-1 的管理后台首页
Then 趋势图区域显示"暂无数据"提示
```

---

### 故事 12：通过 Dashboard 快捷导航跳转 [US-RA-012]

**优先级**: P1

**【用户故事】**
**作为**：Realm Admin（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：在 Dashboard 下方保留管理功能导航入口
**从而**：从 Dashboard 快速跳转到用户管理、角色管理等子页面

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：快捷导航正常跳转**
```gherkin
Given 我是 realm-1 的管理员
When 我访问 realm-1 的管理后台首页
Then 页面下方显示快捷导航入口（Users、Roles、Permissions、Client Apps、Realms、Settings）
And 点击任一导航项可跳转到对应管理页面
```

**场景 2：导航入口不丢失**
```gherkin
Given Dashboard 页面底部包含快捷导航区域
When 管理员查看快捷导航
Then 可看到 Users、Roles、Permissions、Client Apps、Realms、Settings 6 个导航入口
And 每个导航入口的跳转目标正确
```

---

### 故事 13：配置 Realm 邮件服务 [US-RA-013]

**优先级**: P0

**【用户故事】**
**作为**：Realm Admin（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：在 Settings 页面的 Email Tab 中配置本 Realm 的邮件发送方式
**从而**：让我的 Realm 能够独立发送邮件（验证邮箱、密码重置等），不依赖全局配置

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：配置 Resend API 方式**
```gherkin
Given 我是 realm-1 的管理员
And realm-1 尚未配置邮件服务
When 我在 Settings 页面切换到 Email Tab
And 我选择 provider 为 "Resend API"
And 我填写发件地址和 API Key
And 我点击 "Save"
Then 邮件配置保存成功
And 页面显示 "Email is configured" 状态
```

**场景 2：配置 SMTP 方式**
```gherkin
Given 我是 realm-1 的管理员
And realm-1 尚未配置邮件服务
When 我选择 provider 为 "SMTP"
And 我填写 SMTP 主机、端口、加密方式、用户名和密码
And 我点击 "Save"
Then 邮件配置保存成功
And 页面显示 "Email is configured" 状态
```

**场景 3：缺少必填字段时无法保存**
```gherkin
Given 我是 realm-1 的管理员
When 我选择 provider 为 "SMTP"
And 我仅填写了 SMTP Host 而未填写其他必填字段
And 我点击 "Save"
Then 保存失败
And 页面提示缺失的必填字段
```

**场景 4：切换 provider 后清空旧配置**
```gherkin
Given 我是 realm-1 的管理员
And realm-1 已配置了 Resend API 方式
When 我切换 provider 为 "SMTP"
Then Resend 相关字段被隐藏
And SMTP 相关字段显示为空
```

---

### 故事 14：发送测试邮件 [US-RA-014]

**优先级**: P1

**【用户故事】**
**作为**：Realm Admin（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：在邮件配置保存后发送测试邮件以验证配置正确性
**从而**：确保用户能正常收到系统邮件

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：测试邮件发送成功**
```gherkin
Given 我是 realm-1 的管理员
And realm-1 已配置完整的邮件服务
When 我在 Email Tab 中点击 "Send Test Email"
And 我输入收件人地址 test@example.com
Then 系统发送测试邮件到 test@example.com
And 页面显示发送成功提示
```

**场景 2：邮件配置不完整时无法发送**
```gherkin
Given 我是 realm-1 的管理员
And realm-1 未配置邮件服务
When 我点击 "Send Test Email"
Then 系统提示 "Email is not configured"
And 不发送任何邮件
```

**场景 3：测试邮件发送失败**
```gherkin
Given 我是 realm-1 的管理员
And realm-1 已配置邮件服务但配置有误
When 我点击 "Send Test Email"
And 我输入收件人地址 test@example.com
Then 系统返回发送失败提示
And 页面显示错误信息
```

---

### 故事 15：邮件依赖的功能开关前置验证 [US-RA-015]

**优先级**: P0

**【用户故事】**
**作为**：Realm Admin（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：在未配置邮件服务时无法开启邮件依赖功能（如邮箱验证、密码重置邮件）
**从而**：避免用户因邮件不可用而无法完成注册或重置密码

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：邮件未配置时无法开启邮箱验证**
```gherkin
Given 我是 realm-1 的管理员
And realm-1 未配置邮件服务
When 我在 Settings 页面 Registration Tab 中查看 "Require Email Verification" 开关
Then 该开关处于禁用状态
And 开关旁显示提示 "Email verification requires email configuration"
```

**场景 2：邮件配置完成后可开启邮箱验证**
```gherkin
Given 我是 realm-1 的管理员
And realm-1 已配置完整的邮件服务
When 我在 Registration Tab 中开启 "Require Email Verification"
Then 开关切换为启用状态
And 保存成功
```

**场景 3：删除邮件配置后，邮箱验证自动失效**
```gherkin
Given 我是 realm-1 的管理员
And realm-1 已配置邮件服务且开启了邮箱验证
When 我删除邮件配置
Then "Require Email Verification" 开关自动变为禁用状态
And 后续用户注册不再要求邮箱验证
```

---

### 故事 16：API Key 角色管理 [US-RA-016]

**优先级**: P0

**【用户故事】**
**作为**：Realm Admin（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：在管理后台为 API Key 分配和管理角色
**从而**：控制 API Key 可执行的操作范围，实现第三方集成的权限自助配置

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：查看 API Key 已分配角色**
```gherkin
Given 我是 realm-1 的管理员
And realm-1 中存在已创建的 API Key "sdk-key-01"
When 我在 API Key 列表页查看该行
Then 该行显示已分配的角色标识
And 如果该 API Key 无角色，则显示「—」
```

**场景 2：为 API Key 分配角色**
```gherkin
Given 我是 realm-1 的管理员
And 我具备角色管理权限
And 已存在自定义角色 "sdk-manager"
When 我在 API Key 列表页点击「Roles」按钮
And 在角色对话框中选择 "sdk-manager" 角色
Then 角色立即保存
And 列表页的角色标识更新为 "sdk-manager"
And 该 API Key 可执行 "sdk-manager" 角色所包含的权限操作
```

**场景 3：清空 API Key 角色**
```gherkin
Given 我是 realm-1 的管理员
And API Key "sdk-key-01" 已分配角色 "sdk-manager"
When 我在角色对话框中取消所有角色
Then 角色保存为空
And 列表页的角色标识恢复为「—」
And 该 API Key 的权限操作全部失效
```

**场景 4：API Key 不允许绑定内置角色**
```gherkin
Given 我是 realm-1 的管理员
And 系统中存在内置角色（如 realm-admin、user）
When 我在角色对话框中尝试选择内置角色
Then 保存失败
And 提示「内置角色不能分配给 API Key」
And 该 API Key 的原有角色绑定不受影响
```

**场景 5：无角色管理权限时不可操作**
```gherkin
Given 我是 realm-1 的管理员
And 我只有 API Key 查看权限，没有角色管理权限
When 我查看 API Key 列表
Then 我能看到每行的角色标识
And 「Roles」按钮不可见
```

**场景 6：角色变更后权限立即生效**
```gherkin
Given API Key "sdk-key-01" 刚被分配了 "sdk-manager" 角色
When 该 API Key 通过外部接口调用需要权限的操作
Then 权限检查立即反映新角色
```

---

### 故事 17：创建 API Key 时绑定角色 [US-RA-017]

**优先级**: P0

**【用户故事】**
**作为**：Realm Admin（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：在创建 API Key 时可选地为其分配角色
**从而**：API Key 创建后即可立即使用，无需再手动分配角色

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：创建 API Key 时选择角色**
```gherkin
Given 我是 realm-1 的管理员
And 我具备 API Key 管理和角色管理权限
When 我在创建 API Key 表单中填写名称和过期时间
And 我在角色选择区选择 "sdk-manager" 角色
And 我提交表单
Then API Key 创建成功
And 明文 Key 正常展示
And "sdk-manager" 角色自动绑定到该 API Key
```

**场景 2：创建 API Key 时不选角色**
```gherkin
Given 我是 realm-1 的管理员
When 我在创建 API Key 表单中只填写名称和过期时间，不选择角色
And 我提交表单
Then API Key 创建成功
And 明文 Key 正常展示
And 该 API Key 无角色绑定
```

**场景 3：角色绑定部分失败时保留 Key**
```gherkin
Given 我是 realm-1 的管理员
When 我创建 API Key 并选择了角色
And API Key 创建成功但角色绑定失败
Then 明文 Key 仍然展示
And 页面提示「API Key 已创建，但角色绑定失败，可稍后在列表页管理角色」
```

**场景 4：无角色管理权限时不显示角色选择器**
```gherkin
Given 我是 realm-1 的管理员
And 我有 API Key 管理权限但没有角色管理权限
When 我打开创建 API Key 表单
Then 表单中不显示角色选择区
And 我仍可正常创建 API Key
```

---

### 故事 18：API Key 按 Client App 隔离 [US-RA-018]

**优先级**: P0

**【用户故事】**
**作为**：Realm Admin（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：创建 API Key 时选择其代表的 Client App
**从而**：让第三方集成只能操作所属 Client App 的资源，避免同一 Realm 内跨应用越权

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：创建 API Key 时选择 Client App**
```gherkin
Given 我是 realm-1 的管理员
And realm-1 中存在 Client App "mobile-app"
When 我在创建 API Key 表单中填写名称
And 我在 Client App 选择器中选择 "mobile-app"
And 我提交表单
Then API Key 创建成功
And API Key 列表显示该 Key 绑定到 "mobile-app"
```

**场景 2：未选择 Client App 时使用默认范围**
```gherkin
Given 我是 realm-1 的管理员
When 我在创建 API Key 表单中未选择 Client App
And 我提交表单
Then API Key 创建成功
And 该 API Key 使用默认的 "admin-api-client" 范围
```

**场景 3：普通 Client App API Key 不能访问其他应用资源**
```gherkin
Given API Key "mobile-key" 绑定到 Client App "mobile-app"
And realm-1 中还存在 Client App "web-app"
When "mobile-key" 尝试访问 "web-app" 的订阅或积分资源
Then 系统拒绝访问
And 不返回 "web-app" 的资源数据
```

**场景 4：admin-api-client API Key 保持 Realm 级访问**
```gherkin
Given API Key "admin-api-key" 使用默认的 "admin-api-client" 范围
And realm-1 中存在多个 Client App
When "admin-api-key" 访问任意 Client App 的订阅或积分资源
Then 系统按该 API Key 的角色权限正常处理请求
```

**场景 5：禁用 Client App 后其 API Key 不可用**
```gherkin
Given API Key "mobile-key" 绑定到 Client App "mobile-app"
When 管理员禁用 "mobile-app"
Then "mobile-key" 无法继续通过外部接口认证
And 系统返回 Client App 已禁用的错误
```

---

### 故事 19：查看并撤销指定用户的活跃会话 [US-RA-020]

**优先级**: P1

**【用户故事】**
**作为**：Realm Admin（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：在用户管理页面查看某个用户当前所有活跃会话，并可以选择性地撤销其中一个会话，或一键撤销该用户的全部活跃会话
**从而**：在发现账号被盗、设备丢失或需要强制用户重新认证时，能即时终止该用户在指定设备/Client App 或所有设备上的访问，而不必等待其令牌自然过期

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：查看某用户的活跃会话列表**
```gherkin
Given Realm Admin 在用户管理页面选中一个存在多个活跃会话（不同设备或不同 Client App）的用户
When 管理员打开该用户的会话管理入口
Then 管理员能看到该用户当前所有活跃会话的列表
And 列表展示每个会话的标识信息，至少包含：所属 Client App、登录时的设备/浏览器（User-Agent）、登录来源 IP、登录时间
And 列表仅包含本 Realm 内的会话，不泄露其他 Realm 的数据
```

**场景 2：撤销单个指定会话**
```gherkin
Given Realm Admin 正在查看某用户的活跃会话列表
When 管理员对其中某一个会话执行撤销操作并确认
Then 仅该被选中的会话被终止
And 该会话对应的设备/Client App 在下一次请求时被判定为未登录
And 该用户的其他活跃会话不受影响，可继续访问
```

**场景 3：一键撤销某用户的全部会话**
```gherkin
Given Realm Admin 正在查看某用户的活跃会话列表，列表中有多个活跃会话
When 管理员执行"撤销全部会话"操作并确认
Then 该用户在本 Realm 的所有活跃会话被立即终止
And 该用户在所有设备/Client App 上的下一次请求都被判定为未登录
And 撤销操作不改变该用户的账号状态（账号仍处于操作前的 Normal/其他状态），用户可重新登录
```

**场景 4：跨 Realm 访问被拒绝**
```gherkin
Given Realm Admin 仅拥有本 Realm 的管理权限
When 管理员尝试查看或撤销其他 Realm 用户的会话
Then 操作被拒绝，不返回其他 Realm 的会话数据
```

**场景 5：权限不足时不可见**
```gherkin
Given 一个只拥有用户查看权限而没有会话管理权限的账号
When 该账号进入用户管理页面
Then 会话管理入口和撤销操作按钮不可见或不可用
```

---

### 故事 20：禁用用户时即时撤销其全部活跃会话 [US-RA-021]

**优先级**: P1

**【用户故事】**
**作为**：Realm Admin（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：当我把一个用户的状态变更为禁用（Forbidden）时，该用户当前的全部活跃会话被立即撤销
**从而**：被禁用的账号无法继续使用已登录的会话访问系统，避免"禁用后仍可访问"的安全窗口，确保禁用即下线

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：禁用用户触发即时下线**
```gherkin
Given 一个处于 Normal 状态且在多个设备/Client App 上有活跃会话的用户
When Realm Admin 通过用户编辑操作将该用户状态变更为 Forbidden
Then 该用户的全部活跃会话被立即撤销（无需等待 access token 或 refresh token 自然过期）
And 该用户在所有设备上的下一次请求都被判定为未登录
And 该用户此后尝试登录被拒绝并收到账号被禁用的明确提示
```

**场景 2：禁用操作在其他变更中仍保证下线**
```gherkin
Given Realm Admin 在编辑用户时同时修改昵称并将状态改为 Forbidden
When 管理员保存变更
Then 昵称变更生效，且由于状态变为 Forbidden，该用户全部活跃会话被立即撤销
And 会话撤销不因同时发生其他字段变更而被遗漏
```

**场景 3：未被禁用的状态变更不撤销会话**
```gherkin
Given 一个处于 Normal 状态且有活跃会话的用户
When Realm Admin 编辑该用户但不改变其状态（或仅在非 Forbidden 的状态间变更）
Then 该用户的活跃会话不被撤销，可继续访问
```
