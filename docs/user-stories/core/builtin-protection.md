# 默认角色和权限保护 用户故事

> 角色定义见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)

## 用户故事

### 故事 1：内置角色和权限保护 [US-BP-001]

**优先级**: P0

**【用户故事】**
**作为**：Realm Admin（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：默认的角色和权限不能被删除或修改
**从而**：确保系统核心功能不受误操作影响，保证系统稳定性

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：不能删除内置角色**
```gherkin
Given 系统中有以下内置角色：realm-admin、user
When 管理员尝试删除这些角色
Then 系统拒绝删除并提示 "Cannot delete built-in role"
```

**场景 2：不能修改内置角色名称**
```gherkin
Given 系统中有内置角色 realm-admin
When 管理员尝试修改角色名称为 "admin-v2"
Then 系统拒绝修改并提示 "Cannot change built-in role name"
```

**场景 3：可以修改内置角色描述**
```gherkin
Given 系统中有内置角色 realm-admin
When 管理员只修改角色描述
Then 更新成功
And 角色名称保持不变
```

**场景 4：可以删除自定义角色**
```gherkin
Given 系统中有自定义角色 content-admin（非内置）
When 管理员删除该角色
Then 删除成功
And 角色已从列表中移除
```

**场景 5：不能删除内置权限**
```gherkin
Given 系统中有以下内置权限：users.manage、points.view
When 管理员尝试删除这些权限
Then 系统拒绝删除并提示 "Cannot delete built-in permission"
```

**场景 6：内置标识在界面上可见**
```gherkin
Given 管理员查看角色或权限列表
When 系统展示列表
Then 内置角色和权限显示"内置"标识
And 内置项的删除按钮处于禁用状态
```

**场景 7：不能从任何内置角色中移除内置权限**
```gherkin
Given 系统中有内置角色 realm-admin 或 user，以及内置权限 users.manage
When 管理员尝试从任一内置角色中移除内置权限
Then 系统拒绝操作并提示 "Cannot remove built-in permission from built-in role"
And 内置权限的复选框处于禁用状态（针对所有内置角色）
```

**场景 8：不能修改内置权限定义**
```gherkin
Given 系统中有内置权限 users.manage
When 管理员尝试修改权限的名称、资源或操作
Then 系统拒绝修改并提示 "Cannot modify built-in permission definition"
And 内置权限的编辑和删除按钮处于禁用状态
```

**场景 9：可以为内置角色添加自定义权限**
```gherkin
Given 系统中有内置角色 realm-admin
And 有自定义权限 reports.view（非内置）
When 管理员将 reports.view 分配给 realm-admin
Then 分配成功
And 管理员也可以从 realm-admin 中移除该自定义权限
```
