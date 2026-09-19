# 支付驱动权益门控（Paywall）用户故事

> 角色定义见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)

## 用户故事

### 故事 1：配置 entitlement 映射的 role 授予维度 [US-PW-001]

**优先级**: P0

**【用户故事】**
**作为**：Realm Admin（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：为任意 entitlement mapping 配置「支付成功后授予哪些角色/权限」，且该配置与购买形态（one_time/recurring）、积分策略彼此独立叠加
**从而**：让同一个商品能同时表示「积分包」「会员订阅」「一次性永久解锁」等不同商业形态，而不必为每种形态新建商品类型

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：为订阅型商品配置 role 授予**
```gherkin
Given 我是 realm-1 的管理员
And 已有一个 billing_type=recurring 的 entitlement mapping（pro-plan）
When 我在该映射上配置「支付成功后授予 role: pro-member」
And 不修改其积分策略
Then 该映射同时具备「积分策略」与「role 授予」两个独立配置维度
And 两维度可各自为空（不授积分 / 不授 role / 都授 / 都不授）
```

**场景 2：为一次性永久解锁商品配置 role 授予（纯权益墙）**
```gherkin
Given 我是 realm-1 的管理员
And 已有一个 billing_type=one_time 的 entitlement mapping（lifetime-unlock）
When 我在该映射上配置「授予 role: lifetime-member」且「积分策略为空」
Then 该商品被标记为「一次性永久解锁」
And 系统允许这种「不发积分、只授 role」的配置存在且可保存
```

**场景 3：role 授予与积分策略正交，互不影响**
```gherkin
Given 同一个 entitlement mapping 已配置积分发放
When 我清空积分策略但保留 role 授予
Then 保存成功，商品变为「纯权益型」
And 反之，清空 role 授予保留积分策略，商品变为「纯积分包」
```

---

### 故事 2：一次性纯权益购买成功且不报错 [US-PW-002]

**优先级**: P0

**【用户故事】**
**作为**：付费终端用户（Regular User）
**我希望**：购买一个「不配积分、只授 role」的一次性商品时支付能成功完成，而不是系统报错
**从而**：使最常见的「付费一次性解锁」付费形态可用（修复当前 one-time 不配积分即报 500 的不一致）

**【验收标准】**

**场景 1：one-time 纯权益购买成功**
```gherkin
Given 一个 billing_type=one_time 的 entitlement mapping 已配置 role 授予但未配置积分分发规则（多钱包规则模型，无单点积分字段）
When 用户完成该商品的支付
Then 支付尝试记录为成功
And 系统不报错、不发放积分
And 用户被授予映射配置的 role
```

**场景 2：行为与 recurring 容错一致**
```gherkin
Given one-time 与 recurring 两种映射都未配置积分
When 分别完成支付
Then 两者履约行为一致：都不因缺积分策略报错，都记录支付成功
```

---

### 故事 3：支付成功自动授予 role [US-PW-003]

**优先级**: P0

**【用户故事】**
**作为**：付费终端用户（Regular User）
**我希望**：支付成功后系统自动授予我对应的 role，无需管理员手工操作
**从而**：实现「付钱=立即解锁」，第三方应用可凭 role 一行判断就放行

**【验收标准】**

**场景 1：一次性购买授予永久 role**
```gherkin
Given 一个 one_time+role 映射（lifetime-unlock）
When 用户首次支付成功
Then 用户立即获得映射配置的 role
And 该 role 不设过期时间（永久解锁，买断制）
```

**场景 2：订阅首期授予 role**
```gherkin
Given 一个 recurring+role 映射（pro-plan）
When 用户首次订阅支付成功
Then 用户立即获得对应 role
And 该 role 在订阅有效期内持续有效
```

**场景 3：支付授予与手工授予可追溯区分**
```gherkin
Given 用户同时被管理员手工授予与支付授予了同一 role
When 系统撤销该 role 的支付授予部分时
Then 仅移除「支付授予」来源的关联
And 管理员手工授予部分不受影响
And 权限来源（支付/手工）可被追溯查询
```

---

### 故事 4：一次性永久权益一人一次防重复购买 [US-PW-004]

**优先级**: P0

**【用户故事】**
**作为**：付费终端用户（Regular User）
**我希望**：对「一次性永久解锁」类商品，系统在购买前阻止我重复购买
**从而**：避免永久权益被重复付费（永久解锁重复购买无意义）

**【验收标准】**

**场景 1：已拥有该永久权益时阻止再次购买**
```gherkin
Given 用户已成功购买 lifetime-unlock（已拥有对应永久 role）
When 用户再次发起同一商品的购买
Then 系统拒绝创建支付尝试并提示已拥有该权益
```

**场景 2：积分包仍可重复购买**
```gherkin
Given 用户已购买过 credits-pack（one_time，无 role 授予）
When 用户再次发起同一商品的购买
Then 允许创建支付尝试（积分包可重复购买）
```

**场景 3：并发双购被阻止**
```gherkin
Given 同一用户对同一 one_time+role 商品同时发起两次支付尝试
When 两个请求竞争到达
Then 至多只有一个请求成功（防并发双购）
```

---

### 故事 5：支付事件触发 role 撤销 [US-PW-005]

**优先级**: P0

**【用户故事】**
**作为**：Realm Admin（系统代为执行）
**我希望**：当订阅被取消、过期或退款，或一次性购买的累计退款达到原支付金额时，系统自动撤销用户因该笔支付获得的 role
**从而**：防止权益被白嫖（支付墙最致命的失败模式），且撤销强度与积分发放/回收同等可靠

**【验收标准】**

**场景 1：订阅取消/过期触发 role 撤销**
```gherkin
Given 用户因订阅持有支付授予的 role
When 支付方推送 subscription.canceled 或 subscription.expired webhook
Then 系统撤销该用户因支付获得的 role（仅支付来源）
And 撤销操作幂等，重复 webhook 不产生二次错误
```

**场景 2：退款触发 role 撤销**
```gherkin
Given 用户因订阅持有支付授予的 role
When 支付方推送 refund.created / charge.refunded webhook
Then 系统撤销该用户因支付获得的 role
```

**场景 3：webhook 丢失/乱序的补偿**
```gherkin
Given webhook 发生丢失、重复或乱序
When 补偿框架介入
Then role 撤销最终一致（分钟级容忍窗口内达成一致）
And 绝不发生永久漏撤（漏撤视为 P0 故障）
```

**场景 4：一次性永久权益仅在累计全额退款时回收**
```gherkin
Given 用户因一次性购买持有永久 role
When 该笔支付的累计退款达到原支付金额 100%（单笔全额或多笔部分退款累计）
Then 系统仅撤销该笔支付来源的永久 role
And 管理员手工授予的同名 role 不受影响
And 正常取消或到期事件不撤销该永久 role
And 未达全额的部分退款（无论金额比例与笔数）保留该永久 role
```

**【备注】**
- 一次性购买的角色回收与积分回收的完整语义（单笔增量比例回收、退款单级幂等、争议例外）见 [refund-clawback.md](refund-clawback.md)（US-RC-001/002）。
- 一次性购买的争议/撤销事件当前不触发角色回收（Stripe、Creem 的争议处理均仅覆盖订阅）；部分退款保留的角色不因争议事件失效。「争议事件回收一次性角色」的能力缺口另立方案解决。

---

### 故事 6：第三方应用凭 role 一行判断解锁功能 [US-PW-006]

**优先级**: P0

**【用户故事】**
**作为**：接入 Herald 的第三方应用开发者（Third-party App）
**我希望**：直接用 Herald 现有 RBAC 运行时（require_permission）判断用户是否拥有支付授予的 role，无需自建 entitlement 门控逻辑
**从而**：接入成本最低，多应用门控行为一致

**【验收标准】**

**场景 1：用既有 RBAC 判断解锁**
```gherkin
Given 用户因支付持有 role: pro-member，该 role 绑定了自定义 resource.action 权限
When 第三方应用调用 Herald 权限检查
Then 应用无需关心 role 的来源（支付/手工），统一按 RBAC 判断放行
And Herald 不需要为 entitlement 新建权限空间
```

**场景 2：Herald 不感知权限语义**
```gherkin
Given 第三方应用为其功能定义了自定义权限（如 feature.advanced_export）
When 应用通过 Herald RBAC 配置 role→权限映射
Then Herald 仅作为键值映射管道，不存储/解释 features/quotas 语义
```
