# Invoice 发票产品需求文档 (PRD)

**创建时间**: 2026-05-08
**优先级**: P0

---

## 1. 相关用户故事

> 详细故事与验收标准请查看 `docs/user-stories/billing/invoice.md`。

### 1.1 故事引用

- `[US-IV-001]` 创建发票，优先级 P0，来源 `docs/user-stories/billing/invoice.md`
  - 角色：Realm Admin
  - 摘要：创建发票草稿，添加行项目，设置费用和双方信息

- `[US-IV-002]` 编辑发票草稿，优先级 P0，来源 `docs/user-stories/billing/invoice.md`
  - 角色：Realm Admin
  - 摘要：修改草稿发票的行项目、费用和双方信息

- `[US-IV-003]` 查看发票列表，优先级 P0，来源 `docs/user-stories/billing/invoice.md`
  - 角色：Realm Admin
  - 摘要：查看本 Realm 所有发票，支持筛选和分页

- `[US-IV-004]` 查看发票详情，优先级 P0，来源 `docs/user-stories/billing/invoice.md`
  - 角色：Realm Admin
  - 摘要：查看发票完整详情，包括行项目和状态历史

- `[US-IV-005]` 开具发票，优先级 P0，来源 `docs/user-stories/billing/invoice.md`
  - 角色：Realm Admin
  - 摘要：将草稿发票正式开具（draft → issued）

- `[US-IV-006]` 作废发票，优先级 P1，来源 `docs/user-stories/billing/invoice.md`
  - 角色：Realm Admin
  - 摘要：作废草稿或已开具的发票

- `[US-IV-007]` 标记发票已付，优先级 P0，来源 `docs/user-stories/billing/invoice.md`
  - 角色：Realm Admin
  - 摘要：手动将发票标记为已付款

- `[US-IV-008]` 查看我的发票，优先级 P1，来源 `docs/user-stories/billing/invoice.md`
  - 角色：Regular User
  - 摘要：查看自己的发票列表和详情

- `[US-IV-009]` 系统标记逾期发票，优先级 P1，来源 `docs/user-stories/billing/invoice.md`
  - 角色：Herald 系统
  - 摘要：自动将超过到期日未支付的发票标记为逾期

- `[US-IV-010]` 配置销售方信息，优先级 P0，来源 `docs/user-stories/billing/invoice.md`
  - 角色：Realm Admin
  - 摘要：在 Billing 设置中配置本 Realm 的销售方信息，用户申请发票时自动填充

- `[US-IV-011]` 申请发票，优先级 P0，来源 `docs/user-stories/billing/invoice.md`
  - 角色：Regular User
  - 摘要：为已付款订单或订阅申请发票

- `[US-IV-012]` 审核并开具用户申请的发票，优先级 P0，来源 `docs/user-stories/billing/invoice.md`
  - 角色：Realm Admin
  - 摘要：审核用户申请的发票，确认后开具或作废

**外部 Provider 发票（Invoice Fallback）**: `docs/user-stories/billing/invoice-fallback.md`

- `[US-IF-001]` 配置发票策略，优先级 P0
  - 角色：Realm Admin
  - 摘要：配置 Realm 发票策略（provider_first / manual_only / none）和各支付平台的外部发票能力开关

- `[US-IF-002]` 系统同步 Stripe 发票，优先级 P0
  - 角色：Herald 系统
  - 摘要：通过 Stripe webhook 自动同步 Stripe 发票数据到 Herald

- `[US-IF-003]` 系统同步 Creem 交易税务数据，优先级 P0
  - 角色：Herald 系统
  - 摘要：同步 Creem MoR 交易的税务数据到 Herald

- `[US-IF-004]` 查看外部 Provider 发票（管理员），优先级 P0
  - 角色：Realm Admin
  - 摘要：在发票列表中查看外部 provider 同步的发票（只读）

- `[US-IF-005]` 查看外部 Provider 发票（普通用户），优先级 P1
  - 角色：Regular User
  - 摘要：在"我的发票"中查看外部 provider 同步的发票（只读）

- `[US-IF-006]` 下载外部发票 PDF 或查看 Provider 页面，优先级 P1
  - 角色：Realm Admin / Regular User
  - 摘要：通过外部 URL 下载或查看 provider 管理的发票 PDF

- `[US-IF-007]` 系统同步 Stripe Credit Note，优先级 P0
  - 角色：Herald 系统
  - 摘要：通过 Stripe webhook 自动同步 Credit Note 数据到关联发票

- `[US-IF-008]` 管理员查看发票退款信息与 Credit Note 列表，优先级 P0
  - 角色：Realm Admin
  - 摘要：在发票详情中查看累计退款金额、剩余应付与只读 Credit Note 列表

- `[US-IF-009]` 普通用户查看退款标注，优先级 P1
  - 角色：Regular User
  - 摘要：在"我的发票"中看到自己已退款发票的退款标注与剩余应付

- `[US-IF-010]` 管理员记录自研发票的线下退款，优先级 P0
  - 角色：Realm Admin
  - 摘要：为已付款的 Herald 自研发票创建 Manual Credit Note，记录线下退款，保留税务合规凭证

- `[US-IF-011]` 系统处理 Stripe Credit Note 作废，优先级 P0
  - 角色：Herald 系统
  - 摘要：通过 Stripe `credit_note.voided` webhook 同步作废状态，恢复关联发票的剩余应付

**支付与发票归属**: `docs/user-stories/billing/payment-invoice-mapping.md`

- `[US-PM-001～004]` 续费支付记录、Creem 续费发票、外部发票归属与补偿可观测性

### 1.2 优先级汇总表

| 优先级 | 数量 | 关键故事 |
|--------|------|----------|
| P0 | 17 | 创建发票、编辑草稿、查看列表、查看详情、开具发票、标记已付、配置销售方、申请发票、审核开具、配置发票策略、同步 Stripe 发票、同步 Creem 税务、管理员查看外部发票、同步 Stripe Credit Note、管理员查看发票退款信息、管理员记录自研发票线下退款、处理 Stripe Credit Note 作废 |
| P1 | 6 | 作废发票、查看我的发票、系统标记逾期、用户查看外部发票、下载外部 PDF、用户查看退款标注 |
| P2 | 0 | - |

---

## 2. 范围界定

### 2.1 包含功能

- 销售方信息配置（Realm Admin 在 Billing 设置中一次性配置，后续发票自动填充；包含默认付款条款 default_payment_terms 字段）
- 用户申请发票（Regular User 可从购买历史或订阅历史上下文入口申请，系统预填并隐藏内部引用 ID；独立申请页保留手动填写引用的兼容路径）
- 管理员审核开具（Realm Admin 审核用户申请，确认后开具或作废）
- 管理员手动创建发票（辅助路径）
- 发票 CRUD（查看、编辑、作废；列表支持按 invoice_number 和 billing_name 模糊搜索）
- 行项目管理（添加、编辑、删除、排序）
- 费用计算（折扣、税费支持固定金额和百分比模式；运费仅支持固定金额模式）
- 发票状态机（draft → issued → paid / void / overdue）
- 发票编号自动生成（租户内按年递增，格式 INV-{YEAR}-{SEQ}）
- 买方信息管理（用户申请时填写开票抬头，含税号）
- 销售方税号（必填，用于发票合规）
- Regular User 查看自己的发票及申请状态
- 系统自动标记逾期发票（定时任务）
- 发票审计追踪（状态变更历史）
- 发票可关联 Subscription 和 Payment Attempt（上下文入口自动传递关联 ID；独立表单仍可手动填写）
- PDF 发票生成和下载
- 发票策略配置：Realm Admin 配置 `invoice_policy`（provider_first / manual_only / none）和每个支付平台的外部发票能力开关
- Stripe 订阅发票同步：通过 webhook 自动同步 Stripe Invoicing 产生的发票数据到 Herald（只读镜像）
- Stripe 一次性购买发票同步：checkout.session.completed 事件中为 mode=payment 的一次性购买创建外部发票记录（与 Creem inline 同步模式一致）
- Creem 交易税务数据同步：Creem MoR 交易支付成功后同步税务数据到 Herald
- 只读展示外部 Provider 发票：provider-owned 发票在 Herald 中只读展示，禁止创建、编辑、开具、作废、标记已付
- 自研发票 Fallback：provider 不支持或未启用外部发票时，走 Herald 自研发票系统
- Provider 来源标识：发票列表和详情页显示发票来源 provider（Manual / Stripe / Creem / Wechat）
- 外部 PDF / 托管页面跳转：有外部 PDF URL 时直接重定向下载；有托管页面 URL 时显示 "View in Provider" 链接
- Creem MoR 保护：Creem MoR 交易不允许创建 Herald manual 发票
- 发票列表 provider 筛选：支持按 provider 类型筛选发票

### 2.2 不包含功能

- 多格式导出（CSV / XLSX / XML，后续迭代）
- 邮件发送发票（后续迭代）
- Subscription 续费自动创建 Invoice（后续迭代）
- Payment Attempt 支付成功自动创建 Invoice（可关联但不自动生成）
- 在线支付集成（发票仅手动标记已付）
- 发票模板自定义
- 多币种自动转换
- Herald 主动调用 Stripe Invoice API 创建发票（仅 webhook 被动同步）
- Herald 主动调用 Creem API 查询交易税务（仅通过支付回调同步）

### 2.3 依赖项

- **Realm 系统** — 发票属于 Realm 级别
- **Account 系统** — 开票对象关联 Account
- **现有角色与权限系统** — 管理端使用 `billing.view` / `billing.manage` 权限控制，用户端复用登录用户身份判断
- **Subscription 系统**（部分实现）— Invoice 可选关联 Subscription
- **Payment Attempt 系统** — Invoice 可选关联 Payment Attempt

---

## 3. 需求概述

### 3.1 功能描述

为 Herald 多租户系统增补发票功能。系统采用"外部平台发票优先 + 自研发票 Fallback"的双模式架构。

**自研发票主流程**：Realm Admin 配置销售方信息 → Regular User 申请发票 → Realm Admin 审核开具。同时保留 Admin 手动创建发票的辅助路径。

**外部 Provider 发票**：当支付平台（如 Stripe、Creem）提供发票/税务能力时，Herald 通过 webhook 被动同步外部发票数据并只读展示。对于 Stripe，订阅发票通过 `invoice.*` 事件同步，一次性购买发票通过 `checkout.session.completed`（mode=payment）事件 inline 创建。发票来源由实际收款 payment_provider 的发票能力决定，而非按产品或 Realm 全局决定。

发票与现有 billing 模块集成，可关联 Subscription 和 Payment Attempt，但不自动生成。

- **以用户申请为主**：Regular User 主动申请发票，Admin 审核
- **销售方信息预配置**：Realm Admin 一次性配置，后续自动填充
- 发票状态机管理（draft / issued / paid / void / overdue）
- 行项目驱动的金额计算，以最小货币单位（分）存储
- 折扣 / 税费支持固定金额和百分比两种模式；运费仅支持固定金额模式
- 发票编号在租户内按年自动递增
- 严格的租户数据隔离
- **发票跟随实际收款 provider**：同一产品支持多支付平台时，发票归属由实际 payment_provider 的发票能力决定
- **三种发票策略**：provider_first（优先外部 provider）、manual_only（仅自研）、none（不提供自研发票入口）
- **只读展示 provider-owned 发票**：数据由 webhook/API 同步，不可通过 Herald API 修改
- **Creem MoR 不可覆盖**：Creem 作为 Merchant of Record 的交易，Herald 不得创建 manual 发票

---

## 4. 业务规则与状态

### 4.1 业务规则

- **销售方信息前置条件**：Realm Admin 必须先配置销售方信息（公司名称、地址、邮箱、电话、税号），否则用户无法提交发票申请
- **用户申请验证**：用户申请发票需验证拥有对应的支付记录；申请时填写开票抬头信息（含税号），系统创建草稿发票（来源标记为 user_application），销售方信息自动从 Realm 配置填充
- **用户申请发票时必须填写开票抬头税号**：用户申请发票时，`billing_tax_id` 为必填字段，不可为空字符串
- **发票编辑时双方税号为必填字段**：编辑发票时，`billing_tax_id` 和 `seller_tax_id` 均为必填字段，不可为空字符串
- **列表搜索**：发票列表支持通过 `search` 查询参数对 `invoice_number` 和 `billing_name` 进行模糊搜索（ILIKE），不区分大小写
- **销售方默认付款条款**：销售方配置（`SellerConfigRequest`）包含 `default_payment_terms` 可选字段，用户申请发票时自动填充为发票的 `payment_terms`；管理员手动创建时也可单独指定
- **发票编号唯一性**：发票编号（invoice_number）在 realm + 年范围内唯一，格式 INV-{YEAR}-{SEQ}
- **编辑约束**：仅 draft 状态可编辑行项目、费用和双方信息；编辑后自动重算金额
- **开具约束**：空发票不可开具；开具时记录开票日期；支持通过 `issue_date` 可选参数覆盖开票日期（默认为当天）；若存在 `due_date`，则 `due_date` 必须大于等于 `issue_date`；开具时 `billing_email` 和 `billing_phone` 至少需填写一个非空值，用于联系开票对象
- **标记已付约束**：仅 issued / overdue 状态可标记已付；支持通过 `paid_at` 可选时间戳参数覆盖实际付款时间（默认为当前时间）
- **作废约束**：已付款发票不可作废；可作废 draft 和 issued 状态。边缘语义：已付款（paid）发票在全部 Credit Note 均已作废（即退款金额归零、剩余应付恢复全额）时允许作废——等价于"该发票从未有效退款"，回到 paid 作废例外；存在任一有效（非作废）Credit Note 时仍不可作废
- **来源标记**：发票来源（admin_manual / user_application）需持久化，用于筛选和审计
- **关联可选**：发票可关联 subscription_id 和 payment_attempt_id，关联为可选，不触发自动行为
- **金额计算规则**：line_item.subtotal = quantity x unit_price；invoice.subtotal = SUM(line_items.subtotal)；invoice.total = subtotal - discount_amount + tax_amount + shipping_amount；所有金额以最小货币单位（分）存储。折扣、税费、运费均以 subtotal 为基准计算，税费未考虑折扣影响（即税费不基于折后金额）
- **运费模式限制**：运费（shipping_mode）仅支持固定金额（fixed）模式，不支持百分比模式（数据库 CHECK 约束限制）

**Provider 发票规则**：

- **发票来源路由**：根据 payment_attempt / subscription 上的实际 payment_provider 和该 provider 的外部发票能力配置决定发票来源；不按产品或 Realm 全局决定
- **invoice_policy 行为矩阵**：

  | 操作 | provider_first | manual_only | none |
  |------|---------------|-------------|------|
  | 发票列表 | 展示自研 + 外部 provider 同步数据 | 展示自研数据 | 仅展示外部 provider 同步数据 |
  | 发票详情 | 外部发票只读，自研发票按现有逻辑 | 全部按现有逻辑 | 外部发票只读 |
  | 创建发票 | 仅 manual fallback 场景 | 允许 | 不允许 |
  | 编辑/开具/作废/标记已付 | 仅 manual 发票 | 全部允许 | 不允许 |
  | 用户申请发票 | 仅 manual fallback 场景 | 允许 | 不允许 |
  | PDF 下载 | 外部发票用外部 URL，自研发票用 IronPress | IronPress 生成 | 外部 URL |
  | "View in Provider" 链接 | 有 external_hosted_url 时显示 | 不显示 | 有时显示 |

- **Creem MoR 约束**：Creem 交易的发票必须由 Creem 管理；无论 invoice_policy 设置如何，Herald 不得为 Creem 交易创建 manual 发票
- **Stripe 发票同步触发**：通过 Stripe webhook 被动同步（invoice.created / invoice.finalized / invoice.voided / invoice.paid），Herald 不主动调用 Stripe Invoice API 创建发票
- **Stripe 一次性购买发票同步触发**：通过 Stripe `checkout.session.completed`（mode=payment）事件 inline 创建外部发票记录；使用 checkout session 上的 Stripe invoice ID（`in_...`，由 Checkout 在启用 invoice_creation 时自动创建）作为 external_invoice_id，payment_intent（`pi_...`）作为 external_order_id；status 直接为 paid
- **Stripe 一次性购买发票数据来源**：从 checkout session 对象提取 amount_total、currency、customer_email、payment_intent 等字段；account_id 从 metadata.userId 解析
- **Stripe 发票状态映射**：Stripe `draft` → Herald `draft`，Stripe `open` → Herald `issued`，Stripe `paid` → Herald `paid`，Stripe `void` → Herald `void`
- **Creem 税务数据同步**：Creem 交易支付成功后同步交易金额、税额、税区等税务信息作为发票记录
- **Provider 切换兼容**：Realm 从 manual_only 切到 provider_first 时，已有 manual 发票保持 provider='manual' 不变，策略切换只影响新发票的路由决策
- **发票编号规则**：外部 provider 发票使用 provider 分配的编号（如 Stripe 的发票编号），自研发票继续使用 INV-{YEAR}-{SEQ} 格式
- **Webhook 幂等性**：复用现有 payment_event 表的 external_event_id 唯一约束，重复 webhook 更新而非创建
- **外部发票不可操作**：provider != 'manual' 的发票禁止通过 Herald API 执行创建、编辑、开具、作废、标记已付操作
- **权限复用**：管理端继续使用 `billing.view` / `billing.manage` 权限控制，不新增发票细粒度权限

### 4.2 关键状态与异常

**支付与发票归属**：

- 每次成功的 Stripe/Creem 托管支付产生一条支付尝试记录并映射且仅映射一张发票；扣款失败和零元周期不产生成功记录或发票。
- 一次性外部发票归属支付尝试，订阅发票归属订阅，续费发票同时归属本次支付尝试和订阅。
- Creem 每个成功续费周期同步一张只读发票；Herald 不为 Creem 主动开具或退款发票。
- 暂时无法归属时标记为待归属并由补偿流程回填；重复 webhook 或补偿不得创建重复支付记录、发票或归属。
- 管理员可发现并筛选“成功支付无发票”和“外部发票无归属”；普通用户不得看到内部支付尝试标识。
- 强制归属只适用于新同步的第三方托管支付；历史空归属不回填，Manual 发票允许不关联支付。

**Credit Note 与退款凭证**：

- Credit Note 叠加退款维度，不改变发票 `paid` 主状态；Stripe Credit Note 作废时回滚累计退款金额和剩余应付。
- Stripe Credit Note ID 唯一，重复 created/voided 事件保持幂等；关联发票缺失时等待补偿，不创建孤儿记录。
- 仅 `provider=manual` 且 `status=paid` 的发票允许管理员记录 Manual Credit Note；金额为正且累计不得超过发票总额，创建后不可编辑、删除或撤销。
- Stripe/Creem 发票拒绝 Manual Credit Note；provider 不匹配或退款金额越界必须明确拒绝或记录诊断。
- 积分回收只由支付退款事件处理；Credit Note 同步和 Manual Credit Note 创建不重复触发积分回收。

- **发票状态机**：draft → issued → paid / void / overdue
- **逾期标记**：系统定时检查到期日已过的 issued 发票，自动标记为 overdue
- **审计追踪**：所有状态变更操作需记录审计事件（actor、timestamp、changes）
- **权限边界**：管理端接口通过 `billing.view` / `billing.manage` 权限检查控制访问（非直接检查 Realm Admin 角色），用户端复用登录用户身份判断；Regular User 只能查询和申请自己的发票
- **外部发票状态**：由 provider 驱动更新，Herald 只做状态映射和只读展示；自研发票状态机保持不变
- **逾期标记范围**：系统自动逾期任务仅处理 `provider='manual'` 的自研发票；外部 provider 发票的逾期或关闭状态由 provider webhook/API 同步驱动
- **Provider 未启用外部发票能力**：当 provider_first 策略下某 provider 未启用外部发票时，该 provider 的交易降级到 manual fallback
- **Creem 无 PDF URL**：Creem API 当前不返回发票 PDF URL，用户需通过 Creem 平台查看完整发票

---

## 5. 功能需求

### 5.1 核心需求

**支付归属与退款凭证**：

- Stripe/Creem 订阅续费成功时创建支付尝试记录；Creem 同步每期续费发票。
- 外部发票写入时建立支付尝试/订阅归属；失败时进入可补偿状态。
- 管理员可筛选“成功支付无发票”和“外部发票无归属”，普通用户不暴露内部支付尝试标识。
- Stripe `credit_note.created` / `credit_note.voided` 同步为只读凭证并更新退款汇总，重复事件保持幂等。
- 管理员可为已付款的 Manual 发票记录线下退款凭证；Stripe/Creem 发票拒绝 Manual Credit Note。

- **销售方信息配置**：Realm Admin 在 Billing 设置中配置本 Realm 的销售方信息，用户申请发票时自动填充
- **用户申请发票（主流程）**：Regular User 为已付款的订单或订阅申请发票，填写开票抬头信息，系统创建草稿发票
- **管理员审核开具**：Realm Admin 在发票列表中筛选待审核发票，审核通过后开具，审核不通过可作废并注明原因；审核时允许编辑草稿内容
- **管理员手动创建（辅助路径）**：Realm Admin 可直接创建草稿发票，手动填写双方信息和行项目
- **发票编辑**：仅草稿状态可编辑，编辑后自动重算金额
- **发票开具**：将草稿发票正式开具，记录开票日期；支持通过 `issue_date` 可选参数覆盖开票日期
- **发票作废**：将草稿或已开具的发票作废；已付款发票不可作废
- **标记已付**：手动将已开具或逾期发票标记为已付款；支持通过 `paid_at` 可选参数指定实际付款时间
- **逾期标记**：系统定时检查到期日已过的 issued 发票，自动标记为 overdue
- **PDF 生成和下载**：支持发票 PDF 生成和下载
- **发票策略配置**：Realm Admin 在 Billing 设置中配置 invoice_policy 和每个支付平台的外部发票能力开关
- **Stripe 发票 webhook 同步**：Herald 自动接收 Stripe 的 invoice.* 事件，同步发票数据到本地，状态按映射规则转换
- **Stripe 一次性购买发票同步**：Herald 在处理 checkout.session.completed（mode=payment）事件时，自动创建 provider=stripe 的外部发票记录，状态为 paid
- **Creem 交易税务同步**：Creem 支付成功后，系统创建 provider='creem' 的发票记录，同步交易税务数据
- **外部发票只读展示**：发票列表和详情页显示 provider 来源标识；provider != manual 的发票隐藏所有编辑操作按钮，显示 "View in Provider" 链接
- **自研发票 Fallback**：invoice_policy=provider_first 时，不支持外部发票的 provider 交易仍可使用 Herald 自研发票
- **外部 PDF / 页面跳转**：有 external_pdf_url 时重定向下载；有 external_hosted_url 时显示跳转链接
- **发票列表 provider 筛选**：支持按 provider 类型（Manual / Stripe / Creem / Wechat）筛选发票列表

### 5.2 验收目标

- Realm Admin 能配置销售方信息，后续发票自动填充
- Regular User 能为已付款订单申请发票，看到申请状态变化
- Realm Admin 能审核用户申请的发票，开具或作废
- Realm Admin 也能手动创建发票（辅助路径）
- 金额计算准确无误，包括百分比税费和折扣
- 发票编号在租户内唯一且按年递增
- 状态变更全部记录到审计历史
- Regular User 只能查看和申请自己的发票，无法访问他人发票
- Realm Admin 能配置 invoice_policy，启用/禁用各 provider 的外部发票能力
- Stripe Invoicing 产生的发票能通过 webhook 自动同步到 Herald 并正确映射状态
- Stripe 一次性购买（Checkout mode=payment）支付成功后，Herald 自动创建外部发票记录
- 一次性购买的外部发票为只读，无法通过 Herald API 修改
- Creem MoR 交易的税务数据能同步到 Herald 并只读展示
- 外部 provider 发票在管理端和用户端均为只读，无法通过 Herald API 修改
- 自研发票功能在 manual_only 和 fallback 场景下完全保持不变
- Creem MoR 交易无法创建 Herald manual 发票
- 发票列表能按 provider 筛选，能区分显示不同来源的发票
- 每次非零第三方支付均可定位到唯一发票，外部发票可直接定位到支付尝试或订阅
- 归属依赖暂时未就绪时，补偿后能够恢复且不产生重复记录
- Stripe Credit Note 创建、部分退款和作废后，累计退款与剩余应付正确且发票主状态不变
- 管理员可记录 Manual 发票线下退款；金额越界和 provider 不匹配被明确拒绝
- PDF 下载正确区分自研（IronPress）和外部（URL 重定向）

---

## 6. API 相关约束

**适用性**: 适用

- **接口能力范围**：发票 CRUD、销售方信息配置、发票开具/作废/标记已付、用户申请发票、PDF 生成下载、发票策略配置、provider 筛选查询的能力边界；在 api-billing crate 中新增
- **访问控制原则**：管理端接口通过 `billing.view` / `billing.manage` 权限检查控制（`require_billing_permission` 辅助函数实现）；用户端接口复用登录用户身份判断；Realm Admin 可管理本 Realm 所有发票；Regular User 只能查询和申请自己的发票；销售方信息配置 API 归属 Realm Billing 设置，需 `billing.manage` 权限
- **租户/Realm 数据边界**：发票按 Realm 隔离；发票编号在 realm + 年范围内唯一；发票策略配置按 Realm 独立；provider 能力开关按 Realm + Provider 独立
- **状态操作约束**：仅 draft 可编辑；issued / overdue 可标记已付或作废；paid 不可修改
- **外部发票写操作禁止**：现有发票 CRUD API 对 provider != manual 的发票禁止写操作（创建、编辑、开具、作废、标记已付）
- **兼容性要求**：现有 invoice API 响应向后兼容（新增字段可选，默认 provider='manual'）；自研发票的全部 API 行为不变

---

## 7. 前端/交互约束

**适用性**: 适用

- **管理后台**（Realm Admin）：
  - 入口：Realm 管理后台的 Billing 区域新增 "Invoices" 菜单；只按管理权限控制，不因当前 Realm 尚未配置销售方信息或尚无发票记录而隐藏
  - 销售方配置：Billing 设置页面新增销售方信息配置区域（公司名称、地址、邮箱、电话、税号）
  - 发票列表页：表格展示编号、开票对象、金额、状态、来源、provider、到期日，支持状态、来源、provider 和日期筛选；外部 provider 发票行不显示编辑/开具/作废/标记已付操作
  - 待审核视图：筛选来源为 "user_application" 且状态为 "draft" 的发票，快速审核
  - 发票详情页：provider != manual 时切换为只读模式，隐藏所有操作按钮，显示 provider 标识和 "View in Provider" 链接（有 external_hosted_url 时）
  - 发票创建/编辑表单：行项目动态添加删除，实时计算金额汇总
  - 发票详情页：展示完整发票信息、行项目和状态历史时间线
  - 状态操作：Issue、Void、Mark as Paid 按钮根据当前状态启用/禁用
  - 发票策略配置入口：Billing 设置页面新增发票策略区域，包含 invoice_policy 选择和每个已启用支付平台的外部发票能力开关
  - PDF 下载：外部发票使用外部 URL 重定向；自研发票使用现有 IronPress 生成

- **个人页面**（Regular User）：
  - 入口：当 Realm 已配置销售方信息时，用户个人中心显示 "My Invoices" 菜单；未配置时隐藏
  - 申请发票入口：当 Realm 已配置销售方信息时，在支付记录或订阅详情旁提供 "Apply for Invoice" 按钮；未配置时不展示
  - 申请表单：选择支付记录、填写开票抬头（名称、地址、邮箱、税号）
  - 列表页：展示属于自己的发票，包含编号、金额、状态、到期日、申请状态、provider 来源标识
  - 详情页：查看发票完整信息；外部发票只读，显示 "View in Provider" 链接
  - 申请发票：Creem 交易的申请入口不可用；Apple（App Store）与 Google（Google Play）同为 Merchant of Record，其交易不进入 Herald 发票体系，不受 invoice_policy 影响，申请入口不可用；其他 provider 交易根据 invoice_policy 决定是否可申请

- **状态反馈**：操作成功后显示对应成功消息；状态不合法时禁用按钮并提示原因；金额变动后实时更新汇总区域；操作外部 provider 发票时提示 "This invoice is managed by {Provider}"

---

## 8. 已确认决策

### 8.1 已确认决策

- 第三方托管支付采用“每次成功支付一条支付尝试、一张映射发票”，零元周期除外
- 新同步的外部发票必须建立显式归属；既有历史空归属不回填
- Credit Note 是退款凭证维度，不新增或改变发票主状态，也不重复触发积分回收

- 主流程为用户申请 + 管理员审核开具，保留管理员手动创建辅助路径
- 不新增 Invoice 细粒度权限，管理端使用 `billing.view` / `billing.manage` 权限控制，用户端复用登录用户身份判断
- 发票可关联 Subscription 和 Payment Attempt 但不自动生成
- 发票编号格式为 INV-{YEAR}-{SEQ}，租户内按年递增
- 发票来源跟随实际收款 payment_provider，而非跟随产品或 Realm 全局选择
- 三种发票策略：provider_first / manual_only / none
- Stripe 发票同步通过 webhook 被动驱动，Herald 不主动调用 Stripe Invoice API 创建发票
- Stripe 一次性购买发票通过 checkout.session.completed（mode=payment）事件 inline 同步，与 Creem 模式一致
- Creem MoR 交易的发票不可被 Herald manual 覆盖，无论 invoice_policy 设置
- 已有 manual 发票在策略切换后保持 provider='manual' 不变
- 外部发票 PDF 有 URL 时直接重定向，无 URL 时提示由 provider 管理
- 外部发票编号使用 provider 分配的编号，自研发票继续 INV-{YEAR}-{SEQ}

---

## 9. 参考资料

- 用户故事：`docs/user-stories/billing/invoice.md`
- 用户故事：`docs/user-stories/billing/invoice-fallback.md`、`docs/user-stories/billing/payment-invoice-mapping.md`
- 技术预研：`.ai/tech-research/invoice_fallback.md`
- 相关 PRD：`docs/prd/billing/subscription.md`
- 相关 PRD：`docs/prd/billing/stripe-payment.md`
