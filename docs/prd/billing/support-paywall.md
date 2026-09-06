# 支付驱动权益门控（Paywall）产品需求文档 (PRD)

**创建时间**: 2026-07-08
**优先级**: P0

---

## 1. 相关用户故事

> 详细故事与验收标准请查看 [docs/user-stories/billing/support-paywall.md](/docs/user-stories/billing/support-paywall.md)。

### 1.1 相关故事

- `[US-PW-001]` 配置 entitlement 映射的 role 授予维度，优先级 P0，来源 [docs/user-stories/billing/support-paywall.md](/docs/user-stories/billing/support-paywall.md)
  - 角色：Realm Admin
  - 摘要：为任意 entitlement mapping 配置「授予哪些 role」，与 billing_type、积分策略正交叠加

- `[US-PW-002]` 一次性纯权益购买成功且不报错，优先级 P0，来源 [docs/user-stories/billing/support-paywall.md](/docs/user-stories/billing/support-paywall.md)
  - 角色：Regular User
  - 摘要：one-time 不配积分的纯权益履约不再报 500，与 recurring 容错一致

- `[US-PW-003]` 支付成功自动授予 role，优先级 P0，来源 [docs/user-stories/billing/support-paywall.md](/docs/user-stories/billing/support-paywall.md)
  - 角色：Regular User
  - 摘要：支付成功自动授权，一次性=永久、订阅=周期内有效，且与手工授予可追溯区分

- `[US-PW-004]` 一次性永久权益一人一次防重复购买，优先级 P0，来源 [docs/user-stories/billing/support-paywall.md](/docs/user-stories/billing/support-paywall.md)
  - 角色：Regular User
  - 摘要：仅「one_time + 授予 role」组合强制一人一次；积分包保持可重复

- `[US-PW-005]` 支付事件触发 role 撤销，优先级 P0，来源 [docs/user-stories/billing/support-paywall.md](/docs/user-stories/billing/support-paywall.md)
  - 角色：Realm Admin（系统代为执行）
  - 摘要：订阅取消、过期或退款，以及一次性购买退款或撤销时，回收支付来源 role，幂等且最终一致

- `[US-PW-006]` 第三方应用凭 role 一行判断解锁功能，优先级 P0，来源 [docs/user-stories/billing/support-paywall.md](/docs/user-stories/billing/support-paywall.md)
  - 角色：Third-party App
  - 摘要：复用 Herald 现有 RBAC 运行时，不新建权限空间，Herald 不解释功能语义

### 1.2 优先级汇总

| 优先级 | 数量 | 关键故事 |
|--------|------|----------|
| P0 | 6 | role 授予维度、one-time 一致性修复、支付自动授权、一人一次防重复、支付来源 role 撤销、第三方应用 RBAC 判断 |
| P1 | 0 | - |
| P2 | 0 | - |

---

## 2. 范围界定

### 2.1 包含功能

- **W1 — one-time 履约一致性修复**：允许 billing_type=one_time 的 entitlement mapping 不配积分（纯权益型），履约时记录支付成功、不报错、不发积分，行为与 recurring 容错对齐
- **M1 — role 授予横切维度**：entitlement mapping 新增「支付成功后授予哪些 role」配置，与 billing_type、points 策略三者正交叠加
- **M2 — 支付成功自动授权**：支付成功 webhook 触发自动授予映射配置的 role（一次性=永久解锁不设过期；订阅=周期内有效）
- **M3 — 一次性永久权益一人一次**：仅当「one_time + 授予 role」组合时，购买前检查是否已成功购买或已拥有该 role，防重复与防并发双购
- **M4 — 订阅类 role 撤销**：复用现有 subscription.canceled/expired/refund webhook 链路与补偿框架，撤销因支付授予的 role，幂等且最终一致
- 权限来源可追溯：支付授予与 Realm Admin 手工授予并存时，撤销仅移除支付来源，手工授予不受影响

### 2.2 不包含功能 (Out of Scope)

- 裂变 billing_type 或新增商品类型/新名词——横切叠加已统一
- 一次性购买 role 关联的过期撤销——一次性=永久解锁，不设过期；系统不引入后台 scheduler/cron
- Herald 自建第三方功能目录（features/quotas）——维持现有决策
- 支付驱动权限授予 Herald 内置管理端权限（dashboard/billing 等）——语义不符
- 权限撤销的强实时性 SLA——webhook 延迟属固有约束，本期只定义容忍窗口
- 跨 Realm 的权益门控
- 一次性权益限时（若未来需要，需重新评估决策）

### 2.3 依赖项

- 现有 RBAC 运行时（`require_permission` / role→权限映射）——不新建权限空间
- 现有 entitlement mapping / 支付尝试记录 / subscription 投影底座
- 现有 `billing-webhook-compensation` 框架（M4 复用其幂等键与补偿机制）

---

## 3. 需求概述

### 3.1 功能描述

Herald 当前付费履约硬绑积分：one-time 购买不配积分时履约直接报 500，recurring 却容错跳过——这种不一致使 Herald 无法支撑「付钱=解锁权益、不发积分」这种最常见的会员制/解锁制付费形态。本功能在不破坏「Herald 不管理 features」边界的前提下，把 role 授予做成一个横切叠加的配置维度，让任意购买形态都能「支付成功自动授权、过期自动撤销」，并让第三方应用直接用 Herald 现有 RBAC 一行判断即可解锁功能，无需自建门控逻辑。

### 3.2 关键特性

- **横切叠加模型**：role 授予是独立配置维度，与 billing_type（购买形态）、points 策略（是否发积分）三者正交，任何组合都可配置
- **一致性修复**：one-time 纯权益履约对齐 recurring 容错行为
- **支付驱动授权**：支付成功自动授 role；订阅过期/取消/退款自动撤 role（最终一致）
- **复用 RBAC**：不新建权限空间，Herald 仍是键值映射管道，不解释功能语义
- **来源可追溯**：支付授予与手工授予可区分，撤销时只撤支付来源

---

## 4. 业务规则与状态

### 4.1 业务规则

**横切叠加规则（核心模型）**：

| 组合 | 含义 | 重复购买 | role 撤销 |
|---|---|---|---|
| recurring + 积分 + role | 会员订阅（发积分+解锁） | 续费 webhook 续授 | 订阅过期/取消/退款撤 |
| recurring + 无积分 + role | 纯会员墙（只解锁） | 续费 | 订阅过期/取消/退款撤 |
| non_renewing + 可选积分 + role | 固定期限权益（不自动续费） | 受活跃订阅购买约束 | 到期/取消/退款撤销支付来源角色 |
| one_time + 积分 + 无 role | 积分包（现状） | 可重复买 | 不适用 |
| one_time + 无积分 + role | 纯永久权益墙（买断解锁） | 一人一次 | 退款/撤销时回收支付来源角色 |
| one_time + 积分 + role | 买断礼包（发积分+永久解锁） | 一人一次 | 退款/撤销时回收支付来源角色 |

- 三维度（billing_type / points 策略 / role 授予）各自独立，可为空；空 role 授予 = 纯积分/纯支付记录
- 不裂变 billing_type 枚举，不新增商品类型，不引入「积分包 vs 权益包」之类新名词

**履约一致性规则**：
- one-time 映射未配置 points_per_period 时，履约不报错、不发积分、记录支付尝试成功
- 该行为与 recurring 未配积分时跳过发放的容错一致
- 此规则为独立修复，与主体 role 授予能力正交

**role 授予规则**：
- role 来自用户在 Herald 自定义的角色/权限（复用现有 RBAC），不新建权限空间、不引入 `entitlement.*` 新权限格式
- 支付成功自动授予映射配置的 role
- 一次性购买（one_time + role）= 永久解锁，role 不设过期；退款或撤销时仅回收该笔支付来源的 role，正常取消或到期不触碰买断 role；`validity_days` 只约束积分有效期，不约束 role
- 订阅（recurring + role）= 周期内有效（靠 webhook 撤销 + 补偿框架最终一致，非 `expires_at` TTL 自动失效），续费 webhook 续授；`user_roles.expires_at` 仅保存支付周期来源信息，权限检查不按该字段过滤

**重复购买判定规则**：
- 仅「one_time + 授予 role」组合强制一人一次
- 购买前检查：是否已存在该 mapping 的成功购买记录，或用户已拥有对应 role
- 积分包（one_time + 无 role）保持可重复购买，recurring 续费不受限
- 并发安全：应用层购买前检查为 UX 快路径，DB 层在 `payment_attempts(user_id, target_id) WHERE status='Succeeded' AND is_one_time_role=TRUE` 上有 partial unique index 兜底，关闭并发双购窗口

**role 撤销规则**：
- 订阅的取消、过期或退款触发支付来源 role 撤销；一次性购买仅在退款或撤销时触发回收
- 撤销仅移除「支付授予」来源的 role 关联；Realm Admin 手工授予部分不受影响
- 撤销操作必须幂等（复用既有 webhook 幂等键）
- 一次性永久权益不会因正常取消或到期事件撤销；退款或撤销必须回收支付来源 role
- 撤销可靠性目标：最终一致，容忍窗口为分钟级，绝不永久漏撤；漏撤视为 P0 故障

**权限来源可追溯规则**：
- 支付授予与手工授予必须可区分
- 撤销、审计、查询场景下能识别 role 关联的来源（支付 vs 手工）

**RBAC 边界规则**：
- Herald 仅作为 entitlement→role→权限 的键值映射管道
- Herald 不存储、不解释 features/quotas 语义；权限语义由第三方应用定义
- 与既有「Herald 不管理 features」决策不冲突——Herald 仍不知权限语义

**权限规则**：

| 操作 | 需要权限 | 说明 |
|------|---------|------|
| 配置 entitlement mapping 的 role 授予维度 | `billing.manage` | Realm Admin |
| 查看 role 授予配置 | `billing.view` | Realm Admin |
| 支付成功自动授权 / 订阅撤销 | System Actor（webhook 处理） | 非 UI 操作 |
| 第三方应用凭 role 判断解锁 | 既有 RBAC 权限检查 | Third-party App |

### 4.2 关键状态与异常

**role 关联来源状态**（用于可追溯）：
- **支付授予（payment-granted）** — 因支付成功自动授予，可被 webhook 撤销
- **手工授予（manual）** — Realm Admin 手工授予，不受支付 webhook 影响

**异常场景**：
- one-time 不配积分的纯权益履约：修复前会报 500，修复后应记录成功、不发积分
- 并发双购 one_time+role：须保证至多一个成功（唯一约束/购买前检查）
- webhook 丢失/重复/乱序导致撤销：补偿框架介入，最终一致，不得永久漏撤（M4 风险核心）
- 撤销时用户已无该 role（如管理员已手工删除）：幂等跳过，不报错
- 撤销时 role 同时有手工与支付两个来源：仅撤支付来源，手工保留
- 乱序 webhook（cancel 后迟到 renewal）：续费路径须能重新授予 role，保证「订阅仍活则应有 role」

---

## 5. 功能需求

### 5.1 核心需求

**W1 — one-time 履约一致性修复**：
- billing_type=one_time 映射未配置 points_per_period 时，履约不报错、不发积分、记录支付尝试成功
- 行为与 recurring 未配积分时的容错一致

**M1 — role 授予横切维度**：
- entitlement mapping 新增「支付成功后授予哪些 role」配置
- 该维度与 billing_type、points 策略正交，可各自为空
- 支持为同一映射配置多个 role（一对多绑定）

**M2 — 支付成功自动授权**：
- 支付成功 webhook 触发自动授予映射配置的 role
- 一次性 = 永久解锁，role 不设过期
- 订阅 = 周期内有效，续费续授
- 支付授予与手工授予来源可追溯区分

**M3 — 一次性永久权益一人一次**：
- 仅「one_time + 授予 role」组合在购买前检查重复
- 命中已成功购买或已拥有 role → 拒绝创建支付尝试并提示
- 积分包（one_time + 无 role）不检查，保持可重复
- 并发安全，防双购

**M4 — 支付来源 role 撤销**：
- 订阅取消、过期和退款，以及一次性购买退款或撤销，都通过既有补偿能力处理
- 撤销仅移除支付来源的 role 关联，幂等
- 容忍窗口内最终一致（分钟级）

**M5 — 第三方应用 RBAC 判断**：
- 第三方应用直接用 Herald 现有 RBAC 运行时判断 role/权限，无需自建门控
- 不新建权限空间

### 5.2 验收目标

- one-time 纯权益购买履约不报错、不发积分、记录成功，与 recurring 容错一致（Wedge）
- 任意购买形态（one_time/recurring × 有无积分 × 有无 role）的 entitlement mapping 可配置并保存
- 支付成功后用户自动获得映射配置的 role；一次性为永久，订阅为周期内有效
- 「one_time + role」组合重复购买被阻止（含并发），积分包仍可重复
- 订阅过期/取消/退款，以及一次性购买退款/撤销时，支付来源的 role 被撤销，手工授予不受影响
- 撤销幂等，重复 webhook 不产生二次错误
- 第三方应用可凭 Herald RBAC 一行判断解锁，无需自建 entitlement 门控
- Herald 全程不存储/解释 features/quotas 语义

---

## 6. API 相关约束

**适用性**: 适用

**能力边界**：
- 不新建权限空间；role 授予复用现有 RBAC role/权限模型
- entitlement mapping 配置接口扩展「role 授予维度」配置能力（与既有积分策略配置同层）；创建端点可设置 `granted_role_ids`，后续修改走 batch 更新端点（多价格批量管理），single PATCH 不写该字段
- 支付成功 webhook 处理链路扩展：支付成功→授 role、订阅 canceled/expired/refund→撤 role
- 第三方应用查询/判断：复用既有 RBAC 权限检查能力，不新增 entitlement 专用门控接口

**访问控制与数据边界**：
- 所有配置/查询接口遵守 realm 隔离原则
- role 授予维度配置写入需 `billing.manage`；读取需 `billing.view`
- 自动授权/撤销由 System Actor（webhook 处理）执行，非用户 UI 操作
- 权限来源（支付/手工）必须可追溯，撤销时按来源隔离

**兼容性要求**：
- role 撤销链路必须复用既有 webhook 幂等键与补偿框架，不得另起
- role 授予维度须支持空映射（无 role 绑定时等同纯权益/纯积分包，行为不变）
- one-time 一致性修复须向后兼容现有积分包商品

---

## 7. 前端/交互约束

**适用性**: 适用

**Entitlement Mapping 配置界面**：
- role 授予维度作为独立配置区域，与积分策略区域并列、互不影响
- 支持选择用户自定义的 role（多选）
- 清空 role 授予保留积分策略 = 纯积分包；清空积分保留 role = 纯权益墙；两者皆空 = 仅记录支付

**购买流程**：
- one_time + role 商品：用户已拥有该 role 或已有成功购买时，购买按钮禁用并提示「已拥有该权益」
- one_time 积分包：保持现有可重复购买行为
- 支付成功后用户无需额外操作即获得 role

**状态反馈**：
- 一人一次拦截：后端返回结构化错误 `{ "code": "already_owned", "entitlementKey": <key> }`，前端据 code 自行渲染文案（示例：「You already own this item」）
- role 授予/撤销为系统自动行为，用户侧体现为功能可用性变化，无需显式提示（除非第三方应用自行展示）

**管理端可见性**：
- role 授予配置入口对拥有 `billing.manage` 的 Realm Admin 可见
- 权限来源（支付/手工）应在用户角色管理界面可查询/区分（便于排查撤销异常）

---

## 8. 已确认决策

- **横切叠加（核心模型）**：role 授予是独立配置维度，与 billing_type、points 策略正交；不裂变类型、不新增商品类型、不引入新名词
- **复用 RBAC**：映射到用户自定义 role/权限，不新建权限空间，不引入 `entitlement.*` 新权限格式
- **不破坏 features 边界**：Herald 仅做键值映射管道，仍不知权限语义；与既有「Herald 不管理 features」决策不冲突
- **一次性=永久解锁**：one_time + role 的 role 不设过期；`validity_days` 只约束积分有效期；不引入 cron/scheduler。退款或撤销仍须回收支付来源 role。
- **role 撤销边界**：订阅取消、过期和退款会撤销支付来源 role；一次性购买只在退款或撤销时回收，正常取消或到期不触碰买断权益。
- **webhook 撤销可靠性**：撤销不可靠 = 白嫖权益，是支付墙最致命失败模式；通过复用补偿框架 + 内部失败重试扫面达成与积分发放同等的幂等/补偿可靠性，漏撤视为 P0 故障
- **重复购买判定**：仅「one_time + 授予 role」强制一人一次；积分包可重复；recurring 续费不受限

---

## 9. 参考资料

- 用户故事：[docs/user-stories/billing/support-paywall.md](/docs/user-stories/billing/support-paywall.md)
- 相关 PRD：[docs/prd/billing/subscription.md](/docs/prd/billing/subscription.md)（订阅计费、Entitlement 映射、Webhook 处理）
- 相关 PRD：[docs/prd/billing/points.md](/docs/prd/billing/points.md)（积分系统、退款积分回收）
- 相关 PRD：[docs/prd/billing/subscription.md](/docs/prd/billing/subscription.md)（含多价格映射）
- 相关 PRD：[docs/prd/billing/subscription.md](/docs/prd/billing/subscription.md)（webhook 处理与补偿规则）
- 相关 PRD：[docs/prd/auth/permissions.md](/docs/prd/auth/permissions.md)（RBAC 权限管理）
- 角色定义：[docs/user-stories/_roles.md](/docs/user-stories/_roles.md)
