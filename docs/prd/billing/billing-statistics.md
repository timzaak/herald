# 计费统计（支付与积分消耗）产品需求文档 (PRD)

**创建时间**: 2026-09-12
**优先级**: P1

> 场景背景：管理端对支付与积分此前只有逐笔流水（购买历史、订阅、发票、积分流水），没有任何汇总视角。本 PRD 是 `docs/prd/billing/subscription.md` §2.2 中「计费统计和报表：不在首版范围」这一延后项的正式立项（DEC-billing-statistics-001），交付管理后台 billing 区统一「统计」页：支付统计与积分消耗统计两个面板。本文档不承载接口端点、请求/响应 schema、HTTP 状态码、数据库建表/迁移或代码类型定义；技术方案细节请参见对应技术设计。

---

## 1. 相关用户故事

> 详细故事与验收标准请查看 `docs/user-stories/` 中对应文档。

| US-ID | 标题 | 优先级 | 来源 |
|-------|------|--------|------|
| US-BS-001 | 查看支付统计总览 | P1 | `docs/user-stories/billing/billing-statistics.md` |
| US-BS-002 | 查看积分消耗统计 | P1 | `docs/user-stories/billing/billing-statistics.md` |
| US-PV-005 | 查看支付平台使用统计——本 PRD 交付其「按渠道分组的成功/失败笔数、成功率、按币种金额」视角；Active Subs、Avg Payment Time 指标不随首版交付（Q-billing-statistics-003，排期另定） | P2 | `docs/user-stories/billing/payment-provider.md` |
| US-PO-007 | 查看免费用户积分统计——该故事主体为免费用户发放/转化漏斗，不随本 PRD 交付（Q-billing-statistics-003）；本 PRD 的消耗口径（按积分账户分组、按日趋势）与之互补但不重叠 | P1 | `docs/user-stories/billing/points-admin.md` |

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
- **CSV / 数据导出**：不含任何导出能力，未来按独立迭代立项
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

## 4. 业务规则与状态

### 4.1 业务规则

**支付统计口径**：
- 只统计已完结支付尝试：成功计入收入金额与成功笔数，失败计入失败笔数；进行中（待支付/需操作/已取消/已过期）不计（DEC-billing-statistics-002）
- 时间归属按尝试发起日（`created_at`）落窗；异步完成的支付按发起日归属
- 金额按支付币种分组展示，任何位置不做跨币种相加（DEC-billing-statistics-002）
- 按支付渠道（provider）分组给出各自的成败笔数与按币种金额
- 金额以 Herald 记录的支付金额快照为准；金额列受 `CHECK(amount > 0)` 约束，不可能为 0——provider 侧定价渠道（IAP/Creem 映射）缺可读价时以哨兵金额 1（最小货币单位）落库，统计将哨兵金额原样累加进币种聚合（已知口径偏差，如实呈现、不估算、不隐藏该渠道分组）（DEC-billing-statistics-002、DEC-multiple_currency-013）
- 成功率由前端从「成功 /（成功 + 失败）」计算，后端不回传派生比率（避免第二事实源）

**积分消耗口径**：
- 以积分扣减（consume）流水为准：退款回收（按未使用比例回收）、配额权益撤销、补偿回退均不冲减消耗统计（DEC-billing-statistics-003）
- 按积分账户（bucket）分组汇总，并给出按日趋势与窗口内「有消耗用户数」
- 时间归属按交易创建日落窗

**趋势与窗口规则**：
- 窗口仅「最近 7 天 / 最近 30 天」两档，非法窗口值被拒绝（DEC-billing-statistics-005）；`days` 参数缺省时回退 7 天
- 按日趋势对无数据日期补零，按日期升序返回

**数据隔离规则**：
- 不同 Realm 的统计数据完全隔离；不提供跨 Realm 汇总
- 支付统计面板要求 `billing.view`，积分消耗面板要求 `points.view`，两权限互不隐含（DEC-billing-statistics-004）

### 4.2 关键状态与异常

**异常场景**：
- 非法窗口（非 7/30）：请求被拒绝（fail-loud），不回退默认窗口之外的两档
- 窗口内无任何已完结支付/消耗流水：返回全零统计与补零趋势，不视为错误
- provider 侧定价渠道缺可读价：金额快照为哨兵 1（schema CHECK 禁止 0），其金额参与统计累加（已知口径差异），不估算、不隐藏该渠道分组

---

## 5. 验收目标

- 支付统计对窗口内已完结尝试给出成败笔数、渠道分组、按币种金额与按日补零趋势；进行中尝试不计入任何计数
- 多币种窗口下金额始终按币种分组呈现，无任何跨币种相加的合计金额
- 积分消耗统计按扣减流水聚合；窗口内发生过退款回收时，消耗统计不因此下降
- 窗口切换仅接受 7/30 两档，非法值被拒绝
- 仅持 `billing.view` 的管理员可取支付统计但取不到积分消耗统计（反之亦然），跨 Realm 不可见

---

## 6. 边界与约束

**适用性**: 适用（API 与前端/交互边界合并陈述）

**API / 集成边界:**
- **接口能力范围**：两个只读统计聚合查询（支付统计挂 `billing.view`、积分消耗挂 `points.view`），不改变既有购买/履约/积分端点；不在 PRD 列出端点、schema 或状态码。
- **访问控制原则**：遵守 realm 隔离；统计为只读聚合，不产生资金或积分变更；口径须与既有事实来源（支付尝试记录、积分交易流水）一致，不引入第二事实源。
- **租户/realm 边界**：统计数据按 Realm 严格隔离。
- **兼容性要求**：项目未上线，无迁移兼容负担；与支付渠道、积分账本的详细契约下沉到技术设计（`.ai/design/billing-statistics.md`）。

**前端 / 交互边界:**
- **统计页（billing 区）**：支付与积分消耗两面板并列，共用「最近 7 天 / 最近 30 天」窗口切换；侧边栏在 billing 区提供入口（DEC-billing-statistics-006）。
- **支付面板**：成功/失败笔数、成功率（前端派生）、按渠道分组表、按币种金额列表、按日趋势图。
- **积分面板**：总消耗、有消耗用户数、按积分账户分组表、按日趋势图。
- **状态反馈**：空窗口呈现全零态而非报错；无对应权限的面板不渲染（也不发起其查询），两面板权限均缺失时页面给出整体无权限反馈。
- **金额/积分变化**：金额展示始终带币种；缺价渠道的哨兵金额分组如实呈现，不隐藏。

---

## 7. 已确认决策

> 本节只收录当前有效的决策与未决问题，记取舍、理由、决策人与重开条件；规则正文只在 §4 定义，DEC/Q 编号保持稳定，供代码注释、测试与跨 PRD 引用追溯。

- **DEC-billing-statistics-001 · 范围覆盖**（user，2026-09-08 会话确认）：一份 PRD 同时覆盖管理端「支付统计」与「积分消耗统计」两个子域；不包含终端用户自助用量可视化。理由：两类统计同属 billing 域、共享统计页交互模式，拆分立项成本高于收益；终端用户用量可视化是独立后置候选。落点：§2.1。重开条件：用户改选更窄（仅支付或仅积分）或更广（含用户侧用量页）范围。
- **DEC-billing-statistics-002 · 支付口径**（agent）：只统计已完结支付尝试；金额按币种分组、任何位置不跨币种相加；按渠道分组；金额以 Herald 快照为准，provider 侧定价渠道缺可读价时按哨兵金额 1 原样累加、不估算（规则正文见 §4.1）。理由：支付尝试记录是购买历史已在用的唯一事实来源；多货币体系下货币是显式选择维度、跨币种相加无意义（`DEC-multiple_currency-014`）；金额快照不可能为 0（IAP 缺价路径写哨兵 1 满足 CHECK，`DEC-multiple_currency-013`），渠道分组如实暴露哨兵口径差异。落点：§2.1、§4.1、§5。重开条件：业务要求净收入或渠道侧金额估算口径。
- **DEC-billing-statistics-003 · 积分口径**（agent）：以积分扣减流水为准，按积分账户（bucket）分组汇总并给出按日趋势；退款回收、配额权益撤销、补偿回退均不冲减消耗统计（规则正文见 §4.1）。理由：`docs/prd/billing/points.md` 规定退款回收只回收未使用部分、已消费量不反向调整，冲减式口径会掩盖真实用量；钱包行既有 `total_consumed` 仅有累计值、无维度与时间轴，无法支撑运营分析。落点：§2.1、§4.1、§5。重开条件：业务要求净消耗（消耗-回收）口径。
- **DEC-billing-statistics-004 · 权限复用**（agent）：支付统计面板挂 `billing.view`，积分消耗面板挂 `points.view`；不新增权限项，不挂 `dashboard.view`。理由：`docs/user-stories/_roles.md` 中两权限已精确覆盖对应数据可见性且 Realm Admin 默认持有；Dashboard 现范围仅用户活跃指标（`docs/prd/core/dashboard.md` §2.2），挂 `dashboard.view` 会造成权限语义混淆。落点：§2.1、§4.1、§6。重开条件：统计页需要独立于数据查看的授权粒度。
- **DEC-billing-statistics-005 · 时间窗口**（agent）：首版固定「最近 7 天 / 最近 30 天」两档切换，不做自定义时间范围。理由：对齐 Dashboard 首版先例（固定 7/30 天窗口）与 US-PV-005 场景 2（Last 7/30 days）。落点：§2.1、§2.2、§4.1、§6。重开条件：用户需要任意起止日期的自定义窗口。
- **DEC-billing-statistics-006 · 页面入口**（agent）：管理后台 billing 区新增统一「统计」页面，支付与积分消耗两个面板并列、共用同一时间窗口选择；不在既有 Dashboard 首屏混入计费指标。理由：Dashboard 范围明确为用户活跃指标，混入计费指标需改动其已发布范围；billing 区已有 invoices/subscriptions 等页面，统计入口在此语义聚合。落点：§2.1、§2.2、§6。重开条件：用户要求统计上移到 Dashboard 首屏。

**问题记录**（原账本 Resolved / Deferred Questions）：

- `Q-billing-statistics-002`（延期）：终端用户自助用量可视化是独立后置候选，重启条件已有记录；本 PRD 范围已明确为管理端。须在该方向立项前决议。
- `Q-billing-statistics-003`（延期）：US-PV-005 剩余指标（Active Subs、Avg Payment Time）与 US-PO-007 免费用户发放/转化统计不随首版交付，承接排期待定。须在下一迭代排期时决议。

---

## 8. 参考资料

- 用户故事来源见 §1 表格
- 技术设计：`.ai/design/billing-statistics.md`（及 `.ai/design/billing-statistics/` 分端文档）
- 相关 PRD：`docs/prd/billing/subscription.md`（订阅计费与统一支付尝试基线；本 PRD 为其 §2.2 延后项的立项）
- 相关 PRD：`docs/prd/billing/points.md`（积分交易与退款回收口径）
- 相关 PRD：`docs/prd/billing/credit-bucket.md`（积分账户分组维度）
- 相关 PRD：`docs/prd/billing/multiple-currency.md`（多货币背景与金额快照口径）
- 角色定义：`docs/user-stories/_roles.md`
