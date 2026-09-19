# Third-Party App 用户故事

> 角色定义见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)

## 用户故事

### 故事 1：通过 SDK 管理 Realm [US-TP-012]

**优先级**: P1

**【用户故事】**
**作为**：Third-Party App（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：通过 SDK 编程式管理 Realm（创建、查询列表、查询详情）
**从而**：在我的平台为不同组织自动开通和管理独立的认证服务

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：创建 Realm 成功**
```gherkin
Given SDK 已使用 admin realm 的 API Key 初始化，且具备创建 Realm 的权限
When 调用 SDK 提供名称和管理员信息创建 Realm
Then 返回新创建的 Realm 信息（包含 Realm ID 和名称）
```

**场景 2：查询 Realm 列表**
```gherkin
Given SDK 已使用 API Key 初始化，且具备查看 Realm 的权限
And 系统中存在多个可见 Realm
When 调用 SDK 查询 Realm 列表
Then 返回所有可见 Realm 的列表（包含 ID、名称、状态等基本信息）
```

**场景 3：查询 Realm 详情**
```gherkin
Given SDK 已使用 API Key 初始化，且具备查看 Realm 的权限
And 指定 ID 的 Realm 存在
When 调用 SDK 查询该 Realm 详情
Then 返回 Realm 的完整信息（名称、状态、管理员、创建时间等）
```

**场景 4：创建 Realm 权限不足**
```gherkin
Given SDK 使用的 API Key 不具备创建 Realm 的权限或不在 admin realm 中
When 调用 SDK 创建 Realm
Then 返回权限不足错误
```

**场景 5：Realm 不存在**
```gherkin
Given 指定 ID 的 Realm 不存在
When 调用 SDK 查询该 Realm 详情
Then 返回未找到错误
```

---

### 故事 2：通过 SDK 管理用户 [US-TP-013]

**优先级**: P0

**【用户故事】**
**作为**：Third-Party App（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：通过 SDK 在指定 Realm 中管理用户（创建、查询列表、查询详情）
**从而**：在我的应用中自动完成用户注册开通和信息查询

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：创建用户成功**
```gherkin
Given SDK 已使用 API Key 初始化，且具备目标 Realm 的用户创建权限
When 调用 SDK 提供邮箱、密码和昵称创建用户
Then 返回新用户信息（包含用户 ID、邮箱、状态）
```

**场景 2：查询用户列表**
```gherkin
Given SDK 已使用 API Key 初始化，且具备目标 Realm 的用户查看权限
And 目标 Realm 中存在多个用户
When 调用 SDK 查询用户列表
Then 返回用户列表（包含用户 ID、邮箱、昵称、状态）
```

**场景 3：查询用户详情**
```gherkin
Given SDK 已使用 API Key 初始化，且具备目标 Realm 的用户查看权限
And 指定 ID 的用户存在于目标 Realm 中
When 调用 SDK 查询用户详情
Then 返回用户完整信息（ID、邮箱、昵称、状态、创建时间）
```

**场景 4：邮箱重复**
```gherkin
Given 目标 Realm 中已存在相同邮箱的用户
When 调用 SDK 使用该邮箱创建用户
Then 返回明确的错误提示，说明邮箱已被注册
```

**场景 5：跨 Realm 拒绝**
```gherkin
Given API Key 具备 Realm-A 的用户管理权限
When 调用 SDK 尝试在 Realm-B 中创建或查询用户
Then 返回权限不足错误
```

**场景 6：缺少管理权限**
```gherkin
Given SDK 使用的 API Key 不具备目标 Realm 的用户管理权限
When 调用 SDK 在目标 Realm 中创建或查询用户
Then 返回权限不足错误
```

---

### 故事 3：通过 SDK 管理 Client App [US-TP-014]

**优先级**: P1

**【用户故事】**
**作为**：Third-Party App（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：通过 SDK 在指定 Realm 中管理 Client App（创建、查询列表、查询详情）
**从而**：自动注册新的接入应用并查询已有应用信息

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：创建 Client App 成功**
```gherkin
Given SDK 已使用 API Key 初始化，且具备目标 Realm 的客户端创建权限
When 调用 SDK 提供名称和回调地址创建 Client App
Then 返回新 Client App 信息（包含 Client ID、Client Secret、名称）
```

**场景 2：查询 Client App 列表**
```gherkin
Given SDK 已使用 API Key 初始化，且具备目标 Realm 的客户端查看权限
And 目标 Realm 中存在多个 Client App
When 调用 SDK 查询 Client App 列表
Then 返回 Client App 列表（包含 ID、名称、状态、创建时间）
```

**场景 3：查询 Client App 详情**
```gherkin
Given SDK 已使用 API Key 初始化，且具备目标 Realm 的客户端查看权限
And 指定 ID 的 Client App 存在于目标 Realm 中
When 调用 SDK 查询 Client App 详情
Then 返回 Client App 完整信息（ID、名称、回调地址、状态、Client ID）
```

**场景 4：缺少必要参数**
```gherkin
Given 调用参数缺少名称
When 调用 SDK 创建 Client App
Then 返回参数校验错误
```

**场景 5：Client App 不存在**
```gherkin
Given 指定 ID 的 Client App 不存在
When 调用 SDK 查询 Client App 详情
Then 返回未找到错误
```

**场景 6：缺少管理权限**
```gherkin
Given SDK 使用的 API Key 不具备目标 Realm 的客户端管理权限
When 调用 SDK 创建或查询 Client App
Then 返回权限不足错误
```

---

### 故事 4：通过 SDK 发放积分 [US-TP-017]

**优先级**: P0

**【用户故事】**
**作为**：Third-Party App（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：通过 SDK 向指定用户发放积分
**从而**：在我的平台实现积分奖励、等级提升等运营能力

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：通过 SDK 发放积分成功**
```gherkin
Given SDK 已使用 API Key 初始化，且具备积分发放权限
And 指定的用户存在于目标 Realm
And 目标 Realm 中已存在一个启用的积分 Bucket
When 调用 SDK 向该用户发放 100 积分
And 指定发放目标为该积分 Bucket
And 设置发放原因为 "Level up bonus"
And 设置有效期为 30 天
Then 返回发放成功
And 该 Bucket 内用户的积分余额增加 100
And 该批积分将在 30 天后过期
```

**场景 1b：未指定积分 Bucket 被拒**
```gherkin
Given SDK 已使用 API Key 初始化，且具备积分发放权限
And 指定的用户存在于目标 Realm
When 调用 SDK 向该用户发放积分但未指定目标积分 Bucket
Then 返回参数校验错误（错误码 grant_bucket_required）
And 提示每笔发放必须指定目标积分 Bucket
```

**场景 2：发放积分不设置有效期（永久有效）**
```gherkin
Given SDK 已使用 API Key 初始化，且具备积分发放权限
When 调用 SDK 向用户发放积分并指定目标积分 Bucket
And 不设置有效期
Then 返回发放成功
And 该批积分永久有效
```

**场景 3：积分数量越界被拒**
```gherkin
Given SDK 已使用 API Key 初始化
When 调用 SDK 发放积分且数量为 0、负数或超过 1,000,000
Then 返回参数校验错误（错误码 invalid_amount）
And 提示积分数量必须为正数且不超过 1,000,000
```

**场景 4：缺少积分发放权限**
```gherkin
Given SDK 使用的 API Key 不具备积分发放权限
When 调用 SDK 发放积分
Then 返回权限不足错误
```

**场景 5：用户不存在**
```gherkin
Given SDK 已使用 API Key 初始化
And 指定用户不存在
When 调用 SDK 向该用户发放积分
Then 返回未找到错误
```

**场景 6：跨 Realm 操作被拒绝**
```gherkin
Given SDK 使用的 API Key 属于 realm-1
And 目标用户属于 realm-2
When 调用 SDK 向该用户发放积分
Then 返回权限不足错误
```

---

## 相关文档

- **SDK 增强**: [docs/prd/integration/sdk.md](/docs/prd/integration/sdk.md)
- **Client App 管理**: [docs/prd/integration/client-app.md](/docs/prd/integration/client-app.md)
- **积分发放**: [docs/prd/billing/points.md](/docs/prd/billing/points.md) - 积分系统（含发放功能）
