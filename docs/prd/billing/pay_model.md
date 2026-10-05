# 履约模型扩展：买断与非续期订阅

**创建时间**: 2026-07-29
**优先级**: P1

---

## 1. 相关用户故事

> 故事按商品形态分前缀：`US-BM`（买断 / one-time + role）、`US-NR`（非续期订阅 / non-renewing）。`US-BM-004` / `US-NR-004` 共用同一查询场景（故事 7），分别覆盖买断权益查询与非续期订阅查询。

| US-ID | 标题 | 优先级 | 来源 |
|-------|------|--------|------|
| US-BM-001 | 配置买断商品映射 | P0 | `docs/user-stories/billing/pay_model.md` |
| US-NR-001 | 配置非续期订阅映射 | P0 | `docs/user-stories/billing/pay_model.md` |
| US-NR-002 | 管理非续期订阅（区分计费类型与截止时间） | P1 | `docs/user-stories/billing/pay_model.md` |
| US-BM-002 | 购买卖断商品 | P0 | `docs/user-stories/billing/pay_model.md` |
| US-BM-003 | 恢复买断购买 | P0 | `docs/user-stories/billing/pay_model.md` |
| US-NR-003 | 购买非续期订阅 | P0 | `docs/user-stories/billing/pay_model.md` |
| US-BM-004 / US-NR-004 | 查询权益（买断与非续期订阅） | P1 | `docs/user-stories/billing/pay_model.md` |
| US-BM-005 | 退款或撤销买断（整笔退款/撤销回收支付来源角色） | P0 | `docs/user-stories/billing/pay_model.md` |
| US-NR-005 | 处理非续期订阅生命周期（到期失效、退款提前回收、Apple 到期缺口） | P0 | `docs/user-stories/billing/pay_model.md` |
| US-IAP-001～006 | IAP 渠道配置、凭证提交和对账基础能力 | — | `docs/user-stories/billing/support-iap.md` |
| US-PW-001～006 | 支付来源角色授予、撤销和重复购买边界 | — | `docs/user-stories/billing/support-paywall.md` |

---

## 2. 范围界定

### 2.1 包含功能

- IAP 非消耗型商品可作为买断销售：一次性购买后授予永久角色，支持恢复购买与退款/撤销回收。
- IAP 商品可配置为非续期订阅：购买即获得固定服务期，不自动续费。
- IAP 履约按消耗型积分包、买断和非续期订阅区分处理，不改变现有自动续期订阅与积分包的行为。
- 所有渠道的 `one_time + role` 购买在整笔退款或撤销时回收支付来源的永久角色；Stripe、Creem 为累计退款达到原支付金额时回收，部分退款保留角色（见 [refund-clawback.md](refund-clawback.md)）。
- 管理员可配置非续期订阅的服务期，并在订阅管理中识别其计费类型和截止时间。

### 2.2 不包含功能

- 不提供 Apple 非续期订阅的本地到期扫描或查询时强制失效。
- 买断权益不纳入基于 entitlement key 的订阅查询；第三方应用通过角色或权限判断买断解锁。
- 不扩展 Stripe、Creem 的商品形态；其既有一次性角色购买仍遵循本 PRD 的退款回收规则。
- 不提供 Herald 移动 SDK、商店商品管理 UI、佣金核算或 IAP 发票。
- 不包含 Google Play RTDN、跨平台订阅共享或家庭共享。

### 2.3 依赖项

- [IAP 支持](support-iap.md)提供的凭证验证、商店通知和 Google 对账能力（support-iap 首期已将买断与非续期订阅排除在范围外，本 feature 承接该扩展）。
- [支付驱动权益门控](support-paywall.md)提供的角色授予、来源追溯和重复购买控制。
- 既有订阅查询与管理能力。

---

## 4. 业务规则与状态

### 4.1 业务规则

- 买断使用一次性购买加永久角色授予表达；同一用户不得重复购买同一买断商品。
- 买断恢复购买与原始交易按同一交易标识去重，可补授缺失的永久角色，恢复同一交易不会重复发放权益。
- Google 买断商品只确认购买，不消耗购买；消耗仅适用于积分包。
- Apple 退款/撤销、Google 作废购买为整笔退款语义，回收该笔支付来源的角色；Stripe、Creem 的一次性退款在累计退款达到原支付金额时回收该笔支付来源的角色（部分退款保留，见 [refund-clawback.md](refund-clawback.md)）。重复事件不得造成二次错误。回收只影响支付来源，不影响管理员的手工授予。
- 非续期订阅必须配置服务期时长；购买成功即取得固定截止时间，不产生续费或后续扣款，到期后可再次购买。
- 非续期订阅不参与自动续期的状态流转；查询和管理视图须显示其计费类型与截止时间。
- Google 通过对账发现非续期订阅过期并使其失效；退款或撤销提前回收权益。

### 4.2 关键状态与异常

- 非续期订阅沿用有效与已失效状态，变更历史保留状态变化。
- Apple 不为非续期订阅提供服务端到期事件。无商店事件时，Herald 不以本地扫描或查询时判断强制失效；这是已接受的平台限制。
- 凭证校验失败、归属不符或商品未映射时，拒绝履约并给出明确原因。
- 履约、恢复补授与退款回收都必须幂等。

---

## 5. 验收目标

- 买断购买、恢复购买和整笔退款/撤销（Stripe、Creem 为累计全额退款）分别实现永久角色授予、幂等补授和支付来源角色回收。
- Google 买断购买不被消耗；现有消耗型积分包仍可被消耗。
- 未配置服务期时长的非续期订阅映射不能保存。
- 非续期订阅展示截止时间、不自动续费，并能由 Google 对账或退款事件进入失效状态。
- 自动续期订阅与消耗型积分包的既有履约、续费和退款行为不回退。

---

## 6. 边界与约束

**适用性**: 适用（API 与前端/交互边界合并陈述）

**API / 集成边界:**
- 复用既有 IAP 凭证提交与权益查询能力；履约按商品形态处理。
- 凭证提交需要用户身份，映射管理需要 billing/points 管理权限，第三方应用复用既有 SDK 与扩展查询能力。
- 映射、履约与回收均按 Realm 隔离。
- 非续期订阅在订阅查询中可识别；买断不进入订阅查询。

**前端 / 交互边界:**
- Entitlement 映射管理页提供非续期订阅选项，并在缺少服务期时长时阻止保存并说明原因。
- 买断沿用一次性购买加角色授予的配置方式。
- 订阅管理列表和详情区分自动续期与非续期订阅，并显示非续期订阅的截止时间。

---

## 7. 已确认决策

> 本节只收录当前有效的决策与未决问题，记取舍、理由、决策人与重开条件；规则正文只在 §4 定义，DEC/Q 编号保持稳定，供代码注释、测试与跨 PRD 引用追溯。

- **DEC-pay_model-001 · 买断存储**（user，2026-07-28 会话）：买断不新增 `BillingType::Buyout`，以 `one_time` + `granted_role_ids` 永久角色授予表达（角色 `expires_at=None`，已有防重复购买）；需补 IAP 非消耗型商品区分与退款时永久角色回收（规则正文见 §4.1）。理由：改动最小；现有 paywall 已落地 one_time+角色永久授予与 M3 防重复购买；接受"买断不进入 entitlement_key 订阅查询"的语义边界。落点：§2.1、§2.2、§4.1。重开条件：出现必须以 entitlement_key 查询买断权益的集成方需求。
- **DEC-pay_model-002 · 非续期模型**（user，2026-07-28 会话）：非续期订阅新增 `BillingType::NonRenewing`，复用 Subscription 实体——履约创建固定时长、不自动续费的订阅（创建即带 `current_period_end`/`cancel_at`，无续费流转）（规则正文见 §4.1）。理由：Subscription 实体字段与状态机（Active/Expired/has_access）足以表达；复用查询、SDK、管理视图，避免并联新实体。落点：§2.1、§4.1。重开条件：非续期与自动续期订阅的字段/状态冲突无法在对账层调和。
- **DEC-pay_model-003 · 到期策略**（user，2026-07-28 会话）：非续期订阅到期失效仅依赖商店通知/轮询驱动——不新增本地到期扫描 job，不在权益查询叠加本地时间判断；接受 Apple 非续期订阅无服务端生命周期（getAllSubscriptionStatuses 仅覆盖 auto-renewable）导致的到期缺口（规则正文见 §4.1、§4.2）。理由：用户明确选择；Google 轮询（subscriptionsv2.get）可发现 EXPIRED；Apple 侧缺口为已接受风险。落点：§2.2、§4.1、§4.2。重开条件：Apple 侧到期失效成为上线验收硬要求。
- **DEC-pay_model-004 · 退款回收渠道范围**（agent）：退款/撤销时回收永久角色的修正对全渠道（Apple/Google/Stripe/Creem）的 one_time+角色购买生效，不限定 IAP 渠道（规则正文见 §4.1）。理由：one_time 撤销路径为全渠道共享代码路径；DEC-pay_model-001 要求"退款时永久角色回收"未限定渠道；按渠道过滤反而增加复杂度；技术预研 §6.2 显式假设。落点：§2.1、§4.1。重开条件：需要把退款角色回收限定为 IAP 渠道。
- **DEC-pay_model-005 · 服务期时长字段**（agent，实现层）：非续期服务期时长使用新列 `provider_entitlement_mappings.service_duration_days`（INT NULL），不复用 `validity_days`；mapping 校验非续期必填，DB 加 CHECK 守卫。理由：`validity_days` 语义为 one_time 积分过期窗口（唯一消费点是 topup 过期），复用会造成按 billing_type 变化的语义重载（技术预研 §5.5 P2 风险）；新增可空列无兼容性损失；不改变产品语义（PRD 仅要求"服务期时长必填"）。落点：§4.1、设计/实现层（`.ai/design/pay_model.md` §4.3）。重开条件：出现必须复用 validity_days 的跨模型统一过期语义需求。
- **DEC-pay_model-006 · Google ack/consume 选择**（agent，实现层）：Google one_time 履约的 ack/consume 选择——mapping 仅配置积分发放（`points_per_period > 0` 且不授予 role）→ `consume`（消耗型积分包）；否则（角色-only 买断或积分+role 买断礼包）→ `acknowledge_product` only，保留 Google 侧恢复购买记录（修订：首版谓词为 `points_per_period > 0` 即 consume，积分+role 场景后改为 acknowledge）。理由：消耗语义与"权益可被再次购买"绑定于积分发放；角色-only 的 one_time 即买断形态，consume 会破坏恢复购买；`GoogleDeveloperClient.acknowledge_product` 已存在未用；不改变现有消耗型积分包行为。落点：§4.1、设计/实现层（`.ai/design/pay_model.md` §5.4）。重开条件：Google 侧出现"发积分但不可消耗"的商品形态。
- **DEC-pay_model-007 · billing_type 快照列**（agent，实现层）：`subscription` 表新增 `billing_type` 列（`'recurring'/'non_renewing'`），履约创建时从 mapping 快照；对账过滤、管理视图与 api-ext 查询均读该列。理由：US-PM-003/US-PM-007 要求订阅可识别计费类型；对账须按 billing_type 过滤（DEC-pay_model-002 架构约束）；经 entitlement_key join mapping 不可靠（mapping 可改可删）；快照列是订阅实体的固有属性。落点：§4.1、§6、设计/实现层（`.ai/design/pay_model.md` §4.3）。重开条件：订阅与 mapping 必须强一致且接受 join 成本。
- **DEC-pay_model-008 · 未上线兼容性**（user）：产品未上线，本特性不考虑向后兼容——迁移直接变更结构（不做 DEFAULT 回填/灰度/双写），DTO 字段自由增改，不为旧客户端保留解析安全；现有 recurring/one_time 行为不回退属回归要求而非兼容承诺。理由：用户明确指示（2026-07-28 `/t-design` 期间）。落点：§5、§6。重开条件：产品上线或已产生外部集成方存量数据。


---

## 8. 参考资料

- [IAP 支持](support-iap.md)
- [支付驱动权益门控](support-paywall.md)
- [订阅计费](subscription.md)
- 用户故事来源见 §1 表格
