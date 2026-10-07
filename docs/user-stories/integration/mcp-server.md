# MCP Server 用户故事

> 角色定义见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)

## 用户故事

### 故事 1：把 Herald 接入 AI agent 客户端（浏览器授权） [US-MCP-001]

**优先级**: P0

**【用户故事】**
**作为**：第三方应用开发者（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：在我的 AI agent 客户端（如 Claude Code、Cursor、VS Code）中只填入 Herald MCP 服务地址即可发起连接，并经浏览器用我的 Herald 账号完成登录授权
**从而**：无需预先创建或分发任何静态凭证，让 agent 以我的用户身份安全地查询 Herald 数据

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：零配置接入并完成浏览器授权**
```gherkin
Given 我是一个 Herald 用户
And 我按官方接入文档在 agent 客户端中填入 Herald MCP 服务地址（无需预先创建任何凭证）
And 我使用全新客户端配置，没有手工补填客户端身份信息
When agent 客户端发起连接
Then 客户端引导我到 Herald 浏览器页面完成登录
And 我完成登录后授权即完成，连接成功
And 我在 agent 客户端中看到 Herald 提供的全部查询工具清单
And 我可以按本人身份调用已获许可的查询工具
```

**场景 2：取消浏览器授权**
```gherkin
Given agent 客户端已拉起 Herald 浏览器授权页面
When 我未完成登录即关闭页面或放弃授权
Then 连接失败
And agent 客户端收到明确的鉴权失败提示
```

**场景 3：授权失效后的表现**
```gherkin
Given 我的授权已被撤销（如账号被禁用、被强制下线，或管理员禁用了内置 MCP 客户端）
When agent 客户端继续调用工具
Then 调用被拒绝
And 我收到需要重新完成浏览器授权的明确提示
```

**场景 4：长会话不断线**
```gherkin
Given agent 客户端已成功连接并完成过工具调用
And 连接使用时间超过访问凭证的单次有效期
And 会话仍在允许续期的绝对有效上限内
When agent 客户端在后台自动续期凭证
Then 连接保持可用
And 我无需重新完成浏览器授权
```

**场景 5：浏览器会话凭证不能直连 MCP**
```gherkin
Given 我持有 Herald 管理台或用户中心的浏览器会话凭证
When 我把该凭证直接配置到 agent 客户端访问 Herald MCP 服务
Then 访问被拒绝
And 不返回任何查询数据
```

**场景 6：MCP 凭证不能用于管理台或用户中心**
```gherkin
Given 我持有经 MCP 浏览器授权取得的凭证
When 我尝试用该凭证访问 Herald 管理台或用户中心的受保护内容
Then 访问被拒绝
And 不返回任何受保护数据
```

**场景 7：旧指南的两种 API Key 配置均不能接入**
```gherkin
Given 我持有一个在 Herald 其他集成中仍有效的 Client API Key
When 我分别按旧接入指南的两种 API Key 配置方式连接 Herald MCP 服务
Then 两种连接均被拒绝
And 不返回任何查询数据
And 客户端引导我完成浏览器授权
```

---

### 故事 2：通过 agent 查询用户 [US-MCP-002]

**优先级**: P1

**【用户故事】**
**作为**：Realm Admin（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：让 agent 查询我租户内的用户列表和指定用户的详情
**从而**：在开发与运营对话中直接获取用户信息，无需切换到管理后台

**【验收标准】**

**场景 1：查询用户列表**
```gherkin
Given agent 已由持有用户查看权限的 Herald 用户完成授权接入
And 本租户内存在多个用户
When 我让 agent 查询用户列表
Then 返回本租户内的用户列表（含用户标识、邮箱、状态等基本信息）
```

**场景 2：查询用户详情**
```gherkin
Given agent 已由持有用户查看权限的 Herald 用户完成授权接入
And 指定用户存在于本租户
When 我让 agent 查询该用户详情
Then 返回该用户的详细信息（标识、邮箱、昵称、状态、创建时间）
```

**场景 3：授权用户缺少用户查看权限**
```gherkin
Given 完成授权的用户未持有任何具备用户查看权限的角色
When agent 调用用户查询工具
Then 工具返回 agent 可读的权限不足错误
And 不返回任何用户数据
And 提示需由管理员授予用户查看权限，不提示重新登录或扩大本人查询许可
```

**场景 4：其他租户的用户不可见**
```gherkin
Given 完成授权的用户属于租户 A
And 指定用户仅存在于租户 B
When agent 尝试查询该用户
Then 返回未找到错误
And 不返回任何用户数据
```

**场景 5：用户不存在**
```gherkin
Given agent 已由持有用户查看权限的 Herald 用户完成授权接入
And 指定用户不存在于本租户
When 我让 agent 查询该用户详情
Then 返回未找到错误
```

---

### 故事 3：通过 agent 查询积分余额 [US-MCP-003]

**优先级**: P1

**【用户故事】**
**作为**：Realm Admin（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：让 agent 查询本租户指定用户的积分余额
**从而**：在运营对话中快速核对用户可用积分，无需打开管理后台

**【验收标准】**

**场景 1：查询积分余额成功**
```gherkin
Given agent 已由持有积分查看权限的 Herald 用户完成授权接入
And 指定用户存在于本租户
When 我让 agent 查询该用户的积分余额
Then 返回该用户的积分余额信息（含余额数量）
And 余额汇总本租户范围内该用户的积分账户，不按连接客户端缩小范围
```

**场景 2：授权用户缺少积分查看权限**
```gherkin
Given 完成授权的用户未持有任何具备积分查看权限的角色
When agent 调用积分余额查询工具
Then 工具返回 agent 可读的权限不足错误
And 不返回任何余额数据
And 提示需由管理员授予积分查看权限，不提示重新登录或扩大本人查询许可
```

**场景 3：用户不存在**
```gherkin
Given agent 已由持有积分查看权限的 Herald 用户完成授权接入
And 指定用户不存在于本租户
When 我让 agent 查询该用户的积分余额
Then 返回未找到错误
```

---

### 故事 4：通过 agent 查询积分交易流水 [US-MCP-004]

**优先级**: P1

**【用户故事】**
**作为**：Realm Admin（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：让 agent 查询本租户指定用户的积分交易历史
**从而**：在排查积分问题或运营分析时直接获取变动记录

**【验收标准】**

**场景 1：查询交易流水成功**
```gherkin
Given agent 已由持有积分查看权限的 Herald 用户完成授权接入
And 指定用户存在积分交易记录
When 我让 agent 查询该用户的积分交易历史
Then 返回交易记录列表（含变动数量、类型、时间等基本信息）
```

**场景 2：授权用户缺少积分查看权限**
```gherkin
Given 完成授权的用户未持有任何具备积分查看权限的角色
When agent 调用积分流水查询工具
Then 工具返回 agent 可读的权限不足错误
And 不返回任何交易数据
And 提示需由管理员授予积分查看权限，不提示重新登录或扩大本人查询许可
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
**作为**：Realm Admin（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：让 agent 查询本租户的审计日志
**从而**：在安全排查和运营对话中直接定位关键操作记录

**【验收标准】**

**场景 1：查询审计日志成功**
```gherkin
Given agent 已由持有审计查看权限的 Herald 用户完成授权接入
And 本租户存在审计日志记录
When 我让 agent 查询审计日志
Then 返回审计记录列表（含操作者、动作、时间等基本信息）
```

**场景 2：授权用户缺少审计查看权限**
```gherkin
Given 完成授权的用户未持有任何具备审计查看权限的角色
When agent 调用审计日志查询工具
Then 工具返回 agent 可读的权限不足错误
And 不返回任何审计数据
And 提示需由管理员授予审计查看权限，不提示重新登录或扩大本人查询许可
```

**场景 3：其他租户的审计数据不可见**
```gherkin
Given 完成授权的用户属于租户 A
And 租户 B 存在审计日志记录
When 我让 agent 查询审计日志
Then 仅返回租户 A 的审计记录
And 租户 B 的任何审计数据不出现在结果中
```

---

### 故事 6：通过 agent 查询 Realm 配置状态 [US-MCP-006]

**优先级**: P1

**【用户故事】**
**作为**：Realm Admin（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：让 agent 查询本租户的配置状态概览（登录方式、安全能力等配置的启用情况）
**从而**：在接入排障与日常运营对话中快速确认租户配置现状

**【验收标准】**

**场景 1：查询配置状态成功**
```gherkin
Given agent 已由持有设置查看权限的 Herald 用户完成授权接入
When 我让 agent 查询本租户配置状态
Then 返回本租户的配置状态概览（含各配置项的启用状态）
```

**场景 2：授权用户缺少设置查看权限**
```gherkin
Given 完成授权的用户未持有任何具备设置查看权限的角色
When agent 调用配置状态查询工具
Then 工具返回 agent 可读的权限不足错误
And 不返回任何配置数据
And 提示需由管理员授予设置查看权限，不提示重新登录或扩大本人查询许可
```

---

### 故事 7：通过 agent 查询我的资料 [US-MCP-007]

**优先级**: P1

**【用户故事】**
**作为**：Regular User（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：让 agent 查询我自己的账号资料
**从而**：在对话中直接确认我在 Herald 的账号信息，无需打开用户中心

**【验收标准】**

**场景 1：查询我的资料成功**
```gherkin
Given agent 已由我的 Herald 账号完成授权接入
And 授权包含读取本人资料所需的许可范围
And 我的账号未被管理员授予查询角色
When 我让 agent 查询我的资料
Then 返回我的资料信息（标识、邮箱、昵称、状态、创建时间）
```

**场景 2：无法借本人视角工具查询他人**
```gherkin
Given agent 已由我的 Herald 账号完成授权接入
When 我让 agent 查询另一个用户的资料
Then 本人视角工具不接受指定其他用户，无法完成该查询
And 不返回任何他人数据
```

**场景 3：授权许可范围不足**
```gherkin
Given 我完成 MCP 授权时未包含读取本人资料所需的许可范围
When agent 调用我的资料查询工具
Then 查询被拒绝，客户端收到可识别的授权不足提示
And 提示指明重新授权所需的许可范围
And 不返回任何资料数据
```

---

### 故事 8：通过 agent 查询我的积分余额 [US-MCP-008]

**优先级**: P1

**【用户故事】**
**作为**：Regular User（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：让 agent 查询我自己的积分余额
**从而**：在对话中直接确认可用积分，无需打开用户侧页面

**【验收标准】**

**场景 1：查询我的积分余额成功**
```gherkin
Given agent 已由我的 Herald 账号完成授权接入
And 授权包含读取本人积分余额所需的许可范围
And 我的账号未被管理员授予查询角色
When 我让 agent 查询我的积分余额
Then 返回我的积分余额信息（含余额数量）
```

**场景 2：授权许可范围不足**
```gherkin
Given 我完成 MCP 授权时未包含查询积分的许可范围
When agent 调用我的积分余额查询工具
Then 查询被拒绝，客户端收到可识别的授权不足提示
And 提示指明重新授权所需的许可范围
And 不返回任何余额数据
```

**场景 3：无法借本人视角工具查询他人余额**
```gherkin
Given agent 已由我的 Herald 账号完成授权接入并获得本人积分余额的查询许可
When 我让 agent 用本人视角工具查询另一个用户的积分余额
Then 本人视角工具不接受指定其他用户，无法完成该查询
And 不返回任何他人余额数据
```

---

### 故事 9：通过 agent 查询我的积分交易流水 [US-MCP-009]

**优先级**: P1

**【用户故事】**
**作为**：Regular User（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：让 agent 查询我自己的积分交易历史
**从而**：在排查自己的积分问题或核对消费时直接获取变动记录

**【验收标准】**

**场景 1：查询我的交易流水成功**
```gherkin
Given agent 已由我的 Herald 账号完成授权接入
And 授权包含读取本人积分流水所需的许可范围
And 我的账号未被管理员授予查询角色
And 我存在积分交易记录
When 我让 agent 查询我的积分交易历史
Then 返回我的交易记录列表（含变动数量、类型、时间等基本信息）
```

**场景 2：无交易记录时返回空列表**
```gherkin
Given agent 已由我的 Herald 账号完成授权接入
And 授权包含读取本人积分流水所需的许可范围
And 我没有任何积分交易记录
When 我让 agent 查询我的积分交易历史
Then 返回空列表
And 不视为错误
```

**场景 3：授权许可范围不足**
```gherkin
Given 我完成 MCP 授权时未包含读取本人积分流水所需的许可范围
When agent 调用我的积分流水查询工具
Then 查询被拒绝，客户端收到可识别的授权不足提示
And 提示指明重新授权所需的许可范围
And 不返回任何交易数据
```

**场景 4：无法借本人视角工具查询他人流水**
```gherkin
Given agent 已由我的 Herald 账号完成授权接入并获得本人积分流水的查询许可
When 我让 agent 用本人视角工具查询另一个用户的积分交易历史
Then 本人视角工具不接受指定其他用户，无法完成该查询
And 不返回任何他人交易数据
```

---

### 故事 10：通过 agent 查询我的订阅 [US-MCP-010]

**优先级**: P1

**【用户故事】**
**作为**：Regular User（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：让 agent 查询我自己的订阅与权益状态
**从而**：在对话中直接确认我当前享受的订阅权益，无需打开用户侧页面

**【验收标准】**

**场景 1：查询我的订阅成功**
```gherkin
Given agent 已由我的 Herald 账号完成授权接入
And 授权包含读取本人订阅所需的许可范围
And 我的账号未被管理员授予查询角色
And 我持有生效中的订阅
When 我让 agent 查询我的订阅
Then 返回我的订阅信息（含订阅状态与生效中的权益）
```

**场景 2：无订阅时的表现**
```gherkin
Given agent 已由我的 Herald 账号完成授权接入
And 授权包含读取本人订阅所需的许可范围
And 我没有任何订阅
When 我让 agent 查询我的订阅
Then 返回明确的无订阅状态
And 不视为错误
```

**场景 3：授权许可范围不足**
```gherkin
Given 我完成 MCP 授权时未包含读取本人订阅所需的许可范围
When agent 调用我的订阅查询工具
Then 查询被拒绝，客户端收到可识别的授权不足提示
And 提示指明重新授权所需的许可范围
And 不返回任何订阅数据
```

**场景 4：无法借本人视角工具查询他人订阅**
```gherkin
Given agent 已由我的 Herald 账号完成授权接入并获得本人订阅的查询许可
When 我让 agent 用本人视角工具查询另一个用户的订阅
Then 本人视角工具不接受指定其他用户，无法完成该查询
And 不返回任何他人订阅数据
```

---

### 故事 11：查看与关闭本租户 MCP 接入 [US-MCP-011]

**优先级**: P1

**【用户故事】**
**作为**：Realm Admin（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：在已有 Client App 列表识别本租户的内置 MCP 客户端并按需禁用
**从而**：统一关闭本租户的 agent 接入，同时避免误删系统内置入口

**【验收标准】**

**场景 1：查看系统内置 MCP 客户端**
```gherkin
Given 我持有客户端应用查看权限
When 我打开本租户的 Client App 列表
Then 列表中有一个内置 MCP 客户端
And 该客户端显示系统内置标识
And 我不能删除该客户端
```

**场景 2：禁用关闭已有连接与新授权**
```gherkin
Given 我持有客户端应用管理权限
And 本租户已有用户经内置 MCP 客户端授权并成功查询
When 我在 Client App 列表禁用该客户端
Then 本租户已有 MCP 授权不能继续查询
And 新的 MCP 浏览器授权被拒绝
And 其他租户的 MCP 接入不受影响
```

**场景 3：不能删除内置客户端**
```gherkin
Given 我持有客户端应用管理权限
When 我尝试删除本租户的内置 MCP 客户端
Then 删除被拒绝
And 该客户端仍存在于本租户列表
```

**场景 4：缺少管理权限不能关闭 MCP 接入**
```gherkin
Given 我只有客户端应用查看权限，没有管理权限
When 我尝试禁用本租户的内置 MCP 客户端
Then 操作被拒绝
And 该客户端保持启用，已有 MCP 授权不因该操作失效
```

---

## 相关文档

- **PRD**: [docs/prd/integration/mcp-server.md](/docs/prd/integration/mcp-server.md)
- **授权服务器基线**: [docs/prd/auth/oauth.md](/docs/prd/auth/oauth.md)
- **决策记录**: `docs/prd/integration/mcp-server.md` §7
- **技术预研**: `.ai/tech-research/mcp-server.md`
