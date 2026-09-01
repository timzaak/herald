# MCP Server 用户故事

> 角色定义见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)

## 用户故事

### 故事 1：把 Herald 接入 AI agent 客户端 [US-MCP-001]

**优先级**: P0

**【用户故事】**
**作为**：Third-Party App（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：在我的 AI agent 客户端（如 Claude Code、Cursor、VS Code）中按官方接入文档配置 Herald 的 MCP 服务与 Client API Key，并确认连接成功
**从而**：让 agent 以结构化工具语义查询我的 Herald 租户数据，不再依赖读接口文档后手写 REST 调用

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：配置后成功连接**
```gherkin
Given 我已创建一个绑定了查询类角色的 Client API Key
And 我按官方接入文档在 agent 客户端中完成 Herald MCP 服务配置
When agent 客户端发起连接
Then 连接成功
And 我在 agent 客户端中看到 Herald 提供的全部查询工具清单
```

**场景 2：API Key 无效**
```gherkin
Given 我在 agent 客户端中配置了一个无效的 API Key
When agent 客户端发起连接
Then 连接被拒绝
And 我收到明确的鉴权失败提示
```

**场景 3：API Key 或其绑定的 Client App 已被禁用**
```gherkin
Given 我配置的 API Key 已被禁用，或该 API Key 绑定的 Client App 已被禁用
When agent 客户端发起连接
Then 连接被拒绝
```

**场景 4：连通性自检**
```gherkin
Given agent 客户端已成功连接 Herald MCP 服务
When 我让 agent 调用一个最小查询工具（如查询本租户配置状态）
Then 工具返回成功结果
And 连接与鉴权链路确认可用
```

---

### 故事 2：通过 agent 查询用户 [US-MCP-002]

**优先级**: P1

**【用户故事】**
**作为**：Third-Party App（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：让 agent 查询我租户内的用户列表和指定用户的详情
**从而**：在开发与运营对话中直接获取用户信息，无需切换到管理后台

**【验收标准】**

**场景 1：查询用户列表**
```gherkin
Given agent 已通过具备用户查看权限的 API Key 接入
And 本租户内存在多个用户
When 我让 agent 查询用户列表
Then 返回本租户内的用户列表（含用户标识、邮箱、状态等基本信息）
```

**场景 2：查询用户详情**
```gherkin
Given agent 已通过具备用户查看权限的 API Key 接入
And 指定用户存在于本租户
When 我让 agent 查询该用户详情
Then 返回该用户的详细信息（标识、邮箱、昵称、状态、创建时间）
```

**场景 3：缺少用户查看权限**
```gherkin
Given agent 使用的 API Key 未绑定任何具备用户查看权限的角色
When agent 调用用户查询工具
Then 工具返回 agent 可读的权限不足错误
And 不返回任何用户数据
```

**场景 4：其他租户的用户不可见**
```gherkin
Given agent 使用的 API Key 属于租户 A
And 指定用户仅存在于租户 B
When agent 尝试查询该用户
Then 返回未找到错误
And 不返回任何用户数据
```

**场景 5：用户不存在**
```gherkin
Given agent 已通过具备用户查看权限的 API Key 接入
And 指定用户不存在于本租户
When 我让 agent 查询该用户详情
Then 返回未找到错误
```

---

### 故事 3：通过 agent 查询积分余额 [US-MCP-003]

**优先级**: P1

**【用户故事】**
**作为**：Third-Party App（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：让 agent 查询本租户指定用户的积分余额
**从而**：在运营对话中快速核对用户可用积分，无需打开管理后台

**【验收标准】**

**场景 1：查询积分余额成功**
```gherkin
Given agent 已通过具备积分查看权限的 API Key 接入
And 指定用户存在于本租户
When 我让 agent 查询该用户的积分余额
Then 返回该用户的积分余额信息（含余额数量）
```

**场景 2：缺少积分查看权限**
```gherkin
Given agent 使用的 API Key 未绑定任何具备积分查看权限的角色
When agent 调用积分余额查询工具
Then 工具返回 agent 可读的权限不足错误
And 不返回任何余额数据
```

**场景 3：用户不存在**
```gherkin
Given agent 已通过具备积分查看权限的 API Key 接入
And 指定用户不存在于本租户
When 我让 agent 查询该用户的积分余额
Then 返回未找到错误
```

---

### 故事 4：通过 agent 查询积分交易流水 [US-MCP-004]

**优先级**: P1

**【用户故事】**
**作为**：Third-Party App（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：让 agent 查询本租户指定用户的积分交易历史
**从而**：在排查积分问题或运营分析时直接获取变动记录

**【验收标准】**

**场景 1：查询交易流水成功**
```gherkin
Given agent 已通过具备积分查看权限的 API Key 接入
And 指定用户存在积分交易记录
When 我让 agent 查询该用户的积分交易历史
Then 返回交易记录列表（含变动数量、类型、时间等基本信息）
```

**场景 2：缺少积分查看权限**
```gherkin
Given agent 使用的 API Key 未绑定任何具备积分查看权限的角色
When agent 调用积分流水查询工具
Then 工具返回 agent 可读的权限不足错误
And 不返回任何交易数据
```

**场景 3：用户不存在**
```gherkin
Given 指定用户不存在于本租户
When 我让 agent 查询该用户的积分交易历史
Then 返回未找到错误
```

---

### 故事 5：通过 agent 查询审计日志 [US-MCP-005]

**优先级**: P1

**【用户故事】**
**作为**：Third-Party App（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：让 agent 查询本租户的审计日志
**从而**：在安全排查和运营对话中直接定位关键操作记录

**【验收标准】**

**场景 1：查询审计日志成功**
```gherkin
Given agent 已通过具备审计查看权限的 API Key 接入
And 本租户存在审计日志记录
When 我让 agent 查询审计日志
Then 返回审计记录列表（含操作者、动作、时间等基本信息）
```

**场景 2：缺少审计查看权限**
```gherkin
Given agent 使用的 API Key 未绑定任何具备审计查看权限的角色
When agent 调用审计日志查询工具
Then 工具返回 agent 可读的权限不足错误
And 不返回任何审计数据
```

**场景 3：其他租户的审计数据不可见**
```gherkin
Given agent 使用的 API Key 属于租户 A
And 租户 B 存在审计日志记录
When 我让 agent 查询审计日志
Then 仅返回租户 A 的审计记录
And 租户 B 的任何审计数据不出现在结果中
```

---

### 故事 6：通过 agent 查询 Realm 配置状态 [US-MCP-006]

**优先级**: P1

**【用户故事】**
**作为**：Third-Party App（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：让 agent 查询本租户的配置状态概览（登录方式、安全能力等配置的启用情况）
**从而**：在接入排障与日常运营对话中快速确认租户配置现状

**【验收标准】**

**场景 1：查询配置状态成功**
```gherkin
Given agent 已通过具备设置查看权限的 API Key 接入
When 我让 agent 查询本租户配置状态
Then 返回本租户的配置状态概览（含各配置项的启用状态）
```

**场景 2：缺少设置查看权限**
```gherkin
Given agent 使用的 API Key 未绑定任何具备设置查看权限的角色
When agent 调用配置状态查询工具
Then 工具返回 agent 可读的权限不足错误
And 不返回任何配置数据
```

---

## 相关文档

- **PRD**: [docs/prd/integration/mcp-server.md](/docs/prd/integration/mcp-server.md)
- **API Key 体系（前置能力）**: [docs/prd/integration/api-key-roles.md](/docs/prd/integration/api-key-roles.md)
- **决策账本**: `.ai/decision-log/mcp-server.md`
- **技术预研**: `.ai/tech-research/mcp-server.md`
