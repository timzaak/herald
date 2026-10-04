# refund-patch Decision Log

## Active Decisions

| ID | Level | Topic | Decision | Rationale | Decided By | Source | Affects | Reopen When | Supersedes |
|---|---|---|---|---|---|---|---|---|---|
| DEC-refund-patch-001 | D0 | scope.refund-optimization | 本轮范围 = ①修复多次部分退款积分过度回收（Stripe 路径把累计退款额当单次退款额使用）；②一次性购买角色回收细化为「仅累计全额退款回收」。订阅退款「任意退款→立即整单取消」政策维持现状，列为 out of scope | ①为确定资金正确性 bug；②用户要求细化；订阅粗粒度政策在业界主流范畴内（Stripe Entitlements 亦为退款不自动撤权） | user | conversation 2026-09-14 | prd/design/task/test | 出现订阅部分退款需按比例保留权益的真实需求 | — |
| DEC-refund-patch-002 | D1 | refund.role-clawback.full-refund-only | 一次性购买（充值/买断）授予的支付来源角色：部分退款（任意比例、无论几笔）保留角色；仅当该笔支付累计退款达到原支付金额 100% 时回收。争议/撤销（dispute）事件不属于退款，不受本规则约束，维持照常回收。覆盖已发布 US-PW-005 场景4 及 support-paywall / pay_model PRD 的「退款即回收一次性角色」表述 | 部分退款通常是商家善意补偿，不应剥夺购买凭证；退款由商家主动发起，大比例退款保角色的滥用风险可控；与主流做法一致 | user | conversation 2026-09-14 | prd/design/task/test + publish 阶段同步 US-PW-005 场景4、docs/prd/billing/support-paywall.md、docs/prd/billing/pay_model.md | 出现部分退款套利（大比例退款保角色）的实际滥用案例 | — |
| DEC-refund-patch-003 | D1 | refund.points-revocation.incremental-semantics | 积分回收语义固化为「以单笔退款增量为单位按比例回收」：每笔退款按其金额占原支付的比例回收对应授予积分，多次部分退款的累计回收对齐累计退款比例；只回收未消费部分；幂等以单笔退款（provider 退款单）为单位 | 修复 Stripe charge.refunded 累计值被当增量使用的过度回收；Creem 路径已是该语义，本决策将其固化为全渠道不变量 | user | conversation 2026-09-14 | prd/design/task/test | 支付方退款事件语义变更 | — |
| DEC-refund-patch-004 | D1 | refund.historical-compensation | 不回溯补偿历史过度回收的积分与历史因部分退款被回收的角色；修复仅对未来退款事件生效；个案由管理员走既有手动调整途径 | 历史回收与消费交错无法精确重建差额；角色状态可能已被后续事件覆盖（重新购买、订阅变化），批量回补有二次错误风险 | user | conversation 2026-09-14 | prd/test | 出现可批量精确重建差额的可靠数据源 | — |
| DEC-refund-patch-005 | D0 | refund.dispute-one-time-out-of-scope | 争议/撤销事件处理完全维持现状，不入本期：现状是一次性购买的争议事件不回收任何角色（Stripe dispute 仅处理订阅、映射不到订阅时忽略告警；Creem dispute.created 无订阅时 400），订阅争议走既有订阅状态流。US-RP-002 场景 4 改写为钉住「争议事件不触发角色回收、部分退款保留不因争议事件失效」；一次性争议回收能力缺口（与已发布 US-PW-005 场景 4「撤销事件回收」表述的差距）另立方案解决 | 澄清 DEC-002「维持照常回收」的事实基线：代码现状并无一次性争议回收可「照常」，实现它需设计 dispute created/lost/won 生命周期，超出本正确性修复的范围；DEC-002 的保留规则豁免部分不变 | user | conversation 2026-09-14 | prd/design/task/test | 一次性购买争议回收的真实需求出现并另立方案 | — |

## Resolved Questions

| ID | Topic | Resolution | Decision ID | Source |
|---|---|---|---|---|
| Q-refund-patch-001 | scope.refund-optimization | 修积分回收 bug + 角色回收细化；订阅退款维持现状 | DEC-refund-patch-001 | conversation 2026-09-14 |
| Q-refund-patch-002 | refund.role-clawback.full-refund-only | 仅全额退款（累计 100%）回收角色；部分退款保留 | DEC-refund-patch-002 | conversation 2026-09-14 |
| Q-refund-patch-003 | refund.historical-compensation | 不补偿，仅修后续 | DEC-refund-patch-004 | conversation 2026-09-14 |
| Q-refund-patch-004 | refund.dispute-one-time-revocation | 争议回收保持范围外；US-RP-002 场景 4 改写钉住现状（争议事件不回收角色、保留不失效）；能力缺口另立方案 | DEC-refund-patch-005 | conversation 2026-09-14 |

## Deferred Questions

| ID | Topic | Why Non-blocking Now | Owner Stage | Must Resolve Before | User Informed |
|---|---|---|---|---|---|

## Superseded Decisions

| ID | Superseded By | Previous Decision | Source |
|---|---|---|---|
