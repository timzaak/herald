# billing-statistics Decision Log

> 创建：2026-09-08，由 `/t-prd billing-statistics` 初始化。
> 背景：代码事实核查（2026-09-08）确认支付统计与积分消耗统计在后端、前端、ext-API、MCP 全链路均不存在；`docs/prd/billing/subscription.md` §2.2 将"计费统计和报表"明确列为首版范围外，本 feature 是该延后项的正式立项。
> 注意：首轮澄清提问未送达用户（工具未获回答，暂按环境指示以推荐项继续）；2026-09-08 重新提问后用户显式确认 DEC-001 选择 `billing-statistics`。其余 DEC 均为 agent D2 取舍。

## Active Decisions

| ID | Level | Topic | Decision | Rationale | Decided By | Source | Affects | Reopen When | Supersedes |
|---|---|---|---|---|---|---|---|---|---|
| DEC-billing-statistics-001 | D0 | scope.coverage | 一份 PRD 同时覆盖管理端「支付统计」与「积分消耗统计」两个子域（feature 名 `billing-statistics`）；不包含终端用户自助用量可视化 | 用户上一轮确认两类统计均缺失并随即发起 `/t-prd 统计`（发起语境指向这两项）；两者同属 billing 域、共享统计页交互模式，拆分立项成本高于收益；终端用户用量可视化在 `.ai/future/next.md` §5 是独立后置候选 | user | conversation 2026-09-08（AskUserQuestion 用户确认；首问未送达，重问后显式确认） | prd/user-stories | 用户改选更窄（仅支付或仅积分）或更广（含用户侧用量页）范围 | — |
| DEC-billing-statistics-002 | D2 | caliber.payment | 支付统计口径：只统计已完结支付尝试（成功计入收入与成功率分子，失败计入失败数与分母，进行中不计）；金额按支付币种分组展示，任何位置不做跨币种相加；按支付渠道分组；金额以 Herald 记录的支付金额为准，provider 侧定价渠道缺可读价时以哨兵金额 1 落库（schema CHECK(amount>0) 禁止 0，见 DEC-multiple_currency-013），统计原样累加哨兵金额、如实呈现不估算 | 支付尝试记录是购买历史已在用的唯一事实来源；多货币体系下货币是显式选择维度、跨币种相加无意义（DEC-multiple_currency-014）；金额快照不可能为 0（IAP 缺价路径写哨兵 1 满足 CHECK，DEC-multiple_currency-013），渠道分组如实暴露哨兵口径差异 | agent | 代码事实核查 2026-09-08；`docs/decisions/multiple-currency.md` | prd/design/task | 业务要求净收入或渠道侧金额估算口径 | — |
| DEC-billing-statistics-003 | D2 | caliber.points | 积分消耗统计口径：以积分扣减流水为准，按积分账户（bucket）分组汇总并给出按日趋势；退款回收（按未使用比例回收）、配额权益撤销、补偿回退均不冲减消耗统计 | `docs/prd/billing/points.md` 规定退款回收只回收未使用部分、已消费量不反向调整，冲减式口径会掩盖真实用量；钱包行既有 `total_consumed` 仅有累计值、无维度与时间轴，无法支撑运营分析 | agent | 代码事实核查 2026-09-08；`docs/prd/billing/points.md` §"退款积分回收" | prd/design/task | 业务要求净消耗（消耗-回收）口径 | — |
| DEC-billing-statistics-004 | D2 | permission.reuse | 复用既有权限项：支付统计面板挂 `billing.view`，积分消耗面板挂 `points.view`；不新增权限项，不挂 `dashboard.view` | `_roles.md` 中两权限已精确覆盖对应数据可见性且 Realm Admin 默认持有；Dashboard 现范围仅用户活跃指标（`docs/prd/core/dashboard.md` §2.2），挂 `dashboard.view` 会造成权限语义混淆 | agent | `docs/user-stories/_roles.md`；`docs/prd/core/dashboard.md` | prd/design | 统计页需要独立于数据查看的授权粒度 | — |
| DEC-billing-statistics-005 | D2 | ui.time-window | 时间窗口首版固定「最近 7 天 / 最近 30 天」两档切换，不做自定义时间范围 | 对齐 Dashboard 首版先例（固定 7/30 天窗口、自定义范围明确 out of scope）与 US-PV-005 场景 2（Last 7/30 days） | agent | `docs/prd/core/dashboard.md` §2.2；US-PV-005 场景 2 | prd/design | 用户需要任意起止日期的自定义窗口 | — |
| DEC-billing-statistics-006 | D2 | ui.entry | 管理后台 billing 区新增统一「统计」页面，支付与积分消耗两个面板并列、共用同一时间窗口选择；不在既有 Dashboard 首屏混入计费指标 | Dashboard 范围明确为用户活跃指标，混入计费指标需改动其已发布范围；billing 区已有 invoices/subscriptions 等页面，统计入口在此语义聚合 | agent | `docs/prd/core/dashboard.md` §2.2 | prd/design | 用户要求统计上移到 Dashboard 首屏 | — |

## Resolved Questions

| ID | Topic | Resolution | Decision ID | Source |
|---|---|---|---|---|
| Q-billing-statistics-000 | scope.feature-name | feature 名定为 `billing-statistics`（「统计」不满足 t-prd 文件名字符约束，且需名称承载范围裁定） | DEC-billing-statistics-001 | conversation 2026-09-08（AskUserQuestion 用户确认） |
| Q-billing-statistics-001 | scope.export-csv | CSV 导出不随首版交付：设计（`.ai/design/billing-statistics.md` 及分端文档）不含任何导出接口或页面能力；未来需要时按独立迭代立项 | —（沿用 PRD §2.2 范围界定，无新 DEC） | `.ai/design/billing-statistics.md` §1.3/§1.4（t-design 2026-09-08 关闭） |

## Deferred Questions

| ID | Topic | Why Non-blocking Now | Owner Stage | Must Resolve Before | User Informed |
|---|---|---|---|---|---|
| Q-billing-statistics-002 | scope.end-user-usage | 终端用户自助用量可视化是 `.ai/future/next.md` §5 独立后置候选，重启条件已有记录；本 PRD 范围已明确为管理端 | t-decision（未来迭代） | 该方向立项前 | yes |
| Q-billing-statistics-003 | scope.us-pv005-us-po007-remainder | US-PV-005 剩余指标（Active Subs、Avg Payment Time）与 US-PO-007 免费用户发放/转化统计不随首版交付，承接排期待定 | t-decision（排期） | 下一迭代排期时 | yes |

## Superseded Decisions

| ID | Superseded By | Previous Decision | Source |
|---|---|---|---|
