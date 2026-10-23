# Realm Admin / Regular User / Herald 系统 用户故事

> 角色定义见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)

## 用户故事

### 故事 1：配置发票策略 [US-IF-001]

**优先级**: P0

**【用户故事】**
**作为**：Realm Admin（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：配置 Realm 的发票策略和各支付平台的发票能力开关
**从而**：控制发票来源，选择使用外部平台发票或 Herald 自研发票

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：设置发票策略为 provider_first**
```gherkin
Given 我是 realm-1 的管理员
And realm-1 已配置 Stripe 支付平台
When 我在 Billing 设置中将发票策略设置为 "Provider First"
And 我启用 Stripe 的外部发票能力
Then 配置保存成功
And 后续通过 Stripe 支付的交易，发票由 Stripe 提供（只读展示）
And 其他支付平台的交易仍可使用 Herald 自研发票
```

**场景 2：设置发票策略为 manual_only**
```gherkin
Given 我是 realm-1 的管理员
When 我在 Billing 设置中将发票策略设置为 "Manual Only"
Then 所有非 MoR 交易使用 Herald 自研发票系统
And 管理员可以手动创建、编辑、开具发票
```

**场景 3：设置发票策略为 none**
```gherkin
Given 我是 realm-1 的管理员
When 我在 Billing 设置中将发票策略设置为 "None"
Then Herald 不提供自研发票入口
And 外部平台自带的发票仍可只读展示
```

**场景 4：Creem MoR 不可被 manual 覆盖**
```gherkin
Given 我是 realm-1 的管理员
And 发票策略为 "Manual Only"
And 存在一笔通过 Creem 支付的交易
When 我尝试为该 Creem 交易创建 Herald 手动发票
Then 系统拒绝操作
And 提示 "Creem transactions are managed by Creem as Merchant of Record"
```

**场景 5：未启用的 provider 不展示发票配置**
```gherkin
Given 我是 realm-1 的管理员
And realm-1 未配置微信电子发票能力
When 我查看发票策略配置页面
Then 微信支付平台不展示外部发票能力开关
```

---

### 故事 2：系统同步 Stripe 发票 [US-IF-002]

**优先级**: P0

**【用户故事】**
**作为**：Herald 系统（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：通过 Stripe webhook 自动同步 Stripe 发票数据到 Herald
**从而**：在 Herald 中只读展示 Stripe 管理的发票，无需人工干预

**【验收标准】**

**场景 1：Stripe invoice.created 事件同步**
```gherkin
Given Realm 配置了 Stripe 支付平台且启用了外部发票能力
And Stripe Invoicing 为某客户创建了发票
When Herald 收到 Stripe 的 "invoice.created" webhook 事件
Then Herald 在本地创建一条来源为 Stripe 的发票记录
And 发票状态为草稿
And 记录 Stripe 发票 ID 和原始数据快照
```

**场景 2：Stripe invoice.finalized 事件同步**
```gherkin
Given Herald 已同步一条 Stripe 发票（状态为草稿）
When Herald 收到 Stripe 的 "invoice.finalized" webhook 事件
Then 发票状态更新为已开具
And 更新外部托管页面 URL 和 PDF 下载 URL
```

**场景 3：Stripe invoice.voided 事件同步**
```gherkin
Given Herald 已同步一条 Stripe 发票
When Herald 收到 Stripe 的 "invoice.voided" webhook 事件
Then 发票状态更新为已作废
```

**场景 4：Stripe invoice.paid 事件同步**
```gherkin
Given Herald 已同步一条 Stripe 发票
When Herald 收到 Stripe 的 "invoice.paid" webhook 事件
Then 发票状态更新为已支付
```

**场景 5：重复 webhook 不产生重复发票**
```gherkin
Given Herald 已同步 Stripe 发票 in_abc123
When Herald 再次收到同一 Stripe 发票的事件
Then Herald 更新已有记录而非创建重复发票
```

**场景 6：Stripe 一次性购买发票同步**
```gherkin
Given Realm 配置了 Stripe 支付平台且启用了外部发票能力
And 一笔通过 Stripe Checkout mode=payment 的一次性购买支付成功
When Herald 收到 Stripe 的 "checkout.session.completed" webhook 事件
And 事件 mode 为 "payment"
Then Herald 在本地创建一条来源为 Stripe 的外部发票记录
And 发票状态为已支付
And 记录 Stripe invoice ID（in_...）和 payment intent ID（pi_...）
And 记录金额和币种
```

**场景 7：Stripe 一次性购买发票不可被 Herald 修改**
```gherkin
Given Herald 存在一条来源为 Stripe 的一次性购买发票
When 任何用户尝试通过 Herald 编辑、开具、作废或标记该发票已付
Then 系统拒绝操作
And 提示 "This invoice is managed by the payment provider"
```

**场景 8：Stripe 一次性购买已有外部发票时不可再申请 Herald 手动发票**
```gherkin
Given 存在一笔通过 Stripe 一次性购买完成的支付
And 该交易已有 Stripe 同步的外部发票
When 用户尝试为该交易申请 Herald 手动发票
Then 系统拒绝操作
And 提示该交易已有外部发票
```

---

### 故事 3：系统同步 Creem 交易税务数据 [US-IF-003]

**优先级**: P0

**【用户故事】**
**作为**：Herald 系统
**我希望**：同步 Creem MoR 交易的税务数据到 Herald
**从而**：在 Herald 中只读展示 Creem 管理的发票和税务信息

**【验收标准】**

**场景 1：Creem 支付成功后同步交易税务数据**
```gherkin
Given Realm 配置了 Creem 支付平台
And 一笔 Creem 支付成功
When 系统处理 Creem 交易回调
Then Herald 创建一条来源为 Creem 的发票记录
And 记录交易金额、税额、税区等税务信息
And 记录 Creem 交易 ID 作为外部发票标识
```

**场景 2：Creem 发票不可被 Herald 修改**
```gherkin
Given Herald 存在一条来源为 Creem 的发票
When 任何用户尝试通过 Herald 编辑、开具、作废或标记该发票已付
Then 系统拒绝操作
And 提示 "This invoice is managed by the payment provider"
```

**场景 3：Creem 交易不允许创建 Herald 手动发票**
```gherkin
Given 存在一笔通过 Creem 完成的支付
When 用户尝试为该交易申请 Herald 手动发票
Then 系统拒绝操作
And 提示该交易的发票由 Creem 管理
```

---

### 故事 4：查看外部 Provider 发票（管理员） [US-IF-004]

**优先级**: P0

**【用户故事】**
**作为**：Realm Admin
**我希望**：在发票列表中查看外部 provider 同步的发票
**从而**：了解所有发票全貌，包括自研和外部 provider 来源

**【验收标准】**

**场景 1：发票列表显示 provider 来源**
```gherkin
Given 我是 realm-1 的管理员
And realm-1 既有 Herald 自研发票，也有 Stripe 同步的发票
When 我查看发票列表
Then 列表显示每条发票的来源标识（Manual / Stripe / Creem）
And 列表同时展示自研和外部 provider 的发票
```

**场景 2：外部发票详情为只读**
```gherkin
Given 我在查看一条来自 Stripe 的发票详情
Then 我可以看到发票的完整信息（编号、金额、行项目、税务明细）
And 我无法看到 "Edit"、"Issue"、"Void"、"Mark as Paid" 按钮
And 详情页显示 "This invoice is managed by Stripe"
And 如有外部托管 URL，显示 "View in Stripe" 链接
```

**场景 3：自研发票功能不受影响**
```gherkin
Given 我在查看一条 Herald 自研发票详情
Then 所有操作按钮按现有逻辑启用或禁用
And 自研发票的全部功能保持不变
```

**场景 4：按 provider 筛选发票**
```gherkin
Given 我在发票列表页面
And 列表中包含来自不同 provider 的发票
When 我选择 provider 筛选为 "Stripe"
Then 列表只显示 Stripe 同步的发票
```

---

### 故事 5：查看外部 Provider 发票（普通用户） [US-IF-005]

**优先级**: P1

**【用户故事】**
**作为**：Regular User
**我希望**：在"我的发票"中查看外部 provider 同步的发票
**从而**：了解我的所有发票，无论来自哪个平台

**【验收标准】**

**场景 1：我的发票列表显示外部 provider 发票**
```gherkin
Given 我是普通用户 user@example.com
And 我有通过 Stripe 支付的交易
And Stripe 已生成发票并同步到 Herald
When 我查看"我的发票"页面
Then 列表中包含 Stripe 同步的发票
And 发票带有 Stripe 来源标识
```

**场景 2：外部发票详情为只读**
```gherkin
Given 我在查看一条 Stripe 同步的发票
Then 我可以看到发票详情（编号、金额、状态）
And 我无法申请开具或编辑该发票
And 如有外部托管 URL，显示 "View in Stripe" 链接
```

**场景 3：用户为 Creem 交易申请发票被拒绝**
```gherkin
Given 我有一笔通过 Creem 支付的交易
When 我尝试为该交易申请 Herald 发票
Then 系统提示该交易的发票由 Creem 管理
And 申请入口不可用
```

---

### 故事 6：下载外部发票 PDF 或查看 Provider 页面 [US-IF-006]

**优先级**: P1

**【用户故事】**
**作为**：Realm Admin / Regular User
**我希望**：下载或查看外部 provider 管理的发票 PDF
**从而**：获得正式的发票文档

**【验收标准】**

**场景 1：Stripe 发票有 PDF URL 时直接下载**
```gherkin
Given 存在一条来自 Stripe 的发票
And 该发票有外部 PDF 下载 URL
When 我点击下载 PDF
Then 系统将我重定向到 Stripe 的 PDF 下载链接
```

**场景 2：外部发票有托管页面 URL**
```gherkin
Given 存在一条来自 Stripe 的发票
And 该发票有外部托管页面 URL
When 我点击 "View in Stripe" 链接
Then 系统在新标签页打开 Stripe 托管的发票页面
```

**场景 3：Creem 发票无 PDF URL 时跳转 Creem 平台**
```gherkin
Given 存在一条来自 Creem 的发票
And 该发票无外部 PDF 下载 URL
When 我查看发票详情
Then 详情页提示 "Invoice managed by Creem"
And 如有 Creem 平台链接则显示 "View in Creem"
```

**场景 4：自研发票 PDF 功能不受影响**
```gherkin
Given 存在一条 Herald 自研发票
When 我下载该发票的 PDF
Then 系统使用 Herald 内置 PDF 生成器生成并下载
```

---

### 故事 7：系统同步 Stripe Credit Note [US-IF-007]

**优先级**: P0

**【用户故事】**
**作为**：Herald 系统（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：通过 Stripe webhook 自动同步 Credit Note 数据到关联的发票
**从而**：在退款发生后正确反映发票的退款金额与剩余应付，保留税务合规所需的 Credit Note 记录

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：Stripe credit_note.created 事件触发同步**
```gherkin
Given Realm 配置了 Stripe 支付平台且启用了外部发票能力
And Herald 已同步一条 Stripe 发票（状态为已支付）
And Stripe Dashboard 中针对该发票开具了一张 Credit Note
When Herald 收到 Stripe 的 "credit_note.created" webhook 事件
Then Herald 在本地记录这张 Credit Note（包含金额、币种、关联发票）
And 关联发票的退款金额增加对应数额
And 关联发票的剩余应付减少对应数额
And 发票主状态保持不变（仍为已支付）
```

**场景 2：部分退款支持（同一发票多张 Credit Note）**
```gherkin
Given Herald 已同步一条 Stripe 发票，总额为 100 元
And 该发票已有一张 30 元的 Credit Note
When Stripe Dashboard 又针对该发票开具一张 20 元的 Credit Note
And Herald 收到对应的 "credit_note.created" 事件
Then Herald 记录这张新的 Credit Note
And 发票的累计退款金额更新为 50 元
And 发票的剩余应付更新为 50 元
And 发票主状态保持已支付
```

**场景 3：Credit Note 不作废原发票**
```gherkin
Given Herald 已同步一条状态为已支付的 Stripe 发票
When 该发票收到一张 Credit Note
Then 发票状态保持已支付，不变为已作废
And 发票在列表中仍可见
And 详情页显示退款金额与剩余应付
```

**场景 4：charge.refunded 与 credit_note.created 各自独立处理**
```gherkin
Given Stripe 对一笔 Charge 执行了退款
And 同时对该 Charge 关联的 Invoice 开具了 Credit Note
When Herald 收到 "charge.refunded" 事件
Then Herald 按现有积分回收规则回收积分（与 Credit Note 处理相互独立）
When Herald 收到 "credit_note.created" 事件
Then Herald 处理发票退款金额更新，不重复执行积分回收
```

**场景 5：重复 credit_note.created 事件幂等**
```gherkin
Given Herald 已处理过某张 Stripe Credit Note 的 created 事件
When Herald 再次收到同一张 Credit Note 的 created 事件
Then Herald 不创建重复记录
And 发票的退款金额不被重复累加
```

**场景 6：未关联发票的退款**
```gherkin
Given Stripe 收到一笔退款请求
And 该退款对应的 Charge 没有关联任何 Stripe Invoice（例如一次性 Checkout 购买但未生成 Invoice）
When Herald 收到 "charge.refunded" 事件
Then Herald 仅执行积分回收
And 不需要等待 credit_note.created 事件
And 不影响任何发票记录
```

---

### 故事 8：管理员查看发票退款信息与 Credit Note 列表 [US-IF-008]

**优先级**: P0

**【用户故事】**
**作为**：Realm Admin（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：在发票详情中查看累计退款金额、剩余应付与 Credit Note 列表
**从而**：掌握发票的真实状态，避免按已退款的发票金额向税务机关申报

**【验收标准】**

**场景 1：详情页展示退款金额与剩余应付**
```gherkin
Given 我是 realm-1 的管理员
And 存在一条来自 Stripe 的发票，总额为 100 元
And 该发票已有一张 30 元的 Credit Note
When 我查看该发票详情
Then 详情页同时展示：
  | 总额     | 100 元     |
  | 已退款   | 30 元      |
  | 剩余应付 | 70 元      |
And 发票主状态显示为 "已支付"
```

**场景 2：查看 Credit Note 列表**
```gherkin
Given 我在查看一条已有 Credit Note 的 Stripe 发票详情
Then 详情页展示 Credit Note 列表
And 每条 Credit Note 显示：编号、开具时间、金额、币种
And Credit Note 列表为只读，不可在 Herald 中创建或修改
```

**场景 3：发票列表展示退款标注**
```gherkin
Given 我是 realm-1 的管理员
And 发票列表中存在一张已部分退款的 Stripe 发票
When 我查看发票列表
Then 该发票行的金额列旁展示退款标注（如 "Refunded 30/100"）
And 主状态仍为已支付
```

**场景 4：无 Credit Note 的发票正常展示**
```gherkin
Given 我在查看一条无 Credit Note 的发票详情
Then 详情页显示"已退款"为 0、"剩余应付"等于总额
And 不展示 Credit Note 列表区域
```

**场景 5：自研发票同样展示退款维度**
```gherkin
Given 我是 realm-1 的管理员
And 存在一张 Herald 自研发票（provider=manual）
And 该发票已被管理员记录过线下退款
When 我查看该发票详情
Then 详情页展示"总额 / 已退款 / 剩余应付"区域
And 展示 Credit Note 列表（来源标识为 Manual）
And 自研发票的其他功能（编辑/开具/作废/标记已付）按现有状态机逻辑保持不变
```

**场景 6：Creem 发票不展示退款维度**
```gherkin
Given 我在查看一张来自 Creem 的发票（provider=creem）
Then 详情页按现有逻辑展示，不显示 Credit Note 区域
And 不展示"已退款 / 剩余应付"维度
```

---

### 故事 9：普通用户查看退款标注 [US-IF-009]

**优先级**: P1

**【用户故事】**
**作为**：Regular User（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：在"我的发票"中看到自己已退款发票的退款标注与剩余应付
**从而**：了解发票的真实金额，避免按已退款金额重复报销或申报

**【验收标准】**

**场景 1：我的发票列表展示退款标注**
```gherkin
Given 我是普通用户 user@example.com
And 我有一张来自 Stripe 的发票，总额 100 元，已退款 30 元
When 我查看"我的发票"页面
Then 该发票行展示退款标注（如 "Refunded 30/100"）
And 状态仍为已支付
```

**场景 2：详情页展示剩余应付**
```gherkin
Given 我在查看一张已部分退款的发票详情
Then 详情页同时展示总额、已退款、剩余应付
And 我无法看到 Credit Note 的内部编号（只看到对账所需的退款摘要）
```

**场景 3：自研发票同样展示退款标注**
```gherkin
Given 我是普通用户 user@example.com
And 我有一张 Herald 自研发票，已被管理员记录过线下退款
When 我查看"我的发票"页面
Then 该发票行展示退款标注（如 "Refunded 30/100"）
And 状态仍为已支付
```

**场景 4：Creem 发票不展示退款标注**
```gherkin
Given 我在查看一张来自 Creem 的发票
Then 详情页按现有逻辑展示
And 不显示"已退款 / 剩余应付"维度
```

---

### 故事 10：管理员记录自研发票的线下退款 [US-IF-010]

**优先级**: P0

**【用户故事】**
**作为**：Realm Admin（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：为已付款的 Herald 自研发票记录线下退款（生成 Manual Credit Note）
**从而**：在 Herald 中保留完整的退款凭证，使发票金额与实际收款一致，满足税务合规

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：为已付款自研发票记录退款**
```gherkin
Given 我是 realm-1 的管理员
And 存在一张状态为 "paid" 的 Herald 自研发票（provider=manual）
And 我已通过线下渠道向客户退款
When 我在该发票详情页点击 "Record Refund" 按钮
And 我在弹窗中填写退款金额、币种与原因（memo）
And 我提交表单
Then 系统创建一条 Manual Credit Note 记录，关联该发票
And 记录包含：金额、币种、原因、操作者、操作时间
And 发票主状态保持 "paid" 不变
And 发票的"已退款"金额增加对应数额，"剩余应付"减少对应数额
```

**场景 2：部分退款支持（同一发票多次记录）**
```gherkin
Given 存在一张总额为 100 元的已付款自研发票
And 该发票已记录过一次 30 元的退款
When 我再次点击 "Record Refund" 并填写 20 元
Then 系统创建第二条 Manual Credit Note
And 发票累计退款金额更新为 50 元，剩余应付为 50 元
And 发票主状态保持 "paid"
```

**场景 3：退款金额不得超过剩余应付**
```gherkin
Given 存在一张总额为 100 元、已退款 80 元的自研发票
When 我尝试再记录一笔 30 元的退款
Then 系统拒绝操作
And 提示 "Refund amount exceeds remaining payable"
```

**场景 4：Stripe / Creem 发票不可手工记录退款**
```gherkin
Given 存在一张来自 Stripe 或 Creem 的外部发票
When 我查看该发票详情
Then 我看不到 "Record Refund" 按钮
And 即使我尝试通过任何方式创建 Manual Credit Note，系统都拒绝
And 提示 "Refunds for this provider are managed externally"
```

**场景 5：Manual Credit Note 创建后不可删除**
```gherkin
Given 我已为某发票创建过一条 Manual Credit Note
When 我尝试删除或撤销该 Credit Note
Then 系统拒绝操作
And 提示 Manual Credit Note 创建后不可撤销
And 如有错误，需通过其他渠道处理（如线下补偿或客服介入）
```

**场景 6：草稿/已作废发票不可记录退款**
```gherkin
Given 存在一张状态为 "draft" 或 "void" 的自研发票
When 我尝试为其记录退款
Then 系统拒绝操作
And 提示 "Only paid invoices support refund recording"
```

---

### 故事 11：系统处理 Stripe Credit Note 作废 [US-IF-011]

**优先级**: P0

**【用户故事】**
**作为**：Herald 系统（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：在 Stripe Credit Note 被作废时同步作废状态，并恢复关联发票的剩余应付
**从而**：保持 Herald 发票退款金额与 Stripe Dashboard 一致，避免按已恢复金额的发票进行税务申报

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：Stripe credit_note.voided 事件触发作废同步**
```gherkin
Given Realm 配置了 Stripe 支付平台且启用了外部发票能力
And Herald 已同步一条 Stripe 发票
And 该发票已有一张状态为有效的 Credit Note，金额为 30 元
When Herald 收到 Stripe 的 "credit_note.voided" webhook 事件
Then 该 Credit Note 状态变为已作废
And 关联发票的累计退款金额减少 30 元
And 关联发票的剩余应付增加 30 元
And 发票主状态保持不变（仍为已支付）
```

**场景 2：重复 credit_note.voided 事件幂等**
```gherkin
Given Herald 已处理过某张 Stripe Credit Note 的 voided 事件
When Herald 再次收到同一张 Credit Note 的 voided 事件
Then 发票的退款金额不被重复扣减
And 发票的剩余应付不被重复增加
```

**场景 3：本地不存在对应 Credit Note 时拒绝创建孤儿记录**
```gherkin
Given Stripe 发送了一张 Herald 本地未记录的 Credit Note 的 voided 事件
When Herald 处理该事件
Then 系统不创建新的 Credit Note 记录
And 返回错误以触发 Stripe 重投递
```

---

## 用户故事优先级汇总

| 优先级 | 用户故事数量 | 关键故事 |
|--------|-------------|---------|
| P0 | 8 | 配置发票策略、同步 Stripe 发票、同步 Creem 税务、管理员查看外部发票、同步 Stripe Credit Note、管理员查看发票退款信息、管理员记录自研发票线下退款、处理 Stripe Credit Note 作废 |
| P1 | 3 | 用户查看外部发票、下载外部 PDF、用户查看退款标注 |
| P2 | 0 | - |

---

## 相关文档

- **PRD**: `docs/prd/billing/invoice.md` - Invoice 发票管理 PRD（含 Provider 发票同步）
- **PRD**: `docs/prd/billing/invoice.md` - Invoice PRD（含 Provider Fallback 与 Credit Note）
