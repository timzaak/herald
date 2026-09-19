# Realm Admin 积分管理用户故事

> 角色定义见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)

**边界说明**：积分策略按 `entitlement_key` 配置，存储在 Entitlement Mapping 中。管理员在 Entitlement Mappings 页面配置每个映射的积分发放规则（每期积分、订阅时是否发放、有效期等）。Product/Plan 本地 CRUD 已移除。

## 用户故事

### 故事 1：配置 Entitlement 积分策略 [US-PO-001]

**优先级**: P0

**【用户故事】**
**作为**：Realm Admin（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：配置每个 Entitlement Mapping 的积分发放规则（每期积分、订阅时是否发放）
**从而**：用户订阅后自动获得相应积分

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：配置 Entitlement 积分策略**
```gherkin
Given 我是 realm-1 的管理员
And 已从 Stripe 同步了产品，存在 Entitlement Mapping "pro-plan"
When 我在 Entitlement Mappings 页面编辑 "pro-plan"
And 我填写积分策略信息：
  | Points per Period | 1000        |
  | Grant on Subscribe | true       |
  | Grant Period Type | monthly     |
  | Validity Days     | 30          |
  | Max Periods       | 10          |
And 我保存更改
Then 积分策略配置成功
And Entitlement Mapping "pro-plan" 显示积分策略为已配置
```

**场景 2：查看 Entitlement 积分配置**
```gherkin
Given 我是 realm-1 的管理员
And Entitlement Mapping "pro-plan" 已配置积分策略
When 我查看 "pro-plan" 的详情
Then 我看到以下积分策略信息：
  | 字段                    | 值       |
  | 每期发放积分             | 1000     |
  | 订阅时是否发放           | 是       |
  | 发放周期类型             | 月度     |
  | 积分有效期（天）         | 30       |
  | 最大发放期数             | 10       |
```

**场景 3：编辑积分策略**
```gherkin
Given 我是 realm-1 的管理员
And Entitlement Mapping "pro-plan" 已配置积分策略
When 我编辑该映射的积分策略：
  | Points per Period  | 1200 |
  | Grant on Subscribe | true |
And 我保存更改
Then 积分策略更新成功
And 新订阅将使用更新后的策略
```

**场景 4：关闭订阅时积分发放**
```gherkin
Given 我是 realm-1 的管理员
And Entitlement Mapping "basic-plan" 已配置积分策略
And 该映射已启用订阅时积分发放（Grant on Subscribe = true）
When 我将 "Grant on Subscribe" 设置为 false
And 我保存更改
Then 订阅时不再额外发放积分
And 按周期的定期发放仍正常执行
```

**场景 5：Entitlement 无积分策略**
```gherkin
Given 我是 realm-1 的管理员
And Entitlement Mapping "free-plan" 未配置积分策略
When 用户通过 "free-plan" 订阅
Then 系统跳过积分发放
And 记录诊断："No points policy found for entitlement 'free-plan'"
```

**场景 6：禁用映射后积分策略不执行**
```gherkin
Given 我是 realm-1 的管理员
And Entitlement Mapping "pro-plan" 已配置积分策略
And 该映射状态为 "disabled"
When Webhook 收到 "pro-plan" 的订阅事件
Then 订阅投影仍正常更新
And 但积分策略不执行（不发放积分）
When 管理员重新启用该映射
Then 后续订阅事件恢复积分策略执行
```


### 故事 2：查看所有用户积分账户 [US-PO-002]

**优先级**: P1

**【用户故事】**
**作为**：Realm Admin（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：查看租户内所有用户的积分账户列表（包括分类余额）
**从而**：了解整体积分使用情况

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：查看用户积分账户列表（包含分类余额）**
```gherkin
Given 我是 realm-1 的管理员
And realm-1 有 100 个用户
When 我访问积分管理页面
Then 我看到用户积分账户列表
And 列表显示每个用户的积分信息：
  | 用户 ID        | 用户名       | 总余额 | 单位   | 充值积分 | 会员积分 | 累计消耗 | 状态 |
  | user-1         | alice@example.com | 5000 | points | 3000 | 2000 | 5000 | active |
  | user-2         | bob@example.com   | 8000 | points | 6000 | 2000 | 4000 | active |
And 我可以按积分类型排序
And 我可以筛选特定积分类型的用户
```

**场景 2：按用户名或邮箱搜索**
```gherkin
Given 我是 realm-1 的管理员
And realm-1 有多个用户
When 我在搜索框输入 "alice"
Then 我只看到用户名或邮箱包含 "alice" 的用户
And 搜索结果正确
```

**场景 3：查看账户状态**
```gherkin
Given 我是 realm-1 的管理员
And 存在账户状态不同的用户
When 我查看积分账户列表
Then 我可以看到每个账户的状态：
  | user-1 | active  |
  | user-2 | frozen  |
  | user-3 | closed  |
```

**场景 4：分页查看账户列表**
```gherkin
Given 我是 realm-1 的管理员
And realm-1 有 1000 个用户
When 我查看积分账户列表（每页 20 条）
Then 我看到第 1 页的 20 个用户
And 可以翻页查看其他用户
And 页面显示总页数和当前页码
```

**场景 5：查看用户积分账本明细**
```gherkin
Given 我是 realm-1 的管理员
And 已存在用户 "user-1"
And user-1 有多笔积分来源
When 我在积分账户列表中点击 user-1
And 我点击"积分账本"标签
Then 我看到 user-1 的积分账本明细
And 列表显示每笔积分的完整生命周期：
  | 积分类型 | 来源类型 | 发放金额 | 已使用 | 剩余 | 回收金额 | 状态 | 过期时间 |
  | 会员积分 | 订阅续费 | 1000 | 300 | 700 | 0 | active | 2026-04-15 |
  | 充值积分 | 充值购买 | 2000 | 500 | 1500 | 0 | active | 永久有效 |
And 我可以按积分类型筛选
And 我可以按状态筛选
And 我可以按过期时间排序
And 我可以点击查看某笔积分的完整消费分摊记录
```


### 故事 3：查看用户积分交易历史 [US-PO-003]

**优先级**: P1

**【用户故事】**
**作为**：Realm Admin（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：查看任意用户的积分交易历史
**从而**：审计和问题排查

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：查看用户的所有交易记录（包含新的交易类型）**
```gherkin
Given 我是 realm-1 的管理员
And 已存在用户 "user-1"
And user-1 有 50 条积分交易记录
When 我在积分账户列表中点击 user-1
Then 我看到 user-1 的交易历史列表
And 列表显示每笔交易的详细信息：
  | 交易 ID | 交易类型 | 积分类型 | 金额 | 交易后余额 | 描述 | 时间 |
  | txn-001 | 订阅续费 | 会员积分 | 1000 | 1000 | entitlement pro-plan 续费 | 2026-03-13 12:00 |
  | txn-002 | 消耗 | 会员积分 | -100 | 900 | 调用 AI API | 2026-03-13 12:30 |
  | txn-003 | 退款回收 | 充值积分 | -350 | 550 | 退款回收：50% 退款 | 2026-03-13 13:00 |
  | txn-004 | 过期回收 | 会员积分 | -200 | 350 | 会员积分过期 | 2026-03-15 00:00 |
And 我可以看到退款、过期等新的交易类型
```

**场景 2：按交易类型筛选（包含新的交易类型）**
```gherkin
Given 我是 realm-1 的管理员
And user-1 有多种类型的交易记录
When 我选择只查看"退款回收"类型的交易
Then 我只看到退款回收类型的交易记录
And 其他类型的交易不显示
When 我选择只查看"过期回收"类型的交易
Then 我只看到过期回收类型的交易记录
And 我可以选择的交易类型包括：
  | 充值 | 订阅赠送 | 订阅续费 | 订阅升级 |
  | 消耗 | 退款回收 | 过期回收 | 取消回收 | 调整 |
```

**场景 3：按时间范围筛选**
```gherkin
Given 我是 realm-1 的管理员
And user-1 有跨越多月的交易记录
When 我选择时间范围 "2026-03-01 到 2026-03-31"
Then 我只看到该时间范围内的交易记录
And 3 月之外的交易不显示
```

**场景 4：按 Client App 筛选**
```gherkin
Given 我是 realm-1 的管理员
And user-1 的积分消耗来自不同的 Client App
When 我选择按 Client App "app-001" 筛选
Then 我只看到来自 app-001 的消耗交易
And 来自其他 Client App 的交易不显示
```

**场景 5：分页查看交易历史**
```gherkin
Given 我是 realm-1 的管理员
And user-1 有 500 条交易记录
When 我查看交易历史（每页 20 条）
Then 我看到第 1 页的 20 条交易
And 可以翻页查看其他交易
And 页面显示总交易数和当前页码
```


### 故事 4：管理 Entitlement 积分策略 [US-PO-004]

**优先级**: P2

**【用户故事】**
**作为**：Realm Admin（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：批量管理多个 Entitlement Mapping 的积分策略
**从而**：灵活调整积分赠送策略

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：为多个 Entitlement 配置积分策略**
```gherkin
Given 我是 realm-1 的管理员
And 已从支付方同步了多个 Entitlement Mapping：
  | Entitlement Key |
  | basic-plan      |
  | pro-plan        |
  | enterprise-plan |
When 我为每个 Entitlement 配置积分策略：
  | Entitlement Key | 每期积分 | 订阅时发放 | 周期类型  |
  | basic-plan      | 500      | 是         | monthly   |
  | pro-plan        | 1000     | 是         | monthly   |
  | enterprise-plan | 2000     | 是         | monthly   |
Then 所有 Entitlement 策略配置成功
And Entitlement Mappings 列表显示所有配置
```

**场景 2：编辑已有策略**
```gherkin
Given 我是 realm-1 的管理员
And Entitlement Mapping "pro-plan" 已配置积分策略
When 我编辑该映射的策略：
  | 字段               | 原值  | 新值  |
  | Points per Period  | 1000  | 1500  |
  | Grant on Subscribe | true  | false |
And 我保存更改
Then 策略更新成功
And 新的订阅和续费将使用新策略
And 历史交易记录不受影响
```

**场景 3：禁用有活跃订阅的 Entitlement 的积分策略**
```gherkin
Given 我是 realm-1 的管理员
And Entitlement Mapping "pro-plan" 有 10 个活跃订阅
When 我禁用该映射
Then 映射状态变为 disabled
And Webhook 仍更新订阅投影
And 但积分策略不执行
And 系统提示："10 active subscriptions affected"
```


### 故事 5：查看 Entitlement 充值引导 [US-PO-005]

**优先级**: P2

**【用户故事】**
**作为**：Realm Admin（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：查看 Entitlement 的积分策略信息
**从而**：向用户说明积分赠送规则

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：查看单个 Entitlement 的积分规则**
```gherkin
Given 我是 realm-1 的管理员
And Entitlement Mapping "pro-plan" 已配置积分策略
When 我查看该映射的积分引导信息
Then 我看到清晰的充值说明：
  | Entitlement Key    | pro-plan                       |
  | 每期发放积分       | 1000                           |
  | 订阅时是否额外发放 | 是                             |
  | 发放周期           | 月度                           |
  | 积分有效期         | 30 天                          |
  | 最大发放期数       | 10                             |
```

**场景 2：查看所有 Entitlement 的积分规则对比**
```gherkin
Given 我是 realm-1 的管理员
And 已存在多个已配置积分策略的 Entitlement Mapping
When 我查看积分策略总览页面
Then 我看到所有 Entitlement 的积分规则对比：
  | Entitlement Key | 每期积分 | 订阅时发放 | 周期 | 最大期数 |
  | basic-plan      | 500      | 是         | 月度 | 5       |
  | pro-plan        | 1000     | 是         | 月度 | 10      |
  | enterprise-plan | 2000     | 是         | 月度 | 无限制  |
```


---

### 故事 6：配置 Realm 默认积分策略 [US-PO-006]

**优先级**: P0

**【用户故事】**
**作为**：Realm Admin（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：配置免费用户的默认积分策略
**从而**：灵活控制免费用户的积分权益，优化产品转化率

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：查看 Realm 默认配置**
```gherkin
Given 我是 realm-1 的管理员
When 我访问"Realm 配置"页面
Then 我看到默认积分配置：
  | 配置项 | 当前值 |
  | 注册初始积分 | 1000 |
  | 每日积分数 | 50 |
  | 每日积分周期类型 | daily |
  | 每日积分有效期 | 1 天 |
And 我看到配置影响说明："配置仅影响新注册用户，不影响现有用户"
```

**场景 2：修改注册初始积分数**
```gherkin
Given 我是 realm-1 的管理员
And 当前注册初始积分为 1000
When 我将"注册初始积分"修改为 1500
And 我点击"保存"按钮
Then 配置更新成功
And 系统显示消息："配置已更新，新注册用户将获得 1500 初始积分"
And 现有用户的积分不受影响
```

**场景 3：修改每日积分数**
```gherkin
Given 我是 realm-1 的管理员
And 当前每日积分为 50
When 我将"每日积分数"修改为 100
And 我点击"保存"按钮
Then 配置更新成功
And 新注册用户每日获得 100 积分
And 现有免费用户的每日积分仍为 50（不受影响）
```

**场景 4：修改每日积分有效期**
```gherkin
Given 我是 realm-1 的管理员
And 当前每日积分有效期为 1 天
When 我将"每日积分有效期"修改为 2 天
And 我点击"保存"按钮
Then 配置更新成功
And 新注册用户的每日积分有效期为 2 天
And 现有免费用户的每日积分有效期仍为 1 天（不受影响）
```

**场景 5：禁用每日积分**
```gherkin
Given 我是 realm-1 的管理员
And 当前每日积分数为 50
When 我将"每日积分数"设置为 0
And 我点击"保存"按钮
Then 配置更新成功
And 新注册用户不会获得每日积分（每日积分数为 0 即禁用）
And 现有免费用户的每日积分调度不受影响（继续发放）
```

**场景 6：配置验证（不允许负数）**
```gherkin
Given 我是 realm-1 的管理员
When 我尝试将"注册初始积分"设置为 -100
And 我点击"保存"按钮
Then 系统显示错误："积分数量必须大于等于 0"
And 配置未更新
```

**场景 7：配置验证（有效期必须大于 0）**
```gherkin
Given 我是 realm-1 的管理员
When 我尝试将"每日积分有效期"设置为 0
And 我点击"保存"按钮
Then 系统显示错误："有效期必须大于 0 天"
And 配置未更新
```

**场景 8：查看配置历史**（待实现）
```gherkin
Given 我是 realm-1 的管理员
And 我已多次修改配置
When 我访问"配置历史"页面
Then 我看到配置变更历史：
  | 修改时间 | 配置项 | 旧值 | 新值 |
  | 2026-03-23 12:00 | 注册初始积分 | 1000 | 1500 |
  | 2026-03-22 10:00 | 每日积分数 | 50 | 100 |
And 我可以看到谁在何时修改了配置

> 注意：此场景尚未实现，后端暂不支持配置变更历史记录。
```

---

### 故事 7：查看免费用户积分统计 [US-PO-007]

**优先级**: P1

> **延后项**：本故事主体（免费用户发放 / 转化漏斗指标）不随首版交付（见 [billing-statistics PRD](/docs/prd/billing/billing-statistics.md) §2.2 已知差异）。

**【用户故事】**
**作为**：Realm Admin（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：查看免费用户的积分统计数据
**从而**：了解免费用户的使用情况和转化率

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：查看免费用户概览统计**
```gherkin
Given 我是 realm-1 的管理员
And realm-1 有 1000 个免费用户
When 我访问"免费用户统计"页面
Then 我看到以下统计数据：
  | 统计项 | 数值 |
  | 总免费用户数 | 1000 |
  | 活跃用户数（近 7 天） | 800 |
  | 累计发放注册初始积分 | 1,000,000 |
  | 累计发放每日积分 | 40,000 |
  | 平均每用户每日积分 | 50 |
  | 转化率（免费→付费） | 15% |
And 数据实时更新
```

**场景 2：按时间范围筛选统计数据**
```gherkin
Given 我是 realm-1 的管理员
When 我选择时间范围"最近 30 天"
Then 我看到近 30 天的统计数据：
  | 统计项 | 数值 |
  | 新增免费用户 | 200 |
  | 发放注册初始积分 | 200,000 |
  | 发放每日积分 | 10,000 |
  | 转化为付费用户 | 30 |
And 转化率显示为 15%（30/200）
```

**场景 3：查看免费用户增长趋势**
```gherkin
Given 我是 realm-1 的管理员
When 我查看"用户增长趋势"图表
Then 我看到折线图显示每日新增免费用户数量
And 图表横轴为日期（最近 30 天）
And 图表纵轴为用户数量
And 我可以鼠标悬停查看具体日期的用户数
```

**场景 4：查看积分发放趋势**
```gherkin
Given 我是 realm-1 的管理员
When 我查看"积分发放趋势"图表
Then 我看到两条曲线：
  - 注册初始积分发放趋势
  - 每日积分发放趋势
And 图表横轴为日期（最近 30 天）
And 图表纵轴为积分数
And 我可以鼠标悬停查看具体日期的发放数量
```

**场景 5：查看转化率趋势**
```gherkin
Given 我是 realm-1 的管理员
When 我查看"转化率趋势"图表
Then 我看到折线图显示每日转化率（免费用户升级为付费用户的比例）
And 图表显示近 30 天的转化率变化
And 我可以看到转化率是否有提升或下降趋势
And 图表显示平均转化率（如 15%）
```

**场景 6：导出统计数据**
```gherkin
Given 我是 realm-1 的管理员
When 我点击"导出数据"按钮
Then 系统生成 CSV 文件
And 文件包含以下字段：
  - 日期
  - 新增免费用户数
  - 活跃免费用户数
  - 发放注册初始积分
  - 发放每日积分
  - 转化用户数
  - 转化率
And 我可以下载或分享该文件
```

**场景 7：查看单个免费用户的积分详情**
```gherkin
Given 我是 realm-1 的管理员
And 已存在免费用户 "user-1"
When 我在免费用户列表中点击 user-1
Then 我看到该用户的积分详情：
  | 积分类型 | 余额 | 有效期 | 发放时间 |
  | 注册初始积分 | 1000 | 永久有效 | 2026-03-23 15:30 |
  | 每日免费积分 | 50 | 明天 15:30 过期 | 2026-03-24 15:30 |
And 我看到该用户的积分使用统计：
  - 累计获得积分：1050
  - 累计消耗积分：200
  - 当前余额：850
And 我看到该用户是否已升级为付费用户
```

---


### 故事 8：主动发放积分 [US-PO-008]

**优先级**: P0

**【用户故事】**
**作为**：Realm Admin（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：主动向指定用户发放积分，并可设置有效期和发放原因
**从而**：灵活运营，奖励用户或补偿异常情况

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：向单个用户发放积分**
```gherkin
Given 我是 realm-1 的管理员
And 已存在用户 "user-1"
When 我在积分管理页面点击 "Grant Points" 按钮
And 我填写发放信息：
  | 用户     | user-1           |
  | 积分数量 | 100              |
  | 有效期   | 30 天            |
  | 发放原因 | "Event reward"   |
And 我提交表单
Then 系统向 user-1 发放 100 积分
And 系统显示成功消息："Successfully granted 100 points to user-1"
And user-1 的积分余额增加 100
And 交易历史中新增一笔类型为 "发放" 的记录，原因为 "Event reward"
And 这批积分将在 30 天后过期
```

**场景 2：发放积分并设置永久有效**
```gherkin
Given 我是 realm-1 的管理员
And 已存在用户 "user-1"
When 我向 user-1 发放积分
And 我设置有效期为 "永久有效"
And 我提交表单
Then 系统向 user-1 发放积分
And 这批积分永久有效（无过期时间）
```

**场景 3：积分数量必须为正数**
```gherkin
Given 我是 realm-1 的管理员
When 我尝试发放积分
And 我设置积分数量为 0 或负数
Then 系统显示验证错误："积分数量必须大于 0"
And 发放操作未执行
```

**场景 4：用户不存在**
```gherkin
Given 我是 realm-1 的管理员
And 指定的用户 "nonexistent-user" 不存在
When 我尝试向该用户发放积分
Then 系统显示错误："User not found"
And 发放操作未执行
```

**场景 5：跨 Realm 用户不可发放**
```gherkin
Given 我是 realm-1 的管理员
And 用户 "user-other-realm" 属于 realm-2
When 我尝试向 user-other-realm 发放积分
Then 系统显示权限不足错误
And 发放操作未执行
```

**场景 6：发放原因必填**
```gherkin
Given 我是 realm-1 的管理员
When 我尝试发放积分但不填写发放原因
Then 系统显示验证错误："发放原因为必填项"
And 发放操作未执行
```

---

### 故事 9：配置多时间窗滚动配额 [US-PO-009]

> 本故事承载"积分发放策略重设计（points-grant-redesign）"的新模型意图：订阅/免费周期积分从"每次发放量 + 周期"改为"多时间窗滚动配额"，作为故事 1（US-PO-001）与故事 6（US-PO-006）的新增配置形态。

**优先级**: P0

**【用户故事】**
**作为**：Realm Admin（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：为订阅（Entitlement Mapping）和免费周期（Realm 默认配置）配置多时间窗滚动配额（如 5 小时窗、周窗、月窗及各自上限）
**从而**：灵活控制不同时间粒度的额度上限，对齐主流 AI 产品的滚动限额体验

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：为订阅 Entitlement Mapping 配置多时间窗配额**
```gherkin
Given 我是 realm-1 的管理员
And 存在 Entitlement Mapping "pro-plan"
When 我编辑 "pro-plan" 的积分策略
And 我配置多时间窗滚动配额：
  | 时间窗 | 上限 |
  | 5 小时 | 500 |
  | 周     | 5000 |
  | 月     | 20000 |
And 我保存更改
Then 配置保存成功
And "pro-plan" 显示已配置的多时间窗配额
And 新订阅与续费的用户将按该多窗口配额享受滚动限额
```

**场景 2：为 Realm 默认免费周期配置多时间窗配额**
```gherkin
Given 我是 realm-1 的管理员
When 我访问"Realm 配置"页面编辑免费用户默认积分策略
And 我为免费周期配置多时间窗滚动配额（如每日窗 50、周窗 200）
And 我保存更改
Then 配置保存成功
And 新注册的免费用户将按该多窗口配额享受滚动限额
And 配置说明提示"仅影响新注册用户，不影响现有用户"
```

**场景 3：窗口长度与上限校验**
```gherkin
Given 我是 realm-1 的管理员
When 我尝试配置一个时间窗上限为负数
Then 系统显示错误："配额上限必须大于等于 0"
And 配置未保存
When 我尝试配置一个窗口长度为 0 或非正
Then 系统显示错误："时间窗长度必须为正"
And 配置未保存
```

**场景 4：配置仅影响新授予的配额权益**
```gherkin
Given 我是 realm-1 的管理员
And "pro-plan" 已有活跃订阅用户按旧配额享受额度
When 我修改 "pro-plan" 的多时间窗配额并保存
Then 新订阅与后续续费的用户使用新配额
And 已有订阅用户当前周期的既有权益不受本次修改影响
```

---

## 用户故事优先级汇总

| 优先级 | 用户故事数量 | 关键故事 |
|--------|------------|---------|
| P0 | 4 | US-PO-01: 配置 Entitlement 积分策略, US-PO-06: 配置 Realm 默认积分策略, US-PO-08: 主动发放积分, US-PO-09: 配置多时间窗滚动配额 |
| P1 | 3 | US-PO-02: 查看所有用户积分账户, US-PO-03: 查看用户积分交易历史, US-PO-07: 查看免费用户积分统计 |
| P2 | 2 | US-PO-04: 管理 Entitlement 积分策略, US-PO-05: 查看 Entitlement 充值引导 |

---

## 相关文档

- **PRD**: [docs/prd/billing/points.md](/docs/prd/billing/points.md) - 积分系统产品需求文档（含积分发放）
- **用户故事**: [docs/user-stories/billing/entitlement-mapping.md](/docs/user-stories/billing/entitlement-mapping.md) - Entitlement Mapping 用户故事
- **用户故事**: [docs/user-stories/billing/points-user.md](/docs/user-stories/billing/points-user.md) - 用户积分查询用户故事
- **用户故事**: [docs/user-stories/billing/points-free-user.md](/docs/user-stories/billing/points-free-user.md) - 免费用户积分用户故事
- **依赖 PRD**: [docs/prd/billing/subscription.md](/docs/prd/billing/subscription.md) - Billing 订阅计费产品需求文档
- **依赖 PRD**: [docs/prd/billing/points.md](/docs/prd/billing/points.md) - 积分系统产品需求文档
