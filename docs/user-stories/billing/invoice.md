# Realm Admin / Regular User / Herald 系统用户故事

> 角色定义见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)

## 用户故事

### 故事 1：创建发票 [US-IV-001]

**优先级**: P0

**【用户故事】**
**作为**：Realm Admin（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：创建发票草稿，添加行项目和费用信息
**从而**：为订阅或服务生成正式账单

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：创建包含行项目的发票草稿**
```gherkin
Given 我是 realm-1 的管理员
And 已存在账户 "user@example.com"
When 我在发票管理页面点击 "Create Invoice"
And 我填写开票对象信息（名称、邮箱、税号）
And 我填写销售方信息（名称、邮箱、税号）
And 我添加行项目：
  | 名称                | 数量 | 单价    |
  | Pro Plan - Monthly | 1    | 9900    |
And 我设置到期日
And 我提交表单
Then 发票创建成功，状态为 "draft"
And 系统自动生成发票编号（如 INV-2026-0001）
And 系统自动计算小计和总计
And 我可以在发票列表中看到新创建的发票
```

**场景 2：折扣和税费计算**
```gherkin
Given 我正在创建发票
And 行项目小计为 9900 分
When 我设置折扣为固定金额 500 分
And 我设置税费为百分比 6%
Then 系统自动计算：
  | 折扣金额 | 500                          |
  | 税额     | 9900 × 6% = 594（税费以 subtotal 为基准，不基于折后金额） |
  | 总计     | 9900 - 500 + 594 = 9994      |
```

**场景 3：发票编号租户内唯一**
```gherkin
Given 我是 realm-1 的管理员
And realm-1 本年已有发票 INV-2026-0001
When 我创建新发票
Then 新发票编号为 INV-2026-0002
And 不同 Realm 的发票编号独立递增
```

**场景 4：关联订阅或支付记录（可选）**
```gherkin
Given 我正在创建发票
When 我选择关联某个订阅
Or 我选择关联某个支付记录
Then 发票记录该关联关系
And 该关联不影响订阅或支付的状态
```

---

### 故事 2：编辑发票草稿 [US-IV-002]

**优先级**: P0

**【用户故事】**
**作为**：Realm Admin
**我希望**：编辑草稿状态的发票（修改行项目、费用和双方信息）
**从而**：在开票前修正错误或调整内容

**【验收标准】**

**场景 1：编辑草稿发票**
```gherkin
Given 我是 realm-1 的管理员
And 存在状态为 "draft" 的发票 INV-2026-0001
When 我修改行项目、折扣、税费或双方信息
And 我保存更改
Then 发票更新成功
And 系统重新计算小计和总计
```

**场景 2：非草稿状态不可编辑**
```gherkin
Given 存在状态为 "issued" 的发票 INV-2026-0002
When 我尝试编辑该发票
Then 系统提示 "Only draft invoices can be edited"
And 我无法修改发票内容
```

---

### 故事 3：查看发票列表 [US-IV-003]

**优先级**: P0

**【用户故事】**
**作为**：Realm Admin
**我希望**：查看本 Realm 的所有发票列表，并按状态、日期等条件筛选
**从而**：掌握发票全貌和财务状况

**【验收标准】**

**场景 1：查看发票列表**
```gherkin
Given 我是 realm-1 的管理员
When 我访问发票管理页面
Then 我看到发票列表表格
And 表格包含：编号、开票对象、金额、状态、到期日、创建时间
And 列表支持分页
```

**场景 2：按状态筛选**
```gherkin
Given 我在发票管理页面
When 我选择状态筛选为 "overdue"
Then 列表只显示逾期发票
```

**场景 3：按日期范围筛选**
```gherkin
Given 我在发票管理页面
When 我设置创建日期范围为 "2026-04-01" 到 "2026-04-30"
Then 列表只显示该时间段内创建的发票
```

**场景 4：普通用户无法访问管理端发票列表**
```gherkin
Given 我是普通用户 user@example.com
When 我尝试访问 Realm 管理后台的发票列表页面
Then 系统拒绝访问
```

---

### 故事 4：查看发票详情 [US-IV-004]

**优先级**: P0

**【用户故事】**
**作为**：Realm Admin
**我希望**：查看发票的完整详情，包括所有行项目和状态变更历史
**从而**：了解发票的完整信息

**【验收标准】**

**场景 1：查看发票详情**
```gherkin
Given 我是 realm-1 的管理员
And 存在发票 INV-2026-0001
When 我点击该发票
Then 我看到发票详情页面
And 页面显示：
  | 发票编号     | INV-2026-0001          |
  | 状态         | issued                 |
  | 开票对象     | 名称、地址、邮箱、税号 |
  | 销售方       | 名称、地址、邮箱、税号 |
  | 行项目列表   | 名称、数量、单价、小计 |
  | 费用汇总     | 小计、折扣、税费、总计 |
  | 到期日       | 2026-06-08             |
```

**场景 2：查看状态变更历史**
```gherkin
Given 我在发票详情页面
Then 我看到状态变更时间线：
  | 时间              | 事件     | 操作者 |
  | 2026-05-08 10:00 | created | admin  |
  | 2026-05-08 14:00 | issued  | admin  |
```

**场景 3：查看不存在的发票**
```gherkin
Given 我是 realm-1 的管理员
When 我尝试查看一个不存在的发票
Then 系统提示 "Invoice not found"
```

**场景 4：无法查看其他 Realm 的发票**
```gherkin
Given 我是 realm-1 的管理员
When 我尝试访问属于 realm-2 的发票
Then 系统拒绝访问
```

---

### 故事 5：开具发票 [US-IV-005]

**优先级**: P0

**【用户故事】**
**作为**：Realm Admin
**我希望**：将草稿发票正式开具
**从而**：向客户发送正式账单

**【验收标准】**

**场景 1：开具草稿发票**
```gherkin
Given 我是 realm-1 的管理员
And 存在状态为 "draft" 的发票 INV-2026-0001
And 发票至少包含一个行项目
When 我点击 "Issue" 按钮
Then 发票状态变为 "issued"
And 系统记录开票日期和操作者
And 状态变更记录到历史
```

**场景 2：空发票不可开具**
```gherkin
Given 存在状态为 "draft" 的发票 INV-2026-0003
And 该发票没有行项目
When 我尝试开具
Then 系统提示 "Invoice must have at least one line item"
```

---

### 故事 6：作废发票 [US-IV-006]

**优先级**: P1

**【用户故事】**
**作为**：Realm Admin
**我希望**：作废草稿或已开具的发票
**从而**：处理错误或取消的账单

**【验收标准】**

**场景 1：作废草稿发票**
```gherkin
Given 我是 realm-1 的管理员
And 存在状态为 "draft" 的发票
When 我点击 "Void" 按钮
And 我确认作废
Then 发票状态变为 "void"
And 状态变更记录到历史
```

**场景 2：作废已开具发票**
```gherkin
Given 存在状态为 "issued" 的发票
When 我作废该发票
Then 发票状态变为 "void"
```

**场景 3：已付款发票不可作废**
```gherkin
Given 存在状态为 "paid" 的发票
When 我尝试作废
Then 系统提示 "Paid invoices cannot be voided"
```

---

### 故事 7：标记发票已付 [US-IV-007]

**优先级**: P0

**【用户故事】**
**作为**：Realm Admin
**我希望**：手动将发票标记为已付款
**从而**：记录线下或其他渠道的付款

**【验收标准】**

**场景 1：标记已付**
```gherkin
Given 我是 realm-1 的管理员
And 存在状态为 "issued" 的发票 INV-2026-0001
When 我点击 "Mark as Paid" 按钮
Then 发票状态变为 "paid"
And 系统记录付款日期和操作者
```

**场景 2：逾期发票标记已付**
```gherkin
Given 存在状态为 "overdue" 的发票
When 我标记为已付
Then 发票状态变为 "paid"
```

**场景 3：草稿发票不可标记已付**
```gherkin
Given 存在状态为 "draft" 的发票
When 我尝试标记已付
Then 系统提示 "Only issued or overdue invoices can be marked as paid"
```

---

### 故事 8：查看我的发票 [US-IV-008]

**优先级**: P1

**【用户故事】**
**作为**：Regular User（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：查看自己的发票列表和详情
**从而**：了解我的账单记录

**【验收标准】**

**场景 1：查看我的发票列表**
```gherkin
Given 我是普通用户 user@example.com
And Realm 已配置销售方信息
When 我访问我的发票页面
Then 我看到所有开给我的发票列表
And 列表包含：编号、金额、状态、到期日
```

**场景 1b：未配置销售方信息时隐藏个人发票入口**
```gherkin
Given 我是普通用户 user@example.com
And Realm 尚未配置销售方信息
When 我打开个人中心
Then 侧边栏不显示 "My Invoices" 菜单
And 我直接访问我的发票页面时被引导回个人资料页
```

**场景 2：查看发票详情**
```gherkin
Given 我在我的发票列表
When 我点击某个发票
Then 我看到发票详情，包含行项目和费用汇总
```

**场景 3：无法查看他人发票**
```gherkin
Given 我是 user-1@example.com
When 我尝试访问属于 user-2@example.com 的发票
Then 系统拒绝访问
```

---

### 故事 9：系统标记逾期发票 [US-IV-009]

**优先级**: P1

**【用户故事】**
**作为**：Herald 系统（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：自动将超过到期日未支付的发票标记为逾期
**从而**：提醒管理员跟进未付款账单

**【验收标准】**

**场景 1：自动标记逾期**
```gherkin
Given 存在状态为 "issued" 的发票
And 该发票的到期日已过
When 系统定时任务执行
Then 发票状态变为 "overdue"
And 状态变更记录到历史
```

**场景 2：已付款和已作废发票不受影响**
```gherkin
Given 存在状态为 "paid" 或 "void" 的发票
And 该发票的到期日已过
When 系统定时任务执行
Then 这些发票状态不变
```

---

### 故事 10：配置销售方信息 [US-IV-010]

**优先级**: P0

**【用户故事】**
**作为**：Realm Admin
**我希望**：在 Billing 设置中配置本 Realm 的销售方信息（公司名称、地址、税号等）
**从而**：用户申请发票时系统自动填充销售方信息，避免重复输入

**【验收标准】**

**场景 1：配置销售方信息**
```gherkin
Given 我是 realm-1 的管理员
When 我访问 Billing 设置页面
And 我填写销售方信息：
  | 公司名称 | Acme Corp        |
  | 地址     | 123 Main St      |
  | 邮箱     | billing@acme.com |
  | 电话     | +1-555-0100      |
  | 税号     | 91110000MA01     |
And 我保存配置
Then 销售方信息保存成功
And 后续用户申请发票时自动使用这些信息
```

**场景 2：修改销售方信息**
```gherkin
Given 我是 realm-1 的管理员
And 已配置销售方信息
When 我修改公司名称为 "Acme Corp Ltd"
And 我保存更改
Then 配置更新成功
And 已创建的发票不受影响（保留创建时的信息）
And 后续新申请的发票使用更新后的信息
```

**场景 3：未配置销售方信息时提示**
```gherkin
Given 我是 realm-1 的管理员
And 尚未配置销售方信息
When 用户尝试申请发票
Then 系统提示 "Seller info not configured, please contact admin"
And 申请无法提交
```

---

### 故事 11：申请发票 [US-IV-011]

**优先级**: P0

**【用户故事】**
**作为**：Regular User
**我希望**：为我的已付款订单或订阅申请发票
**从而**：获得正式的账单凭证

**【验收标准】**

**场景 1：从购买历史申请发票**
```gherkin
Given 我是普通用户 user@example.com
And 我的购买历史中有已完成的积分包购买记录
And Realm 已配置销售方信息
When 我进入 My Points 的 Purchase History
And 我在对应购买记录点击 "Invoice"
Then 系统打开申请发票表单
And 表单自动关联该购买记录
And 我填写开票抬头信息（名称、地址、邮箱、税号）
Then 系统创建草稿发票，状态为 "draft"
And 销售方信息自动从 Realm 配置填充
And 我看到提示 "Invoice application submitted, pending review"
```

**场景 1b：未配置销售方信息时不展示购买记录发票入口**
```gherkin
Given 我是普通用户 user@example.com
And 我的购买历史中有已完成的积分包购买记录
And Realm 尚未配置销售方信息
When 我进入 My Points 的 Purchase History
Then 对应购买记录不显示 "Invoice" 按钮
```

**场景 2：从订阅历史申请发票**
```gherkin
Given 我是普通用户 user@example.com
And 我有订阅记录
And Realm 已配置销售方信息
When 我进入 Subscription History
And 我在对应订阅点击 "Invoice"
Then 系统打开申请发票表单
And 表单自动关联该订阅
And 我填写开票抬头信息（名称、地址、邮箱、税号）
Then 系统创建草稿发票，状态为 "draft"
And 销售方信息自动从 Realm 配置填充
And 我看到提示 "Invoice application submitted, pending review"
```

**场景 2b：未配置销售方信息时不展示订阅发票入口**
```gherkin
Given 我是普通用户 user@example.com
And 我有订阅记录
And Realm 尚未配置销售方信息
When 我进入 Subscription History
Then 对应订阅不显示 "Invoice" 按钮
```

**场景 3：查看我的发票申请状态**
```gherkin
Given 我已提交发票申请
When 我查看我的发票列表
Then 我可以看到申请的发票及其状态
And 状态可能为：draft（待审核）、issued（已开具）、void（已作废）
```

**场景 4：无法为他人订单申请发票**
```gherkin
Given 我是 user-1@example.com
When 我尝试为 user-2@example.com 的订单申请发票
Then 系统拒绝操作
```

---

### 故事 12：审核并开具用户申请的发票 [US-IV-012]

**优先级**: P0

**【用户故事】**
**作为**：Realm Admin
**我希望**：审核用户申请的发票并开具
**从而**：确认发票内容正确后再正式开票

**【验收标准】**

**场景 1：审核用户申请的发票**
```gherkin
Given 我是 realm-1 的管理员
And 存在用户申请的草稿发票
When 我在发票列表中筛选 "pending review"
Then 我看到所有待审核的发票
And 每条显示申请人、金额、申请时间
```

**场景 2：审核通过并开具**
```gherkin
Given 我在审核一张用户申请的发票
When 我确认发票内容无误
And 我点击 "Issue" 按钮
Then 发票状态变为 "issued"
And 用户可以在自己的发票列表中看到状态已更新
```

**场景 3：审核不通过并作废**
```gherkin
Given 我在审核一张用户申请的发票
When 我发现问题需要作废
And 我点击 "Void" 按钮
And 我填写作废原因
Then 发票状态变为 "void"
And 用户可以看到发票已作废及原因
```

**场景 4：修改后开具**
```gherkin
Given 我在审核一张用户申请的发票
When 我发现部分信息需要修改
And 我编辑发票内容（因为是 draft 状态）
And 我保存并开具
Then 发票更新并开具成功
```

---

## 用户故事优先级汇总

| 优先级 | 用户故事数量 | 关键故事                                                                                   |
| ------ | ------------ | ------------------------------------------------------------------------------------------ |
| P0     | 9            | 创建发票、编辑草稿、查看列表、查看详情、开具发票、标记已付、配置销售方、申请发票、审核开具 |
| P1     | 3            | 作废发票、查看我的发票、系统标记逾期                                                       |
| P2     | 0            | -                                                                                          |

---

## 相关文档

- **PRD**: `docs/prd/billing/invoice.md` - Invoice 发票产品需求文档
- **PRD**: `docs/prd/billing/subscription.md` - Billing 订阅计费产品需求文档
