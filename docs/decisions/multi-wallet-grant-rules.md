# multi-wallet-grant-rules Decision Log

> 来源：2026-07-29 当前对话、已发布积分账户/积分/订阅 PRD 与现有实现。
> 背景：现有模型允许用户持有多个积分账户，但购买映射和注册配置仍各自只能把一次触发路由到一个账户和一组积分策略。

## Active Decisions

| ID | Level | Topic | Decision | Rationale | Decided By | Source | Affects | Reopen When | Supersedes |
|---|---|---|---|---|---|---|---|---|---|
| DEC-multi-wallet-grant-rules-001 | D1 | grants.multi-target.scope | 一个购买事件或注册事件可同时命中多条积分分发规则；每条规则独立指定目标积分账户和发放策略 | 用户明确要求一次购买、注册能够同时触发不同钱包和不同规则 | user | conversation 2026-07-29 | prd/design/task/test | 需求收窄为单目标，或新增其他触发源 | — |
| DEC-multi-wallet-grant-rules-002 | D1 | compatibility.pre-launch | 产品未上线，本特性采用破坏性重建；不保留旧单规则 DTO/字段，不做历史数据回填、双写或灰度兼容 | 用户明确说明当前未上线、无需考虑兼容性 | user | conversation 2026-07-29 | prd/design/task/test | 产品上线或产生外部存量集成 | — |
| DEC-multi-wallet-grant-rules-003 | D1 | grants.rule-ownership | 分发规则属于具体触发配置；每条规则只指定一个目标积分账户，一个触发配置通过持有多条规则实现多账户扇出；积分账户本身不承载规则 | 保持“账户是池与消费隔离边界、规则属于购买/注册配置”的既有职责，同时满足一对多 | user | conversation 2026-07-29 + docs/prd/billing/credit-bucket.md §4.1 | prd/design/task/test | 需要跨购买/注册复用规则模板，或规则改为账户固有属性 | — |
| DEC-multi-wallet-grant-rules-005 | D2 | grants.atomic-idempotency | 同一业务事件的全部积分规则在一个数据库事务中执行；幂等键细化为“事件 + 规则”，重放返回原结果，任一规则失败则本次积分扇出整体回滚 | 防止部分钱包到账；保留现有支付/注册 fail-loud 与重复事件不重复发放约束 | agent | .ai/design/multi-wallet-grant-rules.md §4.1, §5.3 | design/task/test | 数据库事务无法覆盖某种新增的外部副作用 | — |
| DEC-multi-wallet-grant-rules-006 | D2 | data.remove-singular-routing | 从 Mapping、PaymentAttempt、Subscription 和 Realm 默认配置中移除单一钱包/单一积分策略字段，改由规则集合与规则绑定表达 | 保留旧字段会形成双真源；DEC-002 已允许直接替换 | agent | .ai/design/multi-wallet-grant-rules.md §4.3 | design/task/test | 需要兼容已上线旧客户端或历史记录 | — |
| DEC-multi-wallet-grant-rules-007 | D2 | grants.rule-lifecycle | 已被业务记录引用的规则只允许禁用，不物理删除；禁用只影响后续触发，既有账本、额度权益、退款与回收仍按规则 ID 追踪 | 保证生命周期回收和幂等定位稳定，同时避免额外规则快照审计表 | agent | .ai/design/multi-wallet-grant-rules.md §4.1, §5.1 | design/task/test | 产品要求彻底删除规则及其历史归因 | — |
| DEC-multi-wallet-grant-rules-008 | D2 | api.rule-management | 复用现有 Mapping 与 Realm 积分配置入口，在响应和写入 DTO 中以规则数组替换单一策略字段；不新增平行的通用规则管理模块 | 规则只在其拥有者上下文中有意义，复用现有权限、路由和页面可减少新增概念 | agent | .ai/design/multi-wallet-grant-rules.md §4.2, §4.4 | design/task/test | 规则需要跨触发器搜索、复用或独立授权 | — |
| DEC-multi-wallet-grant-rules-009 | D1 | grants.trigger-catalog | 持久化分发规则只覆盖当前生产代码中的六类自动发放触发：`topup`、`subscription_initial`、`subscription_renewal`、`subscription_upgrade`、`registration`、`free_periodic_grant`；`admin_grant` 与 `sdk_grant` 保持显式定向命令，回收来源由原发放派生，`system_grant` 暂无生产入口不纳入 | 用户要求触发源以当前项目事实为准，并明确确认主动发放不由规则接管 | user | conversation 2026-07-29 + backend/domain/src/points/entities.rs | prd/design/task/test | 新增生产自动发放入口，或主动发放需要规则化 | — |
| DEC-multi-wallet-grant-rules-010 | D2 | data.rule-storage | 使用单一 `points_distribution_rules` 表承载六类自动触发规则，以受限 owner 类型区分 Mapping 与 Realm 注册配置；账本、额度权益和周期计划统一引用规则 ID | 新的触发源目录要求跨固定积分、额度权益、计划和回收统一追踪；单表可提供一个真实外键，避免两套规则表带来的多态引用，同时 owner/trigger CHECK 将范围锁定在本 feature | agent | .ai/design/multi-wallet-grant-rules.md §4.3 | design/task/test | 规则拥有者或策略字段出现无法用清晰 CHECK 表达的结构分裂 | DEC-multi-wallet-grant-rules-004 |
| DEC-multi-wallet-grant-rules-011 | D1 | grants.periodic-policy | 显式推翻已发布积分基线中的 subscription/free-periodic quota-only 约束；订阅和免费周期规则均允许 fixed 周期积分或滚动窗口 quota | 用户确认本 feature 不只改变单目标路由，也恢复 fixed 周期积分作为可配置策略 | user | conversation 2026-07-29 | prd/design/task/test | 产品重新收敛为 quota-only，或 fixed 周期执行成本不可接受 | — |
| DEC-multi-wallet-grant-rules-012 | D2 | grants.event-replay-journal | 每个分发事件在积分结果同一事务内写入唯一完成记录；完成记录包含零规则结果，重放先返回原事件结果，不重新解析当前规则集合 | 仅靠“事件 + 规则”幂等无法区分首次零规则与尚未执行，也无法在规则集合变更后恢复原事件完整结果 | agent | .ai/quality/design-check-multi-wallet-grant-rules-20260729-171447.md P0-1 + .ai/design/multi-wallet-grant-rules.md §4.3, §5.3 | design/task/test | 业务源本身提供可复用且覆盖零结果的等价原子完成记录 | — |
| DEC-multi-wallet-grant-rules-013 | D2 | grants.internal-quota-attribution | 保留 demo/test-only internal quota 直写入口；该入口写入的 quota entitlement 允许 `distribution_event_id` 与 `distribution_rule_id` 同时为空，生产分发执行器写入时两者必须同时非空 | 该入口用于快速 Demo 构造配额，不代表生产自动发放；强制其创建持久化业务规则会把测试夹具耦合到管理配置 | agent | backend/api-points/src/internal_quota.rs + .ai/design/multi-wallet-grant-rules.md §4.3, §6 | design/task/test | internal quota 入口进入生产范围，或 Demo 改为完整购买链路 | — |
| DEC-multi-wallet-grant-rules-015 | D2 | task.backend-dev-split | backend/dev 使用 6 个顺序执行的责任闭环 item；用户明确授权超过默认 3-item 上限，并覆盖本阶段“单项超过 10 个核心文件必须拆分”的规则 | 保持六个业务责任闭环，接受部分 item 文件面较大，以避免继续增加执行切换与 handoff | user | conversation 2026-07-29 | task | 后端设计范围显著变化，或用户撤回本次拆分豁免 | DEC-multi-wallet-grant-rules-014 |
| DEC-multi-wallet-grant-rules-016 | D2 | task.frontend-dev-split | frontend/dev 使用 4 个顺序执行的责任闭环 item（规则编辑基础层、Mapping 域、Realm 注册规则页、Bucket/购买展示与全局收尾）；用户明确授权超过默认 3-item 上限 | 合并为 3 项会把 Realm 注册配置、Credit Bucket、用户购买展示三个弱相关页面域压进单项且恰好 10 个核心文件无余量，购买页 quota/fixed 求和风险的失败归因被稀释 | user | conversation 2026-07-29 | task | 前端设计范围显著变化，或用户撤回本次拆分豁免 | — |
| DEC-multi-wallet-grant-rules-017 | D1 | demo.admin-editor-coverage | Demo 必须包含管理端 Mapping 与 Realm 注册规则编辑能力；迁移现有老旧管理端 Demo，不得只用 Seed 覆盖配置步骤 | 用户明确指出管理端编辑能力已有 Demo 但代码老旧，需要随规则列表模型调整 | user | conversation 2026-07-31 | demo/test | 用户明确收窄 Demo 为纯用户侧结果，或管理端 UI 被移出本 feature | — |

## Resolved Questions

| ID | Topic | Resolution | Decision ID | Source |
|---|---|---|---|---|
| Q-multi-wallet-grant-rules-001 | grants.multi-target.scope | 购买和注册均改为一对多积分分发 | DEC-multi-wallet-grant-rules-001 | conversation |
| Q-multi-wallet-grant-rules-002 | compatibility.pre-launch | 直接替换旧结构，不做兼容与回填 | DEC-multi-wallet-grant-rules-002 | conversation |
| Q-multi-wallet-grant-rules-003 | grants.rule-ownership | 规则归触发配置所有，每条规则指向一个账户 | DEC-multi-wallet-grant-rules-003 | conversation + published PRD |
| Q-multi-wallet-grant-rules-004 | grants.trigger-catalog | 规则覆盖六类现有自动发放来源；管理员与 SDK 主动发放不接管，回收不是独立配置触发源 | DEC-multi-wallet-grant-rules-009 | conversation + code inspection |
| Q-multi-wallet-grant-rules-005 | grants.periodic-policy | 订阅和免费周期规则显式允许 fixed 周期积分与滚动窗口 quota 两种策略，覆盖已发布 quota-only 基线 | DEC-multi-wallet-grant-rules-011 | conversation |
| Q-multi-wallet-grant-rules-006 | task.backend-dev-split | 授权 backend/dev 使用 6 个 items，超过默认上限 3，并忽略本阶段单项超过 10 个核心文件的强制拆分规则 | DEC-multi-wallet-grant-rules-015 | conversation |
| Q-multi-wallet-grant-rules-007 | task.frontend-dev-split | 授权 frontend/dev 使用 4 个 items，超过默认上限 3 | DEC-multi-wallet-grant-rules-016 | conversation |
| Q-multi-wallet-grant-rules-008 | demo.admin-editor-coverage | 管理端 Mapping 与 Realm 注册规则编辑必须进入 Demo，并迁移已有旧 Demo | DEC-multi-wallet-grant-rules-017 | conversation |

## Deferred Questions

| ID | Topic | Why Non-blocking Now | Owner Stage | Must Resolve Before | User Informed |
|---|---|---|---|---|---|
| — | — | — | — | — | — |

## Superseded Decisions

| ID | Superseded By | Previous Decision | Source |
|---|---|---|---|
| DEC-multi-wallet-grant-rules-004 | DEC-multi-wallet-grant-rules-010 | 购买规则与注册规则分别使用子表建模 | design refinement after trigger catalog confirmation |
| DEC-multi-wallet-grant-rules-014 | DEC-multi-wallet-grant-rules-015 | backend/dev 使用 6 个责任闭环 item，但未覆盖单项超过 10 个核心文件的强制拆分规则 | conversation 2026-07-29 |
