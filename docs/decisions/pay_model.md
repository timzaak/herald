# pay_model Decision Log

> 来源：`.ai/future/pay_model.md`（履约模型扩展候选规划）+ 2026-07-28 `/t-tech-research pay_model` 用户裁决。
> 背景：`support-iap`（2026-07-26 范围收窄、2026-07-28 发布）将买断与非续期订阅排除在 IAP 首期范围外，本 feature 承接该扩展。

## Active Decisions

| ID | Level | Topic | Decision | Rationale | Decided By | Source | Affects | Reopen When | Supersedes |
|---|---|---|---|---|---|---|---|---|---|
| DEC-pay_model-001 | D1 | fulfillment.buyout.storage | 买断不新增 `BillingType::Buyout`，以 `one_time` + `granted_role_ids` 永久角色授予表达（角色 `expires_at=None`，已有防重复购买）；需补 IAP 非消耗型商品区分与退款时永久角色回收 | 改动最小；现有 paywall 已落地 one_time+角色永久授予与 M3 防重复购买；接受"买断不进入 entitlement_key 订阅查询"的语义边界 | user | conversation (AskUserQuestion 2026-07-28) | prd/design/task | 出现必须以 entitlement_key 查询买断权益的集成方需求 | — |
| DEC-pay_model-002 | D1 | fulfillment.non-renewing.model | 非续期订阅新增 `BillingType::NonRenewing`，复用 Subscription 实体：履约创建固定时长、不自动续费的订阅（创建即带 `current_period_end`/`cancel_at`，无续费流转） | Subscription 实体字段与状态机（Active/Expired/has_access）足以表达；复用查询、SDK、管理视图，避免并联新实体 | user | conversation (AskUserQuestion 2026-07-28) | prd/design/task | 非续期与自动续期订阅的字段/状态冲突无法在对账层调和 | — |
| DEC-pay_model-003 | D1 | fulfillment.non-renewing.expiry | 非续期订阅到期失效仅依赖商店通知/轮询驱动：不新增本地到期扫描 job，不在权益查询叠加本地时间判断；接受 Apple 非续期订阅无服务端生命周期（getAllSubscriptionStatuses 仅覆盖 auto-renewable）导致的到期缺口 | 用户明确选择；Google 轮询（subscriptionsv2.get）可发现 EXPIRED；Apple 侧缺口为已接受风险 | user | conversation (AskUserQuestion 2026-07-28) | prd/design/task | Apple 侧到期失效成为上线验收硬要求 | — |
| DEC-pay_model-004 | D2 | scope.refund-role-revoke.channels | 退款/撤销时回收永久角色的修正对全渠道（Apple/Google/Stripe/Creem）的 one_time+角色购买生效，不限定 IAP 渠道 | one_time 撤销路径为全渠道共享代码路径；DEC-pay_model-001 要求"退款时永久角色回收"未限定渠道；按渠道过滤反而增加复杂度；技术预研 §6.2 显式假设 | agent | .ai/tech-research/pay_model.md §6.2 | prd/design/task | 需要把退款角色回收限定为 IAP 渠道 | — |
| DEC-pay_model-005 | D2 | fulfillment.non-renewing.duration-field | 非续期服务期时长使用新列 `provider_entitlement_mappings.service_duration_days`（INT NULL），不复用 `validity_days`；mapping 校验非续期必填，DB 加 CHECK 守卫 | `validity_days` 语义为 one_time 积分过期窗口（唯一消费点是 topup 过期），复用会造成按 billing_type 变化的语义重载（技术预研 §5.5 P2 风险）；新增可空列无兼容性损失；不改变产品语义（PRD 仅要求"服务期时长必填"） | agent | .ai/design/pay_model.md §4.3 | design/task | 出现必须复用 validity_days 的跨模型统一过期语义需求 | — |
| DEC-pay_model-006 | D2 | fulfillment.google.ack-consume-rule | Google one_time 履约的 ack/consume 选择：mapping 仅配置积分发放（`points_per_period > 0` 且不授予 role）→ `consume`（消耗型积分包）；否则（角色-only 买断或积分+role 买断礼包）→ `acknowledge_product` only，保留 Google 侧恢复购买记录（修订：首版谓词为 `points_per_period > 0` 即 consume，积分+role 场景后改为 acknowledge） | 消耗语义与"权益可被再次购买"绑定于积分发放；角色-only 的 one_time 即买断形态，consume 会破坏恢复购买；`GoogleDeveloperClient.acknowledge_product` 已存在未用；不改变现有消耗型积分包行为 | agent | .ai/design/pay_model.md §5.4 | design/task | Google 侧出现"发积分但不可消耗"的商品形态 | — |
| DEC-pay_model-007 | D2 | data.subscription.billing-type-snapshot | `subscription` 表新增 `billing_type` 列（`'recurring'/'non_renewing'`），履约创建时从 mapping 快照；对账过滤、管理视图与 api-ext 查询均读该列 | US-PM-003/US-PM-007 要求订阅可识别计费类型；对账须按 billing_type 过滤（DEC-pay_model-002 架构约束）；经 entitlement_key join mapping 不可靠（mapping 可改可删）；快照列是订阅实体的固有属性 | agent | .ai/design/pay_model.md §4.3 | design/task | 订阅与 mapping 必须强一致且接受 join 成本 | — |
| DEC-pay_model-008 | D1 | scope.compatibility.pre-launch | 产品未上线，本特性不考虑向后兼容：迁移直接变更结构（不做 DEFAULT 回填/灰度/双写），DTO 字段自由增改，不为旧客户端保留解析安全；现有 recurring/one_time 行为不回退属回归要求而非兼容承诺 | 用户明确指示（2026-07-28 `/t-design` 期间） | user | conversation | prd/design/task | 产品上线或已产生外部集成方存量数据 | — |

## Resolved Questions

| ID | Topic | Resolution | Decision ID | Source |
|---|---|---|---|---|
| Q-pay_model-001 | fulfillment.buyout.storage | 买断永久权益存储形态：复用 one_time + 永久角色，不新增 BillingType、不新建授予表 | DEC-pay_model-001 | conversation |
| Q-pay_model-002 | fulfillment.non-renewing.model | 非续期订阅复用 Subscription 实体 + BillingType::NonRenewing，不独立建模 | DEC-pay_model-002 | conversation |
| Q-pay_model-003 | fulfillment.non-renewing.expiry | 到期失效仅依赖商店通知/轮询，不加本地扫描与查询时判断 | DEC-pay_model-003 | conversation |
| Q-pay_model-004 | fulfillment.non-renewing.duration-field | 服务期时长使用新列 `service_duration_days`，不复用 `validity_days`；mapping 校验必填 + DB CHECK 守卫 | DEC-pay_model-005 | .ai/design/pay_model.md §4.3 |

## Deferred Questions

| ID | Topic | Why Non-blocking Now | Owner Stage | Must Resolve Before | User Informed |
|---|---|---|---|---|---|
| — | — | — | — | — | — |

## Superseded Decisions

| ID | Superseded By | Previous Decision | Source |
|---|---|---|---|
| — | — | — | — |
