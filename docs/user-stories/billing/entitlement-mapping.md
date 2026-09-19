# Entitlement Mapping 用户故事

> 角色定义见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)

## 用户故事

### 故事 1：查看 Provider Entitlement 映射 [US-EM-001]

**优先级**: P0

**【用户故事】**
**作为**：Realm Admin（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：查看支付方产品/价格到 Herald Entitlement 的映射列表
**从而**：了解每个支付方提供了哪些产品、每个产品映射到什么 entitlement、以及积分策略的同步状态

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：查看所有 Provider Entitlement 映射**
```gherkin
Given 我是 realm-1 的管理员
And realm-1 已配置 Stripe 和 Creem 支付平台
And 已从 Stripe 同步了 3 个产品、从 Creem 同步了 2 个产品
When 我访问 Billing 管理页面的 "Entitlement Mappings" 区域
Then 我看到所有 provider entitlement 映射列表
And 每条映射显示：
  | Payment Provider | Stripe       |
  | External Product | prod_xxxx    |
  | External Price   | price_yyyy   |
  | Entitlement Key  | pro-plan     |
  | Points Policy    | ✅ Synced    |
  | Synced At        | 2026-06-04   |
  | Enabled          | Yes          |
```

**场景 2：按支付方筛选映射**
```gherkin
Given 我是 realm-1 的管理员
And 存在来自多个支付方的映射
When 我选择支付方筛选 "Stripe"
Then 列表仅显示 Stripe 支付方的映射
```

**场景 3：映射尚未同步**
```gherkin
Given 我是 realm-1 的管理员
And realm-1 已配置支付平台但从未同步过产品
When 我访问 "Entitlement Mappings" 区域
Then 显示空状态提示："No provider products synced yet"
And 显示引导操作："Sync provider products to see available mappings"
```

---

### 故事 2：触发 Provider 产品同步 [US-EM-002]

**优先级**: P1

**【用户故事】**
**作为**：Realm Admin（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：手动触发支付方产品的全量同步
**从而**：确保 Herald 中的 entitlement 映射和积分策略与支付方保持一致

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：手动触发全量同步**
```gherkin
Given 我是 realm-1 的管理员
And realm-1 已配置 Stripe 支付平台
When 我点击 "Sync Provider Products" 按钮
And 我选择要同步的支付方 "Stripe"
Then 系统开始同步并显示进度指示
When 同步完成
Then 系统显示同步结果：
  | Products Synced  | 5          |
  | Prices Synced    | 12         |
  | Sync Status      | Completed  |
And Entitlement Mappings 列表更新为最新数据
```

**场景 2：同步失败**
```gherkin
Given 我是 realm-1 的管理员
And Stripe API 当前不可用
When 我触发全量同步
Then 系统显示同步失败提示："Failed to sync provider products"
And 显示失败原因和重试建议
And 现有映射数据不受影响，仍可正常使用
```

**场景 3：支付平台未配置**
```gherkin
Given 我是 realm-1 的管理员
And realm-1 未配置任何支付平台
When 我尝试触发同步
Then 系统提示："No payment providers configured. Please configure a payment provider first."
```

---

### 故事 3：Webhook 通过 Metadata 映射订阅 [US-EM-003]

**优先级**: P0

**【用户故事】**
**作为**：System
**我希望**：通过支付方 webhook metadata（而非本地 Product/Plan）将外部订阅映射到 Herald 订阅投影
**从而**：在移除本地 Product/Plan 后仍能正确处理订阅事件

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：Stripe 订阅激活**
```gherkin
Given Stripe 发送 subscription.active webhook
And webhook metadata 包含：
  | herald_realm_id       | realm-1      |
  | herald_client_app_id  | app-1        |
  | herald_user_id        | user-1       |
  | herald_entitlement_key | pro-plan    |
  | herald_billing_kind   | subscription |
And webhook 签名验证通过
When 系统处理该 webhook
Then Herald 创建/更新订阅投影，关联到 realm-1、app-1、user-1
And 订阅投影的 entitlement_key 为 "pro-plan"
And 订阅状态为 Active
```

**场景 2：Metadata 缺失 entitlement_key**
```gherkin
Given Stripe 发送 subscription.active webhook
And webhook metadata 缺少 herald_entitlement_key
When 系统处理该 webhook
Then 系统记录错误诊断："Missing herald_entitlement_key in webhook metadata"
And 订阅投影更新失败
And 错误对管理员可见
```

**场景 3：Checkout 创建时验证 Metadata**
```gherkin
Given 系统为用户发起 Stripe Checkout
When Checkout Session metadata 未包含 herald_realm_id 或 herald_entitlement_key
Then 系统拒绝创建 Checkout 并提示缺少必填 metadata
```

**场景 4：幂等处理**
```gherkin
Given 已处理过 Stripe event "evt_123"
When 系统再次收到相同 event "evt_123"
Then 系统识别为重复事件并跳过处理
And 订阅投影状态不变
```

---

### 故事 4：基于 Entitlement 应用积分策略 [US-EM-004]

**优先级**: P0

**【用户故事】**
**作为**：System
**我希望**：在订阅事件发生时基于 entitlement_key 查询和应用积分策略
**从而**：在移除 plan_id 后仍能正确发放、续期和回收积分

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：首次订阅积分发放**
```gherkin
Given 用户 user-1 在 realm-1 首次订阅 entitlement "pro-plan"
And "pro-plan" 的映射配置了积分分发规则（规则集合模型：目标积分账户 + 固定发放 1000 积分、有效期 30 天、订阅触发）
When 系统处理订阅激活事件
Then user-1 在规则指定的目标账户获得 1000 积分，有效期 30 天
And 积分发放记录与 entitlement_key "pro-plan" 关联
```

**场景 2：续费积分发放**
```gherkin
Given 用户 user-1 已订阅 entitlement "pro-plan"
And "pro-plan" 的映射为续费触发配置了固定发放 500 积分的分发规则
When 系统处理续费事件
Then user-1 获得 500 积分
And 续费发放次数 +1
```

**场景 3：Entitlement 无积分策略**
```gherkin
Given 用户订阅 entitlement "basic-plan"
And "basic-plan" 的映射未配置任何积分分发规则
When 系统处理订阅激活事件
Then 系统跳过积分发放
And 记录诊断："No points policy found for entitlement 'basic-plan'"
```

**场景 4：取消订阅积分回收**
```gherkin
Given 用户 user-1 通过 entitlement "pro-plan" 获得了积分
When 用户取消订阅
Then 系统根据回收规则处理积分
And 积分回收记录与 entitlement_key "pro-plan" 关联
```

---

### 故事 5：SDK 通过 Entitlement 查询订阅状态 [US-EM-005]

**优先级**: P0

**【用户故事】**
**作为**：Third-Party App（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：通过 entitlement_key 查询用户订阅状态
**从而**：在不依赖本地 Plan 的情况下做出访问控制决策

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：查询用户活跃订阅**
```gherkin
Given 用户 user-1 在 realm-1 有一个活跃订阅
And 订阅的 entitlement_key 为 "pro-plan"
When 第三方应用通过 SDK 查询 user-1 的订阅状态
Then 返回结果显示：
  | Has Subscription | Yes         |
  | Status           | Active      |
  | Has Access       | Yes         |
  | Entitlement Key  | pro-plan    |
  | Payment Provider | Stripe      |
```

**场景 2：用户无订阅**
```gherkin
Given 用户 user-2 在 realm-1 没有订阅
When 第三方应用通过 SDK 查询 user-2 的订阅状态
Then 返回结果显示：
  | Has Subscription | No  |
  | Has Access       | No  |
```

**场景 3：查询性能不依赖 Provider API**
```gherkin
Given 第三方应用查询用户订阅状态
And 当前 Stripe API 响应缓慢或不可用
When SDK 返回订阅查询结果
Then 结果来自 Herald 本地订阅投影
And 查询速度不受 Stripe API 影响
```

---

### 故事 6：查看订阅投影列表 [US-EM-006]

**优先级**: P0

**【用户故事】**
**作为**：Realm Admin（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：查看 Realm 内所有订阅投影列表
**从而**：了解用户的订阅状态、entitlement 和支付方信息

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：查看所有订阅投影**
```gherkin
Given 我是 realm-1 的管理员
And realm-1 有多个用户的订阅来自不同支付方
When 我访问 Billing 管理页面的 "Subscriptions" 区域
Then 我看到所有订阅投影列表
And 每条订阅显示：
  | User             | user-1          |
  | Entitlement Key  | pro-plan        |
  | Payment Provider | Stripe          |
  | Status           | Active          |
  | Current Period   | Jun 1 - Jul 1   |
  | Synced At        | 2026-06-04      |
```

**场景 2：按 entitlement 或状态筛选**
```gherkin
Given 我是 realm-1 的管理员
When 我选择 Entitlement 筛选 "pro-plan"
Then 列表仅显示 entitlement_key 为 "pro-plan" 的订阅
When 我选择状态筛选 "Active"
Then 列表仅显示活跃状态的订阅
```

**场景 3：查看订阅变更历史**
```gherkin
Given 我是 realm-1 的管理员
And 用户 user-1 的订阅有变更历史
When 我点击订阅 "user-1" 的详情
Then 我看到该订阅的完整变更时间线
And 每条变更记录显示：
  | Event Type        | upgraded     |
  | Entitlement       | pro-plan     |
  | Previous Entitlement | basic-plan   |
  | Changed At        | 2026-06-01   |
```

---

### 故事 7：同步并配置一个产品的多个价格 [US-EM-007]

**优先级**: P0

**【用户故事】**
**作为**：Realm Admin（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：当支付方一个产品存在多个价格（如月付与年付，或 recurring 与 one-time）时，能为每个价格分别配置计费类型、计费周期与积分策略
**从而**：让同一产品的不同价格成为各自独立、可正确授权与发放积分的购买选项，与 Stripe 的 Product→Price 模型对齐

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：同步多价格产品**
```gherkin
Given realm-1 已配置 Stripe 支付平台
And Stripe 产品 prod_pro 有两个价格 price_monthly（recurring/月）和 price_annual（recurring/年）
When 我触发 Stripe 产品同步
Then Entitlement Mappings 列表为 prod_pro 生成两条映射，分别对应 price_monthly 与 price_annual
And 每条映射可独立显示其外部价格、计费类型与计费周期
```

**场景 2：为不同价格配置不同积分策略**
```gherkin
Given prod_pro 已同步出 price_monthly 与 price_annual 两条映射
When 我将 price_monthly 配置为 entitlement_key=pro-plan、每月发放 1000 积分
And 将 price_annual 配置为 entitlement_key=pro-plan、每年发放 12000 积分
Then 两条映射各自保存独立的积分策略
And 两者可共享同一 entitlement_key "pro-plan"
```

**场景 3：为不同价格配置不同 entitlement**
```gherkin
Given 产品 prod_bundle 同步出 recurring 与 one-time 两个价格
When 我将 recurring 价格配置为 entitlement_key=pro-plan
And 将 one-time 价格配置为 entitlement_key=credit-100
Then 两个价格分别映射到不同 entitlement，互不影响
```

**场景 4：单价格产品只生成一条映射**
```gherkin
Given 产品 prod_basic 在 Stripe 只有一个价格
When 我同步该产品
Then 只为该产品生成一条映射
```

---

### 故事 8：Webhook 在一产品多价格时正确解析订阅归属 [US-EM-008]

**优先级**: P0

**【用户故事】**
**作为**：System
**我希望**：当一个产品存在多个价格映射时，webhook 能识别订阅实际归属哪个价格/entitlement
**从而**：首次订阅、续费、取消等事件按正确价格的积分策略发放或回收积分，不会因"产品多价格"而误用策略

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：metadata 携带 entitlement_key 时按 entitlement 解析**
```gherkin
Given 产品 prod_pro 有 price_monthly 与 price_annual 两条映射，共享 entitlement_key=pro-plan
And Stripe 发送 subscription.active webhook，metadata 含 herald_entitlement_key=pro-plan
And webhook 标识订阅使用的价格为 price_annual
When 系统处理该 webhook
Then 订阅投影按 entitlement_key=pro-plan 正确建立
And 积分按 price_annual 对应的年付策略发放
```

**场景 2：metadata 缺失 entitlement_key 时按价格回退解析**
```gherkin
Given 产品 prod_pro 有 price_monthly 与 price_annual 两条映射，且两条 entitlement_key 不同
And webhook metadata 缺少 herald_entitlement_key
And webhook 标识订阅使用价格为 price_annual
When 系统处理该 webhook
Then 系统按 (支付方, 产品, 价格) 命中 price_annual 对应的映射
And 按 price_annual 的 entitlement 与积分策略处理
```

**场景 3：无法唯一确定价格时显式失败**
```gherkin
Given 产品 prod_pro 有多个价格映射
And webhook 既无 herald_entitlement_key 也无法确定具体价格
When 系统处理该 webhook
Then 系统不静默使用默认策略
And 记录诊断并让错误对管理员可见
```

---

### 故事 9：用户购买多价格产品的指定价格 [US-EM-009]

**优先级**: P0

**【用户故事】**
**作为**：Regular User（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：在购买一个有多价格的产品时，能选择具体价格（如月付或年付）并按所选价格完成购买
**从而**：我买到的是我选定的计费方式，支付方按真实价格收费，Herald 按该价格授权与发放

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：选择并购买具体价格**
```gherkin
Given 产品 prod_pro 在购买页展示 price_monthly 与 price_annual 两个可选价格
And 两者均已启用且配置了可用的支付平台
When 我选择 price_annual 并发起购买
Then checkout 指向 price_annual 对应的真实支付方价格
And 购买完成后我获得 price_annual 对应的 entitlement 与积分
```

**场景 2：价格未启用或未配置支付平台**
```gherkin
Given 价格 price_annual 未启用，或其对应支付平台未在 Realm 启用
When 我查看该产品的购买选项
Then 该价格不可购买或被禁用，并给出明确提示
```

---

### 故事 10：同步时携带 Stripe 商户自定义 metadata，并在管理端可见 [US-BL-SYNC-001]

**优先级**: P1

**【用户故事】**
**作为**：Admin Realm 管理员（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：在执行 Stripe 产品同步时，把 Stripe `Product.metadata`、`Price.metadata` 这类商户自定义键值对一并同步到本地，并在 entitlement mapping 详情中可查看
**从而**：当商户在 Stripe 后台用 metadata 标注产品用途（例如 `herald_mapping_id`、`tier`、`internal_sku`）时，我能直接在 Herald 里看到这些标注，据此配置 entitlement，而不必切换回 Stripe 后台

> 说明：Creem 的 Product 对象在官方 API 响应中无 `metadata` 字段（其 `metadata` 是 checkout 会话级），因此本故事仅适用于 Stripe。Creem 产品不要求 metadata 同步。

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：同步一个带 metadata 的 Stripe 产品**
```gherkin
Given Stripe 后台的产品 P 带有 metadata {"tier":"pro","internal_sku":"H-001"}
And 该产品下有一个活跃价格 price_abc
When 管理员在 entitlement mappings 页对 Stripe 触发同步
Then 同步完成后，mapping 详情里能看到这些 metadata（tier=pro、internal_sku=H-001）
And 既有字段（产品名、价格、币种、计费类型）继续存在，不被覆盖丢失
```

**场景 2：同步一个不带 metadata 的 Stripe 产品**
```gherkin
Given Stripe 后台的产品 P 没有任何 metadata
When 管理员触发同步
Then 该 mapping 的 metadata 视为空（显示为「无」或省略），不报错
And 产品名、价格等其它字段照常同步
```

**场景 3：Creem 产品不涉及 metadata**
```gherkin
Given 一条 Creem 产品的 mapping
When 管理员查看其详情
Then 其 metadata 视为空（显示为「无」或省略），不报错、不显示伪造字段
And 产品名、价格等其它字段照常展示
```

**场景 4：重复同步以最新为准**
```gherkin
Given 上一次同步后商户在 Stripe 后台把 P 的 metadata 从 {"tier":"pro"} 改成 {"tier":"team"}
When 管理员再次触发同步
Then mapping 详情里看到的 metadata 变为 tier=team
```

---

### 故事 11：在 mapping 列表里看到产品名，便于识别 [US-BL-SYNC-002]

**优先级**: P0

**【用户故事】**
**作为**：Admin Realm 管理员
**我希望**：在 entitlement mappings 列表（尤其是按产品分组后的卡片/行）里直接看到从 provider 同步过来的产品名，而不只看到一串外部 id
**从而**：一眼识别每个外部产品/价格对应哪个商品，不必点开详情或对照 Stripe/Creem 后台

**【验收标准】**

**场景 1：列表显示产品名**
```gherkin
Given 一条 mapping 的同步展示信息里已存有 name="Pro 月度订阅"
When 管理员打开 entitlement mappings 列表
Then 该产品分组或行显示 "Pro 月度订阅"
And 当 name 缺失时退回到外部产品 id 作为可识别标签
```

**场景 2：产品名过滤器**
```gherkin
Given 管理员在产品过滤器里输入 "Pro"
Then 列表只保留产品名或外部 id 命中 "Pro" 的分组
```

---

### 故事 12：产品价格按 provider 单位正确展示，不混淆 Stripe 与 Creem [US-BL-SYNC-003]

**优先级**: P0

**【用户故事】**
**作为**：Admin Realm 管理员
**我希望**：在 entitlement mapping 详情/列表里看到的价格金额，Stripe 产品按整数最小货币单位（分）换算展示，Creem 产品按其原值展示，不会因为我用了哪个 provider 而出现金额错位
**从而**：我能正确识别每条 mapping 对应的真实售价，避免误配 entitlement

**【验收标准】**

**场景 1：Stripe 价格正确展示**
```gherkin
Given 一条 Stripe 同步来的 mapping，其价格为最小货币单位整数（例如 999 表示 9.99）
When 管理员查看该 mapping
Then 展示价格按 Stripe 单位正确换算（例如 9.99），不会被当成 Creem 字符串值再除以 100
```

**场景 2：Creem 价格正确展示**
```gherkin
Given 一条 Creem 同步来的 mapping，价格为字符串 "9.99"
When 管理员查看该 mapping
Then 展示为 9.99，不会因为同步路径混用而被二次缩放
```

---

### 故事 13：计费周期以 Stripe 为准、只读且不被人工覆盖 [US-BL-SYNC-004]

> Creem `billing_period` 取值形如 `every-month` / `every-year`，前端做文案映射，缺失时显示原文；Stripe `Price.recurring.interval` 为原始语义来源。

**优先级**: P0

**【用户故事】**
**作为**：Admin Realm 管理员
**我希望**：每条 entitlement mapping 的计费周期（订阅周期，如 month / year）始终与 Stripe `Price.recurring.interval` 一致，前端只读、不接受我手动输入；保存/更新时即使提交了周期值也不覆盖同步值
**从而**：Herald 创建 Stripe Checkout / 订阅时使用的周期与商户在 Stripe 后台配置的真实计费周期一致，避免" Stripe 上是年订阅、Herald 却按月下单"这类导致真实扣款周期错误的严重问题

> 说明：计费周期在 Stripe 端的语义来源是 `Price.recurring.interval`（day/week/month/year），Herald 不做独立配置或人工覆盖。Creem 侧以其产品响应中的 `billing_period` 字段（取值形如 `every-month` / `every-year`）为同步来源并只读展示，字段缺失时按空（"—"）展示，不推断、不伪造。

**【验收标准】**

**场景 1：Stripe 产品周期正确展示且只读**
```gherkin
Given 一条 Stripe 同步来的 mapping，其 Price.recurring.interval = "year"
When 管理员查看该 mapping
Then 计费周期展示为 "year"（或等价的周期文案），且该字段为只读，无法手动编辑
```

**场景 2：人工提交值不覆盖同步值**
```gherkin
Given 一条 Stripe 同步来的 mapping，同步周期为 "month"
When 通过保存/更新接口提交了一个与同步值不一致的计费周期（如 "year"）
Then 持久化的计费周期仍以同步值为准（month），不接受人工覆盖
And 下次同步仍以 Stripe 当前 interval 覆盖本地
```

**场景 3：Creem 产品周期展示同步值**
```gherkin
Given 一条 Creem 同步来的 mapping，其产品响应含 billing_period
When 管理员查看该 mapping
Then 计费周期展示为同步值（如 every-month 映射为「月」），映射缺失时显示原文，不报错
And 若该 Creem 产品响应不含 billing_period 字段，则按空（"—"）展示，不伪造
```

---

## 业务规则总结

### Provider Ownership 边界
1. **商业目录**：支付方拥有 Product、Price、Checkout、Customer billing、Subscription lifecycle、Invoice/payment
2. **Herald 职责**：Realm/Client App 边界、User binding、Access entitlement projection、Webhook 幂等、积分策略和账本、SDK 读模型
3. **Herald 不维护**：独立的本地商业目录或可编辑的订阅套餐

### Metadata 契约
1. **统一前缀**：所有 Herald metadata key 使用 `herald_` 前缀
2. **必填 metadata**：`herald_realm_id`、`herald_client_app_id`、`herald_user_id`、`herald_entitlement_key`
3. **计费类型**：`herald_billing_kind` 值为 `subscription` 或 `points_package`
4. **Stripe 分层**：稳定映射放 Product/Price metadata，请求特定信息放 Checkout Session/Subscription metadata
5. **Creem**：metadata 写入 checkout 请求，后续 webhook 返回该 metadata

### Entitlement 映射规则
1. **Source of truth**：Herald 本地 provider-to-entitlement mapping 为 entitlement 映射和积分策略的 source of truth；Stripe Product/Price metadata 可作为导入来源
2. **本地角色**：Herald 维护 provider-to-entitlement 映射作为 allowlist 和同步缓存
3. **同步机制**：支持 webhook 触发增量同步和管理员手动触发全量同步
4. **降级策略**：同步失败时本地缓存可独立服务 webhook；同步失败不应静默降级为默认策略

### 订阅投影规则
1. **投影语义**：Subscription 是支付方订阅状态的本地投影，不是 Herald 拥有的订阅
2. **字段边界**：保留 realm_id、client_app_id、user_id、entitlement_key、status、period 信息、provider metadata
3. **移除字段**：不再维护 plan_id、本地 tier、本地 billing_period

### 积分策略规则
1. **策略来源**：积分策略的 source of truth 是 Herald 本地 mapping/entitlement policy；Stripe Product/Price metadata 可作为导入来源，Creem 必须在 Herald 中配置
2. **策略查询**：按 entitlement_key 从本地配置查询
3. **覆盖场景**：首次订阅发放、续费发放、取消回收、退款回收、升级/降级处理
4. **管理员编辑**：管理员可在 Herald 中查看和修改积分策略；修改的是 Herald 业务规则，不回写 provider

### 编目边界
1. Herald 不提供本地 Product CRUD
2. Herald 不提供本地 SubscriptionPlan CRUD
3. 支付平台商品通过 Entitlement Mapping 关联权益、积分策略和 Client App
4. 业务链路使用 `entitlement_key`，不使用本地 `plan_id`

### 多价格规则
1. **Price 一等概念**：Herald 的 provider 模型引入 Price 维度，对齐 Stripe 的 Product→Price；一个产品可拥有多个价格，每个价格是独立可购与可配置单元
2. **按价格配置**：entitlement_key、计费类型、计费周期与积分策略均按价格配置；同一产品的多个价格可共享 entitlement_key（月付/年付同属 pro-plan）或映射到不同 entitlement
3. **单价格产品**：只有一个价格的产品（含 Creem 等无 price 概念的支付方）自然只有一行映射
4. **Price-aware 同步**：产品同步按价格粒度建立/更新映射，不再仅取首个价格
5. **Price-aware 解析**：webhook 解析优先使用 metadata 的 herald_entitlement_key，回退时按 (支付方, 产品, 价格) 命中映射；无法唯一确定时 fail loud
6. **Price-aware 购买**：checkout 引用真实 provider 价格（对有 price 概念的支付方），不再为每次购买重建临时价格

### 产品同步展示规则
1. **产品名主标签**：列表/分组的主标签优先取同步产品名，缺失时回退外部产品 id，二者皆不可用时给可识别占位；不显示空标签
2. **价格按 provider 区分单位**：Stripe 取最小货币单位整数换算为主货币单位；Creem 按原值展示；单位换算由 provider 来源驱动，不跨 provider 共享换算分支
3. **计费周期以 Stripe 为准且只读**：计费周期取 Stripe `Price.recurring.interval`（Creem 取其产品响应 `billing_period`，缺失按"—"），前端只读，保存/更新不得以人工值覆盖同步值；重新同步以 provider 当前值为准
4. **metadata 同步仅适用 Stripe**：Stripe `Product.metadata`/`Price.metadata` 跟随同步进入本地展示信息，只读、不编辑、不回写；Creem Product 无原生 metadata，按空处理，不伪造
5. **同步覆盖展示字段**：重新同步时 name/description/价格/metadata/计费周期以最新一次为准；entitlement_key、points、grant 策略、quota 等业务字段不被同步覆盖
6. **credit/额度周期与计费周期解耦**：`billing_period` 与 `quota_windows` 独立、不整除、不对齐；同步不读取/校验 quota_windows；额度授予由续费 webhook 事件驱动按 (period_start, period_end) 锚定，非日历清零；支持纯窗口、无周期总额模型
7. **缺失即空**：provider 未提供 name/description/metadata/计费周期时按空处理，不报错、不阻塞同步
8. **展示位**：所有同步展示信息（含 metadata）存放于既有 provider_product_info 结构内，作为展示数据，不作为计费/扣点依据
9. **权限可见性**：产品名、价格、metadata 仅在 Admin Realm（具备 entitlement mapping 管理权限）页面可见；普通用户侧不展示 provider 内部信息

---

## 相关文档

- **PRD**: [docs/prd/billing/subscription.md](/docs/prd/billing/subscription.md) - 订阅计费 PRD（含 Entitlement 映射、Metadata 契约）
- **PRD**: [docs/prd/billing/points.md](/docs/prd/billing/points.md) - 积分系统 PRD
- **PRD**: [docs/prd/billing/subscription.md](/docs/prd/billing/subscription.md) - 订阅计费 PRD（含多价格、产品同步、产品名/价格单位/metadata/计费周期）
- **需求来源**: Product and Subscription Local Model Reduction — 移除本地 Product/Plan 商业目录，将目录和订阅生命周期交给支付方
