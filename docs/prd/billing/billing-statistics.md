# 计费统计（支付与积分消耗）产品需求文档 (PRD)

**创建时间**: 2026-09-12
**优先级**: P1

> 场景背景：管理端对支付与积分此前只有逐笔流水（购买历史、订阅、发票、积分流水），没有任何汇总视角。本 PRD 是 `docs/prd/billing/subscription.md` §2.2 中「计费统计和报表：不在首版范围」这一延后项的正式立项（DEC-billing-statistics-001），交付管理后台 billing 区统一「统计」页：支付统计与积分消耗统计两个面板。本文档不承载接口端点、请求/响应 schema、HTTP 状态码、数据库建表/迁移或代码类型定义；技术方案细节请参见对应技术设计。

---

## 1. 相关用户故事

> 详细故事与验收标准请查看 `docs/user-stories/` 中对应文档。

### 1.1 相关故事

本 feature 无专属用户故事文件；立项依据为用户确认的两类统计缺失（conversation 2026-09-08，见决策账本），验收来源为场景测试（`billing_statistics_scenarios`）。与既有故事域的关系：

**支付统计承接（部分）**，来源 `docs/user-stories/billing/payment-provider.md`：
- `[US-PV-005]` 查看支付平台使用统计（P2）——本 PRD 交付其「按渠道分组的成功/失败笔数、成功率、按币种金额」视角；故事中的 Active Subs、Avg Payment Time 指标不随首版交付（Q-billing-statistics-003，排期另定）

**积分消耗统计关联（明确不承接主体）**，来源 `docs/user-stories/billing/points-admin.md`：
- `[US-PO-007]` 查看免费用户积分统计（P1）——该故事主体为免费用户发放/转化漏斗，不随本 PRD 交付（Q-billing-statistics-003）；本 PRD 的消耗口径（按积分账户分组、按日趋势）与之互补但不重叠

**统计口径依赖（复用既有事实来源）**：
- 支付统计以统一支付尝试记录为事实来源（`docs/user-stories/billing/payment-attempt.md`）
- 积分消耗统计以积分扣减流水为事实来源（`docs/prd/billing/points.md` 交易模型）

### 1.2 优先级汇总

| 优先级 | 数量（本 PRD 交付的故事切片） | 关键切片 |
|--------|------|----------|
| P1 | 1 | 管理端支付 + 积分消耗双面板统计页 |

> US-PV-005 剩余指标与 US-PO-007 主体均为延后项，不计入本表（见 §2.2）。

---

## 2. 范围界定

### 2.1 包含功能

- **支付统计面板**：窗口内成功/失败笔数、按支付渠道分组的成功/失败笔数、按币种分组的成功金额、按日支付趋势（DEC-billing-statistics-002）
- **积分消耗统计面板**：窗口内总消耗积分、有消耗用户数、按积分账户（bucket）分组的消耗量、按日消耗趋势（DEC-billing-statistics-003）
- **固定时间窗口**：「最近 7 天 / 最近 30 天」两档切换，两面板共用同一窗口选择（DEC-billing-statistics-005）
- **统一统计入口**：管理后台 billing 区新增「统计」页面，支付与积分消耗两个面板并列（DEC-billing-statistics-006）
- **既有权限复用**：支付统计面板挂 `billing.view`，积分消耗面板挂 `points.view`，互不隐含（DEC-billing-statistics-004）

### 2.2 不包含功能 (Out of Scope)

- **终端用户自助用量可视化**：仅管理端；用户侧用量页为独立后置候选（Q-billing-statistics-002）
- **免费用户发放/转化统计**（US-PO-007 主体）与 **US-PV-005 剩余指标**（Active Subs、Avg Payment Time）：不随首版交付，承接排期待定（Q-billing-statistics-003）
- **CSV / 数据导出**：不含任何导出能力，未来按独立迭代立项（Q-billing-statistics-001）
- **自定义时间范围**：仅 7/30 两档，不做任意起止日期（DEC-billing-statistics-005）
- **净收入 / 退款冲减口径**：支付统计只做已完结尝试的毛口径汇总，不做收入调整（DEC-billing-statistics-002）
- **净消耗口径**：积分消耗不冲减退款回收、配额权益撤销或补偿回退（DEC-billing-statistics-003）
- **实时推送 / 告警 / 报表订阅**：统计为页面加载时拉取的只读聚合，不承诺实时性
- **跨 Realm 汇总**：严格单 Realm 视角，不提供平台级汇总
- **Dashboard 首屏改动**：不在既有 Dashboard（用户活跃指标）混入计费指标（DEC-billing-statistics-006）

### 2.3 依赖项

- **统一支付尝试记录**：支付统计的唯一事实来源（`docs/prd/billing/subscription.md`、`docs/user-stories/billing/payment-attempt.md`）
- **积分交易流水**：积分消耗统计的事实来源（`docs/prd/billing/points.md`）
- **积分账户（Credit Bucket）**：消耗统计的分组维度（`docs/prd/billing/credit-bucket.md`）
- **权限系统**：复用 `billing.view` / `points.view` 既有权限项（`docs/user-stories/_roles.md`）
- **多货币体系**：金额按币种分组展示的多货币背景（`docs/prd/billing/multiple-currency.md`）

---

## 3. 需求概述

### 3.1 功能描述

计费统计为 Realm Admin 提供支付与积分的运营汇总视角：在一个统一「统计」页内，以固定 7/30 天窗口并列展示「支付统计」（成功/失败笔数、成功率、按渠道与币种的金额分组、按日趋势）与「积分消耗统计」（总消耗、有消耗用户数、按积分账户分组、按日趋势）。其本质是对既有支付尝试记录与积分交易流水的只读聚合，不改变任何购买、履约、发放或回收行为。

### 3.2 关键特性

- **已完结口径**：支付统计只统计已完结（成功/失败）支付尝试，进行中不计（DEC-billing-statistics-002）
- **金额永不跨币种相加**：多货币下金额按币种分组展示，任何位置不做跨币种相加（DEC-billing-statistics-002）
- **consume-only 口径**：积分消耗以扣减流水为准，回收/撤销/回退不冲减（DEC-billing-statistics-003）
- **单一事实源**：成功率等派生指标由前端从计数计算，后端不回传派生比率
- **零新增权限**：复用 `billing.view` / `points.view`，两面板分控互不隐含（DEC-billing-statistics-004）

---

## 4. 业务规则与状态

### 4.1 业务规则

**支付统计口径**：
- 只统计已完结支付尝试：成功计入收入金额与成功笔数，失败计入失败笔数；进行中（待支付/需操作/已取消/已过期）不计（DEC-billing-statistics-002）
- 时间归属按尝试发起日（`created_at`）落窗；异步完成的支付按发起日归属
- 金额按支付币种分组展示，任何位置不做跨币种相加（DEC-billing-statistics-002）
- 按支付渠道（provider）分组给出各自的成败笔数与按币种金额
- 金额以 Herald 记录的支付金额快照为准；provider 侧定价渠道（IAP/Creem 映射）金额快照可能为 0，如实呈现不估算（DEC-billing-statistics-002、DEC-multiple_currency-013）
- 成功率由前端从「成功 /（成功 + 失败）」计算，后端不回传派生比率（避免第二事实源）

**积分消耗口径**：
- 以积分扣减（consume）流水为准：退款回收（按未使用比例回收）、配额权益撤销、补偿回退均不冲减消耗统计（DEC-billing-statistics-003）
- 按积分账户（bucket）分组汇总，并给出按日趋势与窗口内「有消耗用户数」
- 时间归属按交易创建日落窗

**趋势与窗口规则**：
- 窗口仅「最近 7 天 / 最近 30 天」两档，非法窗口值被拒绝（DEC-billing-statistics-005）
- 按日趋势对无数据日期补零，按日期升序返回

**数据隔离规则**：
- 不同 Realm 的统计数据完全隔离；不提供跨 Realm 汇总
- 支付统计面板要求 `billing.view`，积分消耗面板要求 `points.view`，两权限互不隐含（DEC-billing-statistics-004）

### 4.2 关键状态与异常

**异常场景**：
- 非法窗口（非 7/30）：请求被拒绝（fail-loud），不回退默认窗口之外的两档
- 窗口内无任何已完结支付/消耗流水：返回全零统计与补零趋势，不视为错误
- provider 侧定价渠道金额快照为 0：如实呈现（已知口径差异），不估算、不隐藏该渠道分组

---

## 5. 功能需求

### 5.1 核心需求

**支付统计**：
- 给定窗口，返回成功/失败笔数、按币种分组的成功金额、按渠道分组的成败笔数与按币种金额、按日补零趋势
- 只统计已完结尝试，按发起日归属；金额按币种分组，绝不跨币种相加
- 成功率不在后端计算，由展示层从计数派生

**积分消耗统计**：
- 给定窗口，返回总消耗积分、有消耗用户数、按积分账户分组的消耗量、按日补零趋势
- 只统计扣减流水；回收/撤销/回退不冲减

**统计页**：
- 管理后台 billing 区「统计」页并列两面板，共用窗口切换（7/30）
- 两面板分别按 `billing.view` / `points.view` 独立分控；持有其一只见对应面板数据

### 5.2 验收目标

- 支付统计对窗口内已完结尝试给出成败笔数、渠道分组、按币种金额与按日补零趋势；进行中尝试不计入任何计数
- 多币种窗口下金额始终按币种分组呈现，无任何跨币种相加的合计金额
- 积分消耗统计按扣减流水聚合；窗口内发生过退款回收时，消耗统计不因此下降
- 窗口切换仅接受 7/30 两档，非法值被拒绝
- 仅持 `billing.view` 的管理员可取支付统计但取不到积分消耗统计（反之亦然），跨 Realm 不可见

---

## 6. API 相关约束

**适用性**: 适用

- **接口能力范围**：两个只读统计聚合查询（支付统计挂 `billing.view`、积分消耗挂 `points.view`），不改变既有购买/履约/积分端点；不在 PRD 列出端点、schema 或状态码。
- **访问控制原则**：遵守 realm 隔离；统计为只读聚合，不产生资金或积分变更；口径须与既有事实来源（支付尝试记录、积分交易流水）一致，不引入第二事实源。
- **租户/realm 边界**：统计数据按 Realm 严格隔离。
- **兼容性要求**：项目未上线，无迁移兼容负担；与支付渠道、积分账本的详细契约下沉到技术设计（`.ai/design/billing-statistics.md`）。

---

## 7. 前端/交互约束

**适用性**: 适用

- **统计页（billing 区）**：支付与积分消耗两面板并列，共用「最近 7 天 / 最近 30 天」窗口切换；侧边栏在 billing 区提供入口（DEC-billing-statistics-006）。
- **支付面板**：成功/失败笔数、成功率（前端派生）、按渠道分组表、按币种金额列表、按日趋势图。
- **积分面板**：总消耗、有消耗用户数、按积分账户分组表、按日趋势图。
- **状态反馈**：空窗口呈现全零态而非报错；权限不足的面板给出独立的无权限反馈。
- **金额/积分变化**：金额展示始终带币种；金额快照为 0 的渠道分组如实呈现，不隐藏。

---

## 8. 已确认决策

> 以下决策来自决策账本 `.ai/decision-log/billing-statistics.md`。仅记录带稳定 DEC ID 的已确认结论。

| Decision ID | 状态 | 决策项 | 结论 | PRD 落点 | 来源 |
|---|---|---|---|---|---|
| `DEC-billing-statistics-001` | Applied | 范围覆盖 | 一份 PRD 同时覆盖管理端「支付统计」与「积分消耗统计」；不含终端用户自助用量可视化 | §2.1、§3 | `.ai/decision-log/billing-statistics.md` |
| `DEC-billing-statistics-002` | Applied | 支付口径 | 只统计已完结支付尝试；金额按币种分组、任何位置不跨币种相加；按渠道分组；金额以 Herald 快照为准（provider 侧渠道可能为 0，如实呈现） | §3.2、§4.1、§5.1、§5.2 | 同上 |
| `DEC-billing-statistics-003` | Applied | 积分口径 | 以扣减流水为准，按 bucket 分组 + 按日趋势；退款回收/配额撤销/补偿回退均不冲减 | §3.2、§4.1、§5.1、§5.2 | 同上 |
| `DEC-billing-statistics-004` | Applied | 权限复用 | 支付统计挂 `billing.view`、积分消耗挂 `points.view`；不新增权限项、不挂 `dashboard.view` | §2.1、§4.1、§5.1、§6 | 同上 |
| `DEC-billing-statistics-005` | Applied | 时间窗口 | 首版固定「最近 7 天 / 最近 30 天」两档，不做自定义范围 | §2.1、§4.1、§7 | 同上 |
| `DEC-billing-statistics-006` | Applied | 页面入口 | billing 区统一「统计」页两面板并列、共用窗口；不混入 Dashboard 首屏 | §2.1、§7 | 同上 |

---

## 9. 参考资料

- 决策账本：`.ai/decision-log/billing-statistics.md`
- 技术设计：`.ai/design/billing-statistics.md`（及 `.ai/design/billing-statistics/` 分端文档）
- 相关 PRD：`docs/prd/billing/subscription.md`（订阅计费与统一支付尝试基线；本 PRD 为其 §2.2 延后项的立项）
- 相关 PRD：`docs/prd/billing/points.md`（积分交易与退款回收口径）
- 相关 PRD：`docs/prd/billing/credit-bucket.md`（积分账户分组维度）
- 相关 PRD：`docs/prd/billing/multiple-currency.md`（多货币背景与金额快照口径）
- 用户故事（支付统计关联）：`docs/user-stories/billing/payment-provider.md`（US-PV-005）
- 用户故事（免费用户统计，不随首版交付）：`docs/user-stories/billing/points-admin.md`（US-PO-007）
- 角色定义：`docs/user-stories/_roles.md`
