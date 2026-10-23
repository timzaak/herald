# Webhook Compensation 用户故事

> 角色定义见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)

## 用户故事

### 故事 1：定时检测并补偿缺失的 Webhook 事件 [US-WC-001]

**优先级**: P0

**【用户故事】**
**作为**：Herald 系统（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：定时从支付方拉取近期事件，与本地记录对比，补处理缺失的事件
**从而**：在 webhook 未到达时仍能保持订阅和支付状态一致

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：Stripe 缺失事件补偿**
```gherkin
Given Stripe 上一个时间段内产生了 3 个订阅事件
And Herald 的 payment_event 中只有 1 条记录
When 补偿 Job 运行
Then 系统从 Stripe 拉取该时间段的所有事件
And 系统识别出 2 条缺失事件
And 系统对缺失事件执行与 webhook 相同的业务逻辑
And 补处理完成后订阅状态与 Stripe 一致
And 系统记录对账统计日志（拉取数、缺失数、补处理成功数）
```

**场景 2：Creem 缺失事件补偿**
```gherkin
Given Creem 上一个时间段内有一笔支付和一次订阅状态变化
And Herald 未收到对应的 webhook 回调
When 补偿 Job 运行
Then 系统从 Creem 查询近期交易和订阅状态
And 系统通过对比发现缺失的事件
And 系统对缺失事件执行补偿处理
And 补处理完成后订阅和积分状态正确
```

**场景 3：无缺失事件**
```gherkin
Given Stripe 上一个时间段内产生了 5 个事件
And Herald 的 payment_event 中已有全部 5 条记录
When 补偿 Job 运行
Then 系统识别出 0 条缺失事件
And 系统不做任何补偿处理
And 系统记录对账统计日志（拉取数 5、缺失数 0）
```

**场景 4：多 Realm 分别对账**
```gherkin
Given realm-1 配置了 Stripe 支付平台
And realm-2 配置了 Creem 支付平台
And realm-3 未配置任何支付平台
When 补偿 Job 运行
Then 系统仅对配置了支付平台的 realm 执行对账
And realm-1 使用 Stripe API 拉取事件
And realm-2 使用 Creem API 查询交易和订阅
And realm-3 被跳过
```

**场景 5：数据不一致时仅记录日志**
```gherkin
Given 补偿 Job 拉取到 provider 侧的事件
And 本地 payment_event 中存在该事件的记录但状态与 provider 不一致
When 系统检测到不一致
Then 系统写入 Error 级别日志，包含 realm_id、event_id、本地状态和 provider 状态
And 系统不执行自动修复
And 系统不发送报警通知
And 补偿 Job 继续处理后续事件
```

---

### 故事 2：补偿处理保持幂等性 [US-WC-002]

**优先级**: P0

**【用户故事】**
**作为**：Herald 系统（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：补偿处理与 webhook 处理共享相同的幂等保证
**从而**：重复的补偿运行不会导致积分重复发放或订阅状态错误

**【验收标准】**

**场景 1：补偿事件已被 webhook 处理**
```gherkin
Given Stripe 事件 evt_123 已通过 webhook 正常处理
And 订阅状态已更新，积分已发放
When 补偿 Job 拉取到 evt_123 并尝试补处理
Then 系统识别该事件已处理并跳过
And 积分不重复发放
And 订阅状态不变
```

**场景 2：补偿事件已被前次补偿处理**
```gherkin
Given 补偿 Job 上次运行已补处理了事件 evt_456
When 本次补偿 Job 再次拉取到 evt_456
Then 系统识别该事件已处理并跳过
And 不产生重复的业务副作用
```

**场景 3：补偿处理失败不阻塞后续事件**
```gherkin
Given 补偿 Job 拉取到 3 个缺失事件
And 第 2 个事件补偿处理时发生业务错误
When 系统处理该批次
Then 系统记录第 2 个事件的失败日志
And 系统继续处理第 3 个事件
And 系统记录本批次统计（成功 2、失败 1）
```

---

## 用户故事优先级汇总

| 优先级 | 用户故事数量 | 关键故事 |
|--------|------------|---------|
| P0 | 2 | 定时检测并补偿缺失事件、补偿处理保持幂等性 |
| P1 | 0 | - |
| P2 | 0 | - |

---

## 相关文档

- **PRD**: `docs/prd/billing/subscription.md` — 订阅计费 PRD（含 Webhook 处理与补偿规则）
- **用户故事**: `billing/entitlement-mapping.md` — Entitlement 映射（含 Webhook metadata 映射）
- **用户故事**: `billing/payment-attempt.md` — 支付尝试（含主动查询补偿）
