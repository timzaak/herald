# 积分系统 产品需求文档 (PRD)

**创建时间**: 2026-03-13
**优先级**: P0

---

## 1. 相关用户故事

> 详细故事与验收标准请查看 `docs/user-stories/` 中对应文档。

| US-ID | 标题 | 优先级 | 来源 |
|-------|------|--------|------|
| US-AP-001～004 | 异步支付积分策略、提前发放、失败回收与未回收负债 | P0/P1 | `docs/user-stories/billing/async-payment-points.md` |
| US-PO-001 | 配置 Entitlement 积分策略 | P0 | `docs/user-stories/billing/points-admin.md` |
| US-PO-002 | 查看所有用户积分账户 | P1 | `docs/user-stories/billing/points-admin.md` |
| US-PO-003 | 查看用户积分交易历史 | P1 | `docs/user-stories/billing/points-admin.md` |
| US-PO-004 | 管理 Entitlement 积分策略 | P2 | `docs/user-stories/billing/points-admin.md` |
| US-PO-005 | 查看 Entitlement 充值引导 | P2 | `docs/user-stories/billing/points-admin.md` |
| US-PO-006 | 配置 Realm 默认积分策略 | P0 | `docs/user-stories/billing/points-admin.md` |
| US-PO-007 | 查看免费用户积分统计——不随 billing-statistics 首版交付（免费用户发放/转化漏斗口径，见 `docs/prd/billing/billing-statistics.md` §2.2 与 Q-billing-statistics-003） | P1 | `docs/user-stories/billing/points-admin.md` |
| US-PO-008 | 主动发放积分 | P0 | `docs/user-stories/billing/points-admin.md` |
| US-PO-009 | 配置多时间窗滚动配额 | P0 | `docs/user-stories/billing/points-admin.md` |
| US-PU-001 | 查看我的积分余额 | P0 | `docs/user-stories/billing/points-user.md` |
| US-PU-002 | 查看我的交易历史 | P1 | `docs/user-stories/billing/points-user.md` |
| US-PU-003 | 筛选交易记录 | P2 | `docs/user-stories/billing/points-user.md` |
| US-PU-009 | 按时使用本期积分（不受分发延迟影响）——由 US-PU-010 取代 | P0 | `docs/user-stories/billing/points-user.md` |
| US-PU-010 | 滚动窗口额度与充值余额的可用性体验 | P0 | `docs/user-stories/billing/points-user.md` |
| US-FU-001 | 注册时获得初始积分（永久有效） | P0 | `docs/user-stories/billing/points-free-user.md` |
| US-FU-002 | 定期自动获得免费积分（支持 once/daily/weekly/monthly） | P0 | `docs/user-stories/billing/points-free-user.md` |
| US-FU-003 | 升级到付费套餐时保留注册初始积分 | P1 | `docs/user-stories/billing/points-free-user.md` |
| US-FU-004 | 按时获得每期免费积分（不受分发延迟影响）——由 US-FU-005 取代 | P0 | `docs/user-stories/billing/points-free-user.md` |
| US-FU-005 | 免费周期积分改为滚动窗口配额 | P0 | `docs/user-stories/billing/points-free-user.md` |
| US-PU-006 | 购买积分包 | P0 | `docs/user-stories/billing/points-package-purchase.md` |
| US-PU-007 | 查看积分包购买记录 | P1 | `docs/user-stories/billing/points-package-purchase.md` |
| US-PU-008 | 理解积分包与订阅购买的区别 | P1 | `docs/user-stories/billing/points-package-purchase.md` |
| US-TP-017 | 通过 SDK 发放积分 | P0 | `docs/user-stories/integration/sdk.md` |

> 注：本地积分包管理（US-PP-001~005）和促销积分包（US-PP-006, US-PP-016~018）已由支付平台产品管理 + Entitlement 映射取代。

---

## 2. 范围界定

### 2.1 包含功能

- **积分池组织单位**：积分池按积分账户组织，每个用户对每个持有的积分账户拥有独立积分池（`user × bucket`），已替换单一钱包模型；积分账户目录、覆盖集、归属、跨池消费与履约路由的完整规则见 `docs/prd/billing/credit-bucket.md`。本 PRD 描述积分类型、过期、消费优先级、退款回收等积分核心规则，其"池归属"维度以积分账户为准
- **积分分发规则（`points_distribution_rules`）**：注册、免费周期、订阅等自动发放不再各自只把一次触发路由到单一账户和一组积分策略，而是由统一的 `points_distribution_rules` 承载。一次触发可命中多条规则，每条规则指定一个目标积分账户和发放策略（fixed 周期积分 **或** 滚动窗口 quota），向多个账户扇出发放。订阅/免费周期规则的发放策略可选 fixed 周期积分或滚动窗口 quota（`DEC-multi-wallet-grant-rules-011`）；本文其余"滑动窗口配额"描述指的是 quota 策略本身的能力。详见 `docs/prd/billing/multi-wallet-grant-rules.md` §4/§7
- 积分账户管理（创建、查询）
- 积分余额查询
- 积分消耗/扣除（SDK API）
- 积分充值（套餐兑换积分）
- 积分交易历史记录
- 积分套餐配置（套餐与积分兑换比例）
- 前端积分管理页面
- 前端积分充值页面
- 积分 SDK（供第三方应用调用消耗积分）
- 积分类型分治：充值/注册/发放积分保持池子模型；订阅/免费周期积分采用 usage-based 滑动窗口配额模型
- 多时间窗滚动配额：同一权益可叠加多个时间窗（如 5 小时/周/月）的额度上限，可用额度取各窗口剩余最小值
- 懒发放（限 quota 滑动窗口策略）：订阅/免费周期的 quota 额度不再后台全表预发，按用户访问/消费时刻实时计算；新用户首访即得，不活跃用户零后台开销。fixed 周期积分策略（DEC-multi-wallet-grant-rules-011 引入）不在此列——由后台 worker 按 `NextGrantTime` 排期预发，扫描与写入不看用户活跃状态
- 混合消费协调：单次消费在事务内先用窗口额度，超额原子转充值/注册/发放池，不足整体拒绝
- 配额权益授予与撤销：订阅激活/续费授予配额权益，取消/退款/升级撤销配额权益；已消费量随窗口滚动自然释放，不反向调整
- 积分有效期管理：支持充值/注册/发放积分的过期时间配置（永久有效/N天有效）
- 免费用户积分系统：注册初始积分（池子）、免费周期积分（滚动窗口配额）
- Realm 默认配置：管理员配置免费周期积分策略（多时间窗滚动配额）
- 注册初始积分：用户注册时自动获得一次性积分（永久有效）
- 免费用户升级：免费用户升级到付费套餐时撤销免费窗口额度，注册初始积分保留
- 一次性积分购买：用户通过 one-time entitlement mapping 产品购买充值积分，不创建订阅
- 管理员主动发放积分：向指定用户发放指定数量的积分，附带发放原因和可选有效期
- SDK 发放积分：第三方应用通过 SDK 向用户发放积分，附带原因和可选有效期

### 2.2 不包含功能 (Out of Scope)

- 积分转账/赠送（暂不包含用户间积分转移）
- 积分提现（积分仅供内部消费，不支持提现为现金）
- 积分等级/会员系统（首版不实现积分等级体系）
- 积分商城（商城系统不在本次实现范围内）
- 免费用户间积分转账（免费积分仅供个人使用）
- 本地积分包管理（已由支付平台产品管理 + Entitlement 映射取代）
- 折扣码/优惠码系统
- 充值积分（topup_credit）、注册积分（registration_credit）、发放积分（granted_credit）模型变更
- 独立用量账本：窗口用量复用既有消费流水聚合
- 独立限流中间件：配额属于积分业务语义，与积分账本同库同事务
- 高频热点下的窗口用量物化计数器（默认精确聚合，性能优化不纳入产品需求）

### 2.3 依赖项

- **billing (订阅计费)**：需要与套餐系统打通，支持通过购买套餐获取积分；影响积分充值、套餐配置、订阅生命周期事件
- **用户注册系统**：需要在用户注册时触发积分发放；影响注册初始积分与免费周期额度权益
- **积分系统核心**：复用积分账本、交易记录等核心功能；影响积分发放、查询、消费
- **credit-bucket**：窗口配额按 `(user × bucket)` 维度生效与聚合，配额定义挂 entitlement mapping / realm default config，不挂 bucket（见 `docs/prd/billing/credit-bucket.md`）
- **一次性积分购买**：依赖 EntitlementMapping 和 PaymentAttempt（见 `docs/prd/billing/subscription.md`）
- **Webhook 补偿**：订阅生命周期事件驱动的配额权益授予/撤销沿用订阅计费 PRD 中的 webhook 幂等键与补偿规则（见 `docs/prd/billing/subscription.md`）

---

## 4. 业务规则与状态

### 4.1 业务规则

**积分账户**：
- 积分池按积分账户组织，每个用户对每个持有的积分账户拥有独立积分池，余额以整数存储，单位固定为 points（池组织与覆盖/路由见 `docs/prd/billing/credit-bucket.md`）
- 每个 `(user, bucket)` 池相互独立，支持 realm 级别隔离
- 积分钱包按 `(用户, 积分账户)` 懒创建：用户创建时不预建，首次对该账户发放或消费时在事务内确保钱包存在（多钱包模型，支持多租户隔离）
- 积分账户和余额接口返回 unit = "points" 表示余额单位
- 支持账户状态管理（正常/冻结/关闭）：持 `points.manage` 的管理员可按 `(user, bucket)` 更新钱包状态；冻结或关闭后发放与消费均拒绝

**积分类型与计费模型**：

| 积分类型 | 计费模型 | 说明 |
|---|---|---|
| subscription_credit（订阅积分） | usage-based 滑动窗口配额 | 可用额度按消费时刻实时计算，不预写整期额度 |
| free_periodic_credit（免费周期积分） | usage-based 滑动窗口配额 | 可用额度按消费时刻实时计算，不累积 |
| topup_credit（充值积分） | 池子模型 | 用户主动购买获得，长期有效（除非产品配置特殊规则），可用于所有消费场景 |
| registration_credit（注册初始积分） | 池子模型（永久有效） | 用户注册时自动获得，升级到付费套餐后保留，每个用户只能获得一次 |
| granted_credit（发放积分） | 池子模型 | 由管理员或 SDK 主动发放获得，可配置有效期，遵循基于过期时间的消费优先级规则 |

**多时间窗滚动配额语义**：
- 一个配额定义由若干"时间窗 × 配额上限"组成（如 `{5小时: 500, 周: 5000, 月: 20000}`）；单个配额定义最多配置 8 个时间窗（保存时校验拒绝超出，实现约束）
- 任一时刻某权益的**可用额度** = 各窗口"配额上限 − 窗口内已用量"取**最小值**（最严约束生效）
- 窗口为**滑动/滚动语义**（按消费发生时间精确滑动），非整点固定窗
- 窗口配额用量来自该 `(user, bucket, credit_type)` 在窗口时间区间内的消费流水聚合（金额累加）
- 多窗口叠加时"不累积"语义由窗口滚动天然表达：用量随时间滑出窗口即释放额度，无需独立的"周期清零"动作
- 订阅"周期结束清零"等价于月窗配额的滚动：原订阅周期额度即月窗配额，窗口滑出即等价清零

**懒发放正确性边界**：
- 可用额度计算**不依赖后台 job**：用量来自 append-only 的不可变消费流水，任意时刻重算结果一致
- 后台 job 仅做周期锚点推进与配额权益过期清理，**不做正确性兜底**；即使后台调度未运行，读路径仍能正确计算窗口额度
- 后台调度**不再**对订阅/免费周期积分执行全表预发

**配额权益授予与撤销（取代发/收 ledger 行）**：
- 订阅激活/续费：**授予**配额权益（记录权益生效区间与周期锚点），不再逐期写 ledger
- 订阅取消（周期结束/立即）/退款：**撤销**配额权益；已消费量随窗口滚动自然释放，**不反向调整**消费流水、不补扣
- 订阅升级：撤销旧套餐配额权益、授予新套餐配额权益
- 免费周期：注册/加入时**授予**配额权益；升级到付费套餐时**撤销**免费配额权益
- 授予/撤销沿用既有业务幂等键（按周期锚点/webhook 幂等键），重复事件不重复授予或撤销

**混合消费协调**：
- 单次消费在同一事务内原子完成：先尝试扣减窗口额度（subscription/free_periodic），不足部分转充值池（topup/registration/granted，沿用既有池子消费选取与过期优先规则）扣减
- 窗口额度 + 充值池合计不足以覆盖本次消费时，**整体拒绝**，不允许部分扣减导致超扣或总额不一致
- 跨 `user × bucket` 多池消费沿用 credit-bucket 覆盖集规则
- 充值/注册/发放积分的消费优先级、退款回收、过期规则**沿用既有池子规则不变**

**积分消费优先级（池子类型）**：
- 按过期时间优先消费即将过期的积分（expires_at 升序，NULL 排最后表示永久有效）
- 不需要调用方指定消费哪种类型的积分，系统根据可用余额自动计算消费分摊
- 单次消费必须原子性的完成跨类型的积分扣减，不允许部分扣减导致数据不一致
- 每次 SDK 调用记录交易历史（包含交易 ID、用户 ID、Client App ID、消耗数量、时间戳、说明）

**查询与展示**：
- 用户查询余额时，对走窗口模型的权益须展示**各窗口剩余额度**与**充值余额**，并提供合计可用
- 余额反映"当前时刻可消费额度"：窗口剩余取各窗口 min，叠加充值余额
- 管理员查询账户时，区分展示窗口配额权益（生效区间/窗口配额）与池子余额明细
- 权限隔离：用户只能查询自己的记录，管理员可查询全租户记录
- 交易历史查询支持按时间范围、交易类型、用户 ID（仅管理员）、Client App ID 筛选，分页查询
- ext API 查询单笔交易：支持通过 external_ref_id 查询单笔交易详情

**配置约束**：
- 配额定义中的窗口长度须为正、配额上限须非负
- 配额定义归属 entitlement mapping（订阅）与 realm default config（免费周期），**不在账户上**配置
- 修改配置仅影响后续授予的配额权益，不影响已授予权益的生效区间

**积分发放规则**：
- 订阅首次赠送：授予 subscription_credit 配额权益
- 订阅续费：授予 subscription_credit 配额权益
- 订阅升级：撤销旧套餐 subscription_credit 配额权益，授予新套餐 subscription_credit 配额权益
- 直接充值购买：发放 topup_credit
- 用户注册：发放 registration_credit，并授予 free_periodic_credit 配额权益。覆盖所有自助注册入口——密码注册（含邮箱验证确认）、邮箱验证码自动注册、LDAP JIT 开通、OAuth 首次登录建号；以 `registration:{user_id}` 幂等键保证每用户仅一次。管理员建号与平台开通建号（初始 Realm 管理员）不发注册积分
- 免费用户：按 realm default 的多时间窗滚动配额授予 free_periodic_credit 配额权益
- Realm 注册/免费周期发放配置由注册分发规则（`points_distribution_rules`，`owner_type=realm_registration`）承载（见 multi-wallet-grant-rules.md）；新 Realm 不自动创建默认规则——无启用规则时注册正常完成、不发积分，管理员通过 registration-rules 端点显式配置注册初始积分与免费周期配额

**退款积分回收**：
- 充值退款（topup_credit 退款）：按未使用比例回收 topup_credit（proportional revocation），已使用部分不回收
- 会员退款（subscription_credit 退款）：撤销 subscription_credit 配额权益，已消费量不反向调整

**订阅升级**：
- 升级立即生效，撤销旧套餐 subscription_credit 配额权益，授予新套餐 subscription_credit 配额权益
- 注册初始积分（registration_credit）保留，不受影响

**订阅降级**：
- 降级下周期生效，不回收当前周期已授予配额权益
- 当前周期继续享受原套餐窗口额度，下周期按新套餐配额权益生效

**订阅取消**：
- 默认取消（周期结束）：当前周期配额权益继续有效，周期结束后自然过期
- 立即取消：撤销 subscription_credit 配额权益；已消费量不反向调整

**免费用户周期积分**：
- 按 realm default 配置的多时间窗滚动配额授予 free_periodic_credit 配额权益
- 不累积：用量随窗口滚动自然释放，未使用额度不跨窗口累积
- 升级到付费套餐后撤销 free_periodic_credit 配额权益

**免费用户升级到付费套餐**：
- 保留注册初始积分（永久有效，不受订阅变更影响）
- 撤销 free_periodic_credit 配额权益
- 根据付费套餐配置授予 subscription_credit 配额权益

**发放规则（granted_credit）**：
- 积分数量必须大于 0
- 发放原因为必填项
- 有效期可选：指定天数（> 0）或永久有效（不设置）
- 管理员只能向本 Realm 内用户发放积分（管理员发放接口需 `points.manage` 权限）
- SDK 发放受 API Key 权限控制，遵循 Realm 隔离
- 发放操作生成交易记录（类型为 grant），包含操作者标识（管理员身份或 API Key / Client App 标识）

**防止滥用**：
- 一次性注册积分以分发事件幂等键 `registration:{user_id}` 去重（`points_distribution_events` 唯一约束），同一用户的注册积分只发放一次，跨全部自助注册入口（邮箱验证、邮箱 OTP 自动注册、LDAP JIT、OAuth 首登、经典注册）生效
- 记录所有积分发放历史，便于审计

**Webhook 事件处理**：
- subscription.paid：授予 subscription_credit 配额权益
- subscription.update：处理升级/降级配额权益
- subscription.canceled：撤销 subscription_credit 配额权益
- refund.created：撤销 subscription_credit 配额权益（如适用）
- 所有事件处理需保证幂等性

**积分过期机制**：
- subscription_credit / free_periodic_credit：额度随滚动窗口滑出自然释放，不再单独设置 per-period expires_at
- topup_credit：默认长期有效（expires_at 为 null），产品可配置有效期（可选）
- registration_credit：永久有效
- granted_credit：按配置有效期过期
- 用户可查看即将过期的池子类型积分

**Product 兼容约束**：
- 本地 Product/Plan 已废弃，不作为积分配置对象
- 当前正式配置对象是 `entitlement_key`
- 积分策略字段保存在 provider entitlement mapping 上，除历史池子字段外新增多时间窗滚动配额配置
- Realm 默认配置保存免费周期的多时间窗滚动配额配置
- Product 级默认规则、按 Client App 差异化规则、规则优先级覆盖等不在当前范围

### 4.2 关键状态与异常

**异步支付积分策略**：

- Realm 可选择保守策略（默认）或积极策略；未配置支付平台时不展示该配置。
- 保守策略仅在异步支付确认成功后发放积分；支付失败时因未发放而无需回收。
- 积极策略可在 checkout 完成但款项未确认时提前发放，复用同步支付的积分履约，并记录本次支付是否已经发放；后续成功事件必须幂等，不得重复发放。
- 积极策略下支付最终失败时，按原发放记录回收该次发放中尚未消费的积分并记录交易，同时更新支付尝试及订阅状态。
- 回收不得使余额低于零；不足部分以带 `debt:` 原因的账本记录保留，供管理员查询与线下对账。当前没有独立的负债结清状态，因此不会据此自动永久冻结后续使用。
- 策略修改只影响后续支付，不追溯改变已经发放的积分。

- **积分不足**：SDK 调用消耗积分时余额不足，返回明确错误；混合消费下窗口额度与充值余额合计不足时整体拒绝，不部分扣减
- **窗口额度耗尽但充值余额充足**：单次消费超额部分原子转充值池扣减，用户无感切换
- **周期中途新增用户**：首访/首消费即得窗口额度，不依赖后台预发或上一期事件链
- **不活跃用户（限 quota 策略）**：无后台预发写入、无回收调度；fixed 周期规则的排期发放仍会扫描并写入（见 §2.1 懒发放限定）
- **账户冻结/关闭**：账户状态异常时，积分操作受限
- **重复发放**：注册初始积分基于 user_id 去重，Webhook 事件与配额权益授予基于幂等键去重
- **部分失败**：异步任务失败时，通过补偿接口退回已消费积分（使用 external_ref_id 关联原始消费交易），补偿积分继承原积分类型和过期时间。当前状态：该补偿调用路径尚未提供（无端点、无 job、无内部调用方）；现行部分失败恢复依赖支付事件幂等键 + 重试任务的履约重放，以及积极策略失败回收的负债记录（`debt:` 原因账本）
- **积分过期通知**：未来通知系统可在池子类型积分过期前 7/3/1 天发送提醒；当前仅提供到期时间查询与页面展示，未实现主动通知任务

---

## 5. 验收目标

- 保守策略下，未确认的异步支付不会提前增加积分；积极策略下积分可立即使用。
- 积极策略的成功确认不重复发放，最终失败能回收未消费积分并记录不足部分负债。

- 用户注册后自动获得初始积分（如果 Realm 配置启用），积分永久有效且不可重复获取
- 周期中途新增的订阅/免费用户，在首次查询余额或首次消费时即获得窗口额度，不依赖后台预发
- 不活跃用户在后台调度中不产生 quota 预发写入与回收（fixed 周期规则的排期发放除外，见 §2.1 懒发放限定）
- 多时间窗叠加时可用额度取各窗口剩余最小值；某窗口滑出后对应额度恢复
- 滑动窗口按消费时间精确滑动，非整点固定窗
- SDK 调用能正确消耗积分，窗口额度优先，超额原子转充值池；合计不足整体拒绝
- 订阅支付成功后正确授予 subscription_credit 配额权益
- 订阅升级/降级/取消时配额权益处理符合业务规则
- 退款时撤销 subscription_credit 配额权益，已消费量不反向调整
- 管理员能查看全租户积分数据，普通用户仅能查看自己的数据
- 免费用户升级到付费套餐时注册初始积分保留，免费窗口额度停止
- 积分过期机制正常工作（池子类型）
- 管理员能在管理后台向指定用户成功发放积分，积分正确到账
- SDK 能通过 API 向指定用户发放积分，返回发放结果
- 发放的积分类型为 granted_credit，在用户余额中按类型正确显示
- 发放交易的消费优先级遵循现有过期时间优先规则
- 所有发放操作生成可追溯的交易记录，包含发放原因
- 发放时跨 Realm 用户、数量无效、缺少原因等场景正确拒绝
- 用户余额页能看到窗口剩余额度（按窗口）与充值余额及合计

---

## 6. 边界与约束

**适用性**: 适用（API 与前端/交互边界合并陈述）

**API / 集成边界:**
- 接口能力范围包括：积分账户查询与管理员钱包状态更新（含窗口剩余 + 充值余额双维度）、积分消费类（SDK，含混合消费协调）、积分充值/发放类、交易历史查询类、Entitlement Mapping 积分策略配置类（随 mapping 管理的 `points_distribution_rules`）、Realm 注册积分分发规则管理类（`registration-rules`，`owner_type=realm_registration`）、Webhook 回调处理类
- 内部直写端点（demo/test-only）：`POST /api/internal/points/{realmId}/quota-entitlement/{grant,revoke}` 绕过用户认证与 `points_distribution_rules`，直接构造/撤销 `PointsQuotaEntitlement`（复刻 webhook 路径产物，供快速 demo/E2E 使用）。该端点仅由 `X-Internal-API-Key`（`INTERNAL_API_KEY` 密钥）防护、fail-closed，不进入 OpenAPI/SDK；生产部署不得配置该密钥，否则构成绕过分发规则的发放入口
- 访问控制：SDK 消耗接口需 API Key 授权（ThirdParty 身份）；管理类接口需 Realm Admin 权限；用户查询类接口仅允许查询本人数据
- SDK 消耗积分时校验 API Key 对 client_app 的作用域（client_app_scope），确保 API Key 只能操作其授权范围内的 client_app 积分
- API Key 鉴权实时校验其绑定 Client App 的启用状态（包括缓存命中路径）：Client App 被禁用后，其 API Key 立即失效并返回 401，不依赖缓存 TTL 过期
- 限流策略（生效范围：SDK ext 消费与发放两点）：realm 级别与 user 级别双层限流，阈值为后端运行常量；api-points 管理端点当前不设独立限流
- SDK ext 消费幂等（`idempotencyKey`）：键上限 255 字节（超限 400，防共享 Redis 的持久键名膨胀）；同键同负载重放返回首次结果；同键异负载 409 `idempotency_conflict`；状态标记丢失（在途 TTL 到期/写入失败/完成前崩溃）时失败关闭为 409（先前排键重放会二次扣减）；缓存记录比请求指纹存活更久（完成滞后超出在途时域）同样 409——ext consume 路由（仅该路由，非 ext 整体）有请求时长上限，正常路径不可能进入该状态
- 管理接口权限：所有管理端点经灵活认证中间件认证后，再经 admin-console 凭据闸门（仅第一方 admin-web-console Bearer token 可通过，API Key 与第三方 Bearer 一律 403；第三方 API Key 走 `/api/ext/points/*`），最后在 handler 内以 `require_authenticated_user_in_realm`（Realm 归属校验）+ 权限校验进行控制：
  - 积分数据查询（wallets、transactions）：`points.manage`。`points.view` 授权用户本人数据查询（经用户自查端点）；管理端跨用户查询 wallets/transactions 需 `points.manage`（内置 user 角色持有 `points.view`，若管理端仅要求 view 会导致普通用户跨用户读取积分数据）
  - Entitlement Mapping 的积分分发规则（随 mapping 的 `point_rules`）：随 mapping CRUD，`billing.manage`（带 `point_rules` 时额外要 `points.manage`）
  - Realm 注册积分分发规则（`registration-rules`，`owner_type=realm_registration`）：读操作 `points.view`，写操作 `points.manage`
- 钱包状态更新端点路径无 realm 段，realm 由 admin 会话钉定；目标用户、账户与调用身份均按 Realm 校验
- 积分变更必须可追溯，所有发放、消费、回收操作创建交易记录
- Realm 隔离：所有接口严格遵守 realm 数据边界，防止跨 realm 操作

**前端 / 交互边界:**
- 管理入口：租户管理员可在管理后台访问 Entitlement Mapping 积分策略配置（随 mapping 的分发规则）、积分报表、Realm 注册积分分发规则管理页面
- 用户入口：用户可在个人中心查看积分余额（窗口剩余 + 充值余额双展示）和交易历史
- 积分充值页面：展示套餐兑换积分的比例和预期获得积分数
- 套餐/积分策略配置页：积分策略支持配置多时间窗滚动配额（窗口长度 × 上限的集合）
- Realm 注册积分分发规则页：注册与免费周期发放按 `points_distribution_rules`（`owner_type=realm_registration`）配置，每条规则指定目标账户和发放策略（fixed 周期积分或滚动窗口 quota），支持多时间窗滚动配额
- 交易历史：支持按时间范围、交易类型、来源应用筛选；"发放"类型记录可按现有筛选规则查看
- 状态反馈：积分变更时（发放、消费、过期、回收、配额权益授予/撤销）提供明确的状态提示
- 免费用户积分：展示窗口剩余额度与恢复时间
- 积分发放入口：在积分管理页面或用户积分账户详情页提供"发放积分"按钮
- 发放表单字段：用户选择（下拉搜索）、积分数量（正整数）、有效期（天数输入或"永久有效"选项）、发放原因（文本输入）
- 发放确认：提交前显示确认弹窗，包含发放摘要（用户、数量、有效期），需管理员二次确认
- 配置校验与反馈：多窗口配额配置提供合理性校验（窗口长度为正、上限非负），非法配置给出明确提示
- 异步支付积分策略：管理端可查看与切换策略，选择时展示提前发放的资金风险提示
- 金额/积分变化场景必须突出变化量、变更影响范围和不可逆风险提示

---

## 7. 已确认决策

> 发放策略由 `points_distribution_rules` 承载，每条规则按 owner（entitlement mapping / realm registration）× trigger × policy（fixed 或 quota）路由到目标账户，一次触发可多账户扇出（`DEC-multi-wallet-grant-rules-006/011`，规则正文与完整取舍见 `docs/prd/billing/multi-wallet-grant-rules.md` §4/§7）。

- 积分余额单位固定为 points，不使用法币 currency 表示
- 计费模型分治：subscription_credit / free_periodic_credit 的发放由 `points_distribution_rules` 承载，策略可选 fixed 周期积分或滚动窗口 quota（`DEC-multi-wallet-grant-rules-011`）；topup_credit / registration_credit / granted_credit 维持池子模型不变
- 积分消费优先级采用过期时间优先策略（池子类型），窗口额度优先于池子扣减
- 免费用户积分系统独立于订阅系统，不需要创建 $0 订阅记录
- 当前正式配置对象是 `entitlement_key`，不再使用 Plan 级配置
- 懒发放：取消后台全表预发，可用额度在读/消费路径按需计算
- 订阅生命周期回收语义从"回收 ledger 行"改为"撤销配额权益"，已消费量不反向调整
- 配额（quota）定义归属发放规则的 owner（entitlement mapping / realm registration），不挂积分账户
- 窗口用量复用既有消费流水聚合，不另建独立用量账本
- 积分补偿使用现有 grant_points_internal 方法，无需新增 API（补偿路径定位为内部调用；当前尚无调用方，见 §4.2 部分失败条目的现状标注）
- 价格使用最小货币单位（分）存储，避免浮点精度问题
- 不创建独立的活动/活动实体，积分发放直接附带原因
- SDK 发放方法与现有 SDK 风格一致
- granted_credit 不因类型获得消费优先级特殊处理，沿用过期时间优先规则

---

## 8. 参考资料

- 用户故事来源见 §1 表格；另有用户故事：`docs/user-stories/billing/credit-bucket.md`
- 相关 PRD：`docs/prd/billing/subscription.md`
- 相关 PRD：`docs/prd/billing/credit-bucket.md`
- 需求来源：`.ai/future/points_plus.md`
