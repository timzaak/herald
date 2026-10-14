# Herald 系统 用户故事

> 角色定义见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)

## 用户故事

### 故事 1：配置异步支付积分发放策略 [US-AP-001]

**优先级**: P0

**【用户故事】**
**作为**：Realm Admin（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：配置异步支付场景下的积分发放策略
**从而**：在用户体验（立即获得积分）和资金安全（确认到账后发放）之间做选择

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：查看当前积分发放策略**
```gherkin
Given 我是 realm-1 的管理员
And realm-1 已配置 Stripe 支付平台
When 我查看 Billing 设置中的"异步支付策略"
Then 系统显示当前策略为"保守"（默认）
And 策略说明为"异步支付确认到账后才发放积分"
```

**场景 2：切换为积极策略**
```gherkin
Given 我是 realm-1 的管理员
And 当前异步支付策略为"保守"
When 我将策略切换为"积极"
And 系统显示警告："积极策略下，用户在银行转账确认前即可获得积分。如果支付最终失败，系统将自动回收积分。"
And 我确认切换
Then 策略更新成功
And 后续异步支付（SEPA/ACH/BECS/Bacs）发起后立即发放积分
```

**场景 3：切换回保守策略**
```gherkin
Given 我是 realm-1 的管理员
And 当前异步支付策略为"积极"
When 我将策略切换为"保守"
Then 策略更新成功
And 已通过积极策略发放的积分不受影响
And 后续新的异步支付回到默认行为（确认到账后发放）
```

**场景 4：未配置支付平台时策略不可见**
```gherkin
Given 我是 realm-2 的管理员
And realm-2 未配置任何支付平台
When 我查看 Billing 设置
Then 系统不显示"异步支付策略"配置项
```

---

### 故事 2：积极策略下异步支付立即发放积分 [US-AP-002]

**优先级**: P0

**【用户故事】**
**作为**：Herald 系统（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：在积极策略下，异步支付发起后立即发放积分
**从而**：使用银行转账等延迟支付方式的用户无需等待 2-14 个工作日即可获得服务

**【验收标准】**

**场景 1：SEPA 支付立即发放积分（积极策略）**
```gherkin
Given realm-1 的异步支付策略为"积极"
And 用户通过 Stripe Checkout 选择 SEPA Direct Debit 支付
When Stripe 发送 checkout.session.completed 事件，payment_status 为 "unpaid"
Then 系统立即执行积分发放（与同步支付相同的履约逻辑）
And 用户积分余额立即增加
And 支付尝试状态更新为"已成功"
```

**场景 2：ACH 支付立即发放积分（积极策略）**
```gherkin
Given realm-1 的异步支付策略为"积极"
And 用户通过 Stripe Checkout 选择 ACH 支付
When Stripe 发送 checkout.session.completed 事件，payment_status 为 "unpaid"
Then 系统立即执行积分发放
And 用户无需等待 2-4 个工作日即可使用服务
```

**场景 3：保守策略下不提前发放**
```gherkin
Given realm-1 的异步支付策略为"保守"
And 用户通过 Stripe Checkout 选择 SEPA Direct Debit 支付
When Stripe 发送 checkout.session.completed 事件，payment_status 为 "unpaid"
Then 系统不发放积分
And 支付尝试状态保持"待处理"
And 系统等待异步确认结果
```

**场景 4：异步确认到达时不重复发放**
```gherkin
Given realm-1 的异步支付策略为"积极"
And 系统已在 unpaid 时发放积分
When Stripe 发送 checkout.session.async_payment_succeeded 事件
Then 系统识别积分已发放，不重复执行
And 用户积分余额不变
```

---

### 故事 3：异步支付失败后回收积分 [US-AP-003]

**优先级**: P0

**【用户故事】**
**作为**：Herald 系统（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：在积极策略下异步支付最终失败时，自动回收已发放的积分
**从而**：防止用户在支付失败后仍持有未付费的积分

**【验收标准】**

**场景 1：SEPA 支付失败后全额回收积分**
```gherkin
Given realm-1 的异步支付策略为"积极"
And 系统已在 SEPA 支付发起时发放了 500 积分
And 用户当前积分余额为 500（未使用）
When Stripe 发送 checkout.session.async_payment_failed 事件
Then 系统回收 500 积分
And 用户积分余额变为 0
And 系统记录积分回收交易
And 支付尝试状态更新为"已失败"
```

**场景 2：一次性购买全额回收**
```gherkin
Given realm-1 的异步支付策略为"积极"
And 用户通过一次性购买获得了 1000 积分
And 积分类型为 topup_credit
When 异步支付失败
Then 系统全额回收已发放的积分数量（扣除已使用部分）
And 回收后的积分余额不低于 0
And 系统记录负数积分交易
```

**场景 3：订阅购买的积分回收**
```gherkin
Given realm-1 的异步支付策略为"积极"
And 用户通过订阅获得了 300 subscription_credit
And 订阅处于 Active 状态
When 异步支付失败
Then 系统回收未使用的订阅积分
And 订阅状态更新为 Canceled 或 Expired
And 系统记录积分回收交易和订阅变更历史
```

**场景 4：保守策略下支付失败不回收**
```gherkin
Given realm-1 的异步支付策略为"保守"
And 支付尝试状态为"待处理"（未发放积分）
When Stripe 发送 checkout.session.async_payment_failed 事件
Then 系统将支付尝试状态更新为"已失败"
And 不执行积分回收（因为未发放过积分）
```

---

### 故事 4：回收时余额不足的处理 [US-AP-004]

**优先级**: P1

**【用户故事】**
**作为**：Herald 系统（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：在积分回收时如果用户余额不足，记录负债并限制后续使用
**从而**：确保系统不会因用户消费已回收积分而产生不可追回的损失

**【验收标准】**

**场景 1：回收时余额充足**
```gherkin
Given 系统需要回收 500 积分
And 用户当前余额为 600
When 系统执行积分回收
Then 用户余额变为 100
And 积分回收交易记录为 -500
```

**场景 2：回收时余额部分不足**
```gherkin
Given 系统需要回收 500 积分
And 用户当前余额为 200（已消费 300）
When 系统执行积分回收
Then 用户余额变为 0
And 系统记录 200 的实际回收
And 系统记录 300 的未回收负债
And 管理员可在管理后台看到负债记录
```

**场景 3：回收时余额为零**
```gherkin
Given 系统需要回收 500 积分
And 用户当前余额为 0（已消费全部 500）
When 系统执行积分回收
Then 用户余额保持为 0
And 系统记录 500 的未回收负债
```

---

## 用户故事优先级汇总

| 优先级 | 用户故事数量 | 关键故事 |
|--------|------------|---------|
| P0 | 3 | 配置策略、立即发放、失败回收 |
| P1 | 1 | 余额不足回收处理 |
| P2 | 0 | - |

---

## 相关文档

- **PRD**: `docs/prd/billing/points.md` — 积分系统 PRD
- **PRD**: `docs/prd/billing/stripe-payment.md` — Stripe 支付集成 PRD
- **PRD**: `docs/prd/billing/subscription.md` — 订阅计费 PRD
- **用户故事**: `billing/payment-attempt.md` — 支付尝试用户故事
- **用户故事**: `billing/webhook-compensation.md` — Webhook 补偿用户故事
