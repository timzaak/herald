# 多货币（按货币选择/本地化）产品需求文档 (PRD)

**创建时间**: 2026-08-15
**优先级**: P1

> 场景背景：多货币（按货币选择/本地化）建立在 Herald 已支持的「一产品多货币多价格」目录能力之上，为 Stripe 多 Price 产品叠加购买页货币分组展示（显式选择，无默认货币）与第三方货币查询能力。本文档不承载接口端点、请求/响应 schema、HTTP 状态码、数据库建表/迁移或代码类型定义；技术方案细节请参见对应技术设计。
>
> 2026-08-15 变更（DEC-multiple_currency-014）：废除 Realm 默认货币与用户偏好货币两级偏好（原 DEC-004/006/007/011）。货币必须由用户显式选定，系统不做任何默认/回退。

---

## 1. 相关用户故事

> 详细故事与验收标准请查看 `docs/user-stories/` 中对应文档。

| US-ID | 标题 | 优先级 | 来源 |
|-------|------|--------|------|
| US-MC-003 | 购买页按货币分组、显式选择货币（无默认） | P0 | `docs/user-stories/billing/multiple-currency.md` |
| US-MC-004 | 按（显式选定的）货币价格行发起购买 | P0 | `docs/user-stories/billing/multiple-currency.md` |
| US-MC-005 | 查询可购权益支持的货币集合 | P0 | `docs/user-stories/billing/multiple-currency.md` |
| US-MC-006 | Creem / IAP / WeChat Pay 单一价格降级展示 | P2 | `docs/user-stories/billing/multiple-currency.md` |
| US-EM-007～009 | 多价格同步配置、Webhook 解析与指定价格购买（多价格基线，本特性复用） | P0 | `docs/user-stories/billing/entitlement-mapping.md` |
| US-BL-SYNC-001～004 | 产品名、价格单位与计费周期同步展示（多价格基线，本特性复用） | P0 | `docs/user-stories/billing/entitlement-mapping.md` |
| US-PA-001～004 | 创建支付尝试、查询状态、成功后履约、关闭过期（统一支付尝试与履约，本特性复用） | P0/P1 | `docs/user-stories/billing/payment-attempt.md` |

> 注：原 `[US-MC-001]`（配置 Realm 默认货币）与 `[US-MC-002]`（个人偏好货币覆盖）随 DEC-014 一并废除。

---

## 2. 范围界定

### 2.1 包含功能

- **按货币解析价格行**：在购买/默认解析时按 (产品/权益 + 计费维度 + 货币) 反查启用的 Stripe 多 Price 映射行；货币为过滤维度，非唯一键（DEC-multiple_currency-005）
- **显式货币选择**：购买页展示某权益全部已配置货币，用户显式选择其一后才渲染价格行；无默认货币、无偏好、无回退链（DEC-multiple_currency-014）
- **购买页分组展示**：对 Stripe 多 Price 产品按货币分组、提供货币切换；同货币下多计费周期/类型并存时由用户在货币组内选择
- **api-ext 货币暴露**：对每个可购权益暴露其支持的货币集合；订阅类与一次性购买类映射均暴露货币，供 SDK/第三方应用做货币切换与按货币解析
- **渠道降级展示**：对 provider/store 侧定价渠道（Creem、IAP、WeChat Pay），购买页降级为单一价格展示，不渲染货币切换器（DEC-multiple_currency-003）

### 2.2 不包含功能 (Out of Scope)

- **改用 Stripe 单 Price `currency_options` 范式**：货币选择层建立在现有「一 Price 一映射行」目录之上，不读取 `currency_options`、不改同步逻辑（DEC-multiple_currency-002）
- **provider 侧定价渠道的货币解析**：Creem 产品级单一价格、IAP 由商店按地区定价、WeChat Pay 由商户在渠道侧定价，均不纳入 Herald 货币解析（DEC-multiple_currency-003）
- **默认/偏好货币**：不设 Realm 默认货币、用户偏好货币或任何回退链（DEC-multiple_currency-014）
- **汇率换算 / 自动本地化定价**：Herald 不做货币换算；Stripe Adaptive Pricing 若启用属商店侧行为，Herald 只引用真实 Price（见 §4.2 已知展示局限）
- **provider 侧目录同步模型改造**：多货币多价格的目录能力已就绪，不新增 Product/Price schema、不改同步（DEC-multiple_currency-002）
- **促销 / 折扣 / 价格实验**：沿用既有「促销委托支付平台」边界，不在本期扩展
- **退款 / 订阅状态的货币相关改造**：退款与积分回收沿用既有模型，不因多货币改变

### 2.3 依赖项

- **既有 Billing 目录与同步**：`provider_entitlement_mappings`（一 Price 一行）与 provider 产品同步是货币解析的数据基线（`docs/prd/billing/subscription.md`）
- **既有统一支付尝试与履约**：货币解析只决定「选哪一行映射」，Checkout 构造与履约链路不变（`docs/user-stories/billing/payment-attempt.md`）
- **既有 Stripe / Creem / IAP / WeChat Pay 集成**：货币解析仅对 Stripe 多 Price 生效；其余渠道保持 provider 侧定价（DEC-multiple_currency-003）
- **权限系统**：api-ext 货币查询遵循既有 SDK 访问控制（`billing.view`）

---

## 4. 业务规则与状态

### 4.1 业务规则

**货币选择规则**：
- 不存在默认货币：购买页不预选任何货币，用户显式选择后才展示该货币的价格行
- 单一货币的权益组直接展示（唯一选项，非默认）；多货币时显示「请选择货币」提示直到选定
- 货币码比较不区分 ASCII 大小写；对外暴露的货币集合、解析结果统一以大写 ISO 4217 码表达（DEC-multiple_currency-012）
- 购买页与 api-ext 镜像端点按存储原样返回货币码（Stripe 同步写入为小写、WeChat 手工配置为大写）；仅强绑定（固定币种）解析端点保证大写规范化
- 程序化解析入参货币码须满足 `^[A-Z]{3}$` 且非保留码（`XXX`/`XTS`），否则拒绝（DEC-multiple_currency-010）

**货币解析规则**：
- 解析键 = (产品/权益 + 计费维度[类型/周期] + 货币)；货币是过滤维度而非唯一键（DEC-multiple_currency-005）
- 同一产品在同一货币下可并存多个计费周期/类型（如 USD 月付 + USD 年付）；此时货币选择只缩小候选范围，用户仍须在所选货币内选择计费周期/类型
- 仅指定货币（未指定计费维度）且该货币下存在多行时，不得静默选择任一行；解析失败须 fail-loud，不使用默认价格（沿用 subscription PRD 既有约束）
- 货币解析仅对 Stripe 多 Price 产品生效；Creem、IAP 与 WeChat Pay 等 provider 侧定价渠道不参与（DEC-multiple_currency-003）

**Checkout 构造规则**：
- 货币解析只决定「选哪一行映射」；选中价格行后，Checkout 仍引用真实 Stripe Price / Creem `product_id`，构造方式不变
- Stripe 映射行缺失价格信息时拒绝下单（fail-loud），不产生零金额或串货币支付；显式 `target_id` 购买的既有路径行为不变（DEC-multiple_currency-009）
- provider/store 侧定价渠道（Creem、IAP、WeChat Pay）的价格由渠道侧决定，Herald 不做服务端价格解析；其映射行无 Herald 侧价格信息在解析/目录层属合法状态，不触发解析层 fail-loud（DEC-multiple_currency-013）。分层例外：WeChat Pay v3 无托管定价且下单必须传正数金额，价格由管理员在映射行手工配置（wechat-support.md §2.2/§7.1）；WeChat 映射行在**写入层**即强制正数金额与合法货币（缺价或非正数价格在映射保存时被拒，"可缺价的映射行"中间状态不存在），下单构造层保留防御性二次守卫，不产生零金额订单

**api-ext 暴露规则**：
- 对每个可购权益聚合其启用映射行覆盖的货币集合并对外暴露
- 订阅类映射与一次性购买类映射均暴露货币
- 货币集合仅反映已启用映射行，不包含禁用映射行的货币
- 支持第三方应用按货币解析默认价格行：命中返回；无匹配即 fail-loud，不回退其他货币

**数据隔离规则**：
- 不同 Realm 的货币集合与解析结果完全隔离

### 4.2 关键状态与异常

**异常场景**：
- 同货币多计费周期且未指定计费维度：解析 fail-loud，不静默选价，不发起支付
- 非法货币码（含保留码 `XXX`/`XTS`）：解析请求被拒绝（DEC-multiple_currency-010）
- Stripe 映射行缺失价格信息：下单被拒绝（fail-loud），不产生零金额或串货币支付（DEC-multiple_currency-009）
- 渠道无可选货币价格（Creem/IAP/WeChat Pay）：降级为单一价格展示，不渲染货币切换器（DEC-multiple_currency-003/013）
- WeChat Pay 映射行未配置价格：写入层即拒绝（缺价或非正数价格在映射保存时被拒，"可缺价的映射行"中间状态不存在，见 §4.1 与 DEC-multiple_currency-013）；下单构造层保留防御性二次守卫（"WeChat order requires a positive amount"，fail-loud）——WeChat v3 下单必须传金额，价格只能来自映射行手工配置
- Stripe Adaptive Pricing 启用导致的展示与实付货币不一致：**已知展示局限**。Herald 缓存的产品基础货币为展示货币；若运营方在 Stripe 启用 Adaptive Pricing，Checkout 会按用户地区自动换算展示与扣款，可能出现购买页展示货币（基础货币）与用户实付货币不一致。以 Checkout 实际呈现为准，购买页对基础货币做标注，不在 Herald 侧做换算（非阻塞，沿用 DEC-multiple_currency-002 不改同步）

---

## 5. 验收目标

- 购买页对 Stripe 多 Price 产品按货币分组、无预选货币；显式选择后展示该货币价格行；同货币多计费周期时用户可在货币组内选择周期
- 下单始终指向显式选定的 mapping id；扣款货币与展示一致，不出现串货币或零金额下单
- api-ext 对每个可购权益暴露支持货币集合，订阅类与一次性类均含货币；第三方应用可据此做货币切换与按货币解析
- 同货币多行且未指定计费维度时解析 fail-loud，不发起支付；无匹配货币即 fail-loud，不回退其他货币
- Creem / IAP / WeChat Pay 产品降级为单一价格展示，不渲染货币切换器
- 非法货币码在解析请求中被拒绝

---

## 6. 边界与约束

**适用性**: 适用（API 与前端/交互边界合并陈述）

**API / 集成边界:**
- **接口能力范围**：货币维度解析属购买/默认解析能力的扩展；可购权益的「支持货币集合」聚合与按货币解析默认价格行属 api-ext 查询能力的扩展。Checkout 发起与履约沿用既有统一能力，不在 PRD 列出端点、schema 或状态码。
- **访问控制原则**：遵守 realm 隔离；api-ext 货币查询遵循既有 SDK/第三方应用访问控制（`billing.view`）；金额与积分变更必须可追溯。
- **租户/realm 边界**：货币集合与解析结果按 Realm 隔离；货币集合仅反映该 Realm 内启用映射行。
- **兼容性要求**：项目未上线，废除偏好货币承载（DEC-multiple_currency-014）为破坏性变更，不做迁移兼容；与 Stripe/Creem/IAP/WeChat Pay、积分账本、订阅系统的详细契约下沉到技术设计。

**前端 / 交互边界:**
- **购买页（Stripe 多 Price 产品）**：按货币分组展示价格行；无预选货币，多货币时先显示「请选择货币」提示；同货币组内列出可选计费周期/类型；提供货币切换；显式选定的货币不因刷新/选项刷新而静默变更（手动选择保留，选定货币消失时回到待选状态）。
- **购买页（Creem / IAP / WeChat Pay 等 provider 侧定价渠道）**：降级为单一价格展示，不渲染货币切换器与货币分组。
- **状态反馈**：待选货币提示、解析失败（fail-loud）的明确反馈；Stripe Adaptive Pricing 场景下对基础货币做标注，说明实际扣款以支付页为准。
- **金额/积分变化**：货币切换与购买场景必须突出所选货币的金额变化与不可逆风险提示。

---

## 7. 已确认决策

> 本节只收录当前有效的决策与未决问题，记取舍、理由、决策人与重开条件；规则正文只在 §4 定义，DEC/Q 编号保持稳定，供代码注释、测试与跨 PRD 引用追溯。本组决策源于 2026-08-13 用户问题「Stripe、Creem 等是否支持一个产品不同货币不同价格，若支持本项目是否要做调整」引发的 `/t-tech-research multiple-currency` 范围裁决。

- **DEC-multiple_currency-001 · 范围路线**（user）：新增「按货币选择/本地化体验」功能，构建于现有多 Price 目录；非「仅确认现状」、非「改走 currency_options」。理由：用户在范围裁决中明确选择（AskUserQuestion 2026-08-13）；现有目录已支持多货币多价格，本特性在其上叠加选择/本地化层。落点：§2.1。重开条件：用户改判为仅确认现状或改走 currency_options。
- **DEC-multiple_currency-002 · 目录模型**（agent）：货币选择层基于现有「一映射行对应一个 provider Price」模型，不改同步、不读 `currency_options`。理由：同步已按 `product.prices` 逐 Price 建行（多货币多价格天然支持）；`currency_options` 是另一范式需改同步且相对多 Price 无额外收益；保持零新增依赖；不改变产品语义。落点：§2.2、§4.1、§6。重开条件：出现必须以单 Price 多货币（currency_options）建模的强需求。
- **DEC-multiple_currency-003 · 渠道覆盖**（agent）：仅 Stripe 多 Price 纳入货币解析；Creem/IAP 等 provider 侧定价渠道保持渠道侧定价，降级为单一价格展示。理由：Creem 无每货币价格对象可选；IAP 价格由商店按地区管理，Herald 只验凭证；对二者强加货币选择无可选内容且徒增复杂度。落点：§2.1、§2.2、§4.1、§6。重开条件：Creem 提供「一产品多货币多价格」的真实价格对象。
- **DEC-multiple_currency-005 · 解析键**（agent）：解析键 = (产品/权益 + 计费维度 + 货币)；货币为过滤维度，非唯一键；同货币多计费周期并存时由用户在货币内选周期（规则正文见 §4.1）。理由：既有目录多价格模型允许同产品多计费周期，货币单独无法唯一定位价格行（tech §4.1/§5.5）；沿用既有 fail-loud 解析约束，不改变产品语义。落点：§4.1、§6。重开条件：目录模型改为单 Price 多货币（Stripe currency_options）。
- **DEC-multiple_currency-008 · 程序化解析暴露**（agent）：程序化默认解析经 api-ext 暴露（货币集合聚合 + 按货币解析默认价格行，fail-loud）；终端用户购买始终显式选定价格行，货币分组由前端完成（规则正文见 §4.1）。理由：终端用户在分组 UI 总能选到具体行（target_id），仅第三方应用「仅传货币」场景需服务端解析；分离避免改动核心支付请求契约；解析语义（fail-loud、无二级回退）由 DEC-005/006 决定，本 DEC 只定暴露位置。落点：§4.1。重开条件：终端用户也需「仅传货币」的浏览器默认解析。
- **DEC-multiple_currency-009 · 缺价 fail-loud**（agent）：Stripe 映射行缺失价格信息时拒绝下单（fail-loud），不产生零金额/串货币支付；显式 `target_id` 路径行为不变（规则正文见 §4.1）。理由：硬编码兜底导致零金额/串货币风险（下游 `payment_attempts CHECK(amount>0)` 强制 amount=1）；PRD §4.1「Checkout 构造规则」要求替换为 fail-loud；新增错误变体沿用既有 `CoreError→ApiError` 范式。落点：§4.1、§4.2。重开条件：—。
- **DEC-multiple_currency-010 · 货币码校验**（agent）：`^[A-Z]{3}$` 格式 + 拒绝 ISO 4217 保留码（`XXX`/`XTS`）；非法码被拒（规则正文见 §4.1）。理由：维持零新增依赖（DEC-002）；格式合法但无对应价格行的货币在解析端已 fail-loud（DEC-006），无需字典级校验；用户故事 US-MC-001 场景 3 要求 `XXX`/非标码被拒。落点：§4.1、§5。重开条件：需要完整 ISO 4217 真实货币字典级校验。
- **DEC-multiple_currency-012 · 大小写归一**（agent，2026-08-14 由回归失败暴露）：货币码匹配不区分 ASCII 大小写（目录存储 provider 原生码）；对外暴露统一大写 ISO 码（规则正文见 §4.1）。理由：设计 §5.3 片段在 `resolve_target` 内调用 `validate_currency_code` 会把所有小写存量货币（Stripe 事实格式）的显式 `target_id` 正常购买变成 422，与 DEC-009「正常路径行为不变」冲突；冲突裁决取不变量侧（Rule: 更强约束优先），Stripe Checkout API 也要求小写货币码。落点：§4.1。重开条件：目录改为统一大写存储货币码。
- **DEC-multiple_currency-013 · fail-loud 范围**（agent，2026-08-14 由回归失败暴露）：缺价 fail-loud 仅对 Stripe 映射行生效；provider/store 侧定价渠道（apple/google/wechat/creem）缺价信息在解析/目录层属合法状态，不视为异常（规则正文见 §4.1）。分层例外：WeChat Pay 映射行在写入层即要求正数金额（WeChat v3 下单必须传金额，缺价/非正数在映射保存时被拒；下单构造层为防御性二次守卫）。理由：设计 §1.1 非目标「不改 Creem/IAP 集成」+ DEC-003（Creem/IAP provider 侧定价）与 DEC-009「缺价 fail-loud」冲突；仓库事实：admin API 创建的 IAP/Creem/WeChat 映射无 provider_product_info（`CreateEntitlementMappingRequest` 无该字段），`create_iap_payment_attempt` 注释明确依赖 amount=0；若无条件 fail-loud，Google IAP 购买主链路（iap_receipt_scenarios）被破坏。落点：§4.1、§4.2、§6。重开条件：Creem/IAP 映射开始承载服务端解析的价格信息，或产品裁决 store-priced 渠道也须 fail-loud。
- **DEC-multiple_currency-014 · 显式货币选择**（user，2026-08-15 会话裁决）：废除「默认/偏好货币」：无 Realm 默认、无用户偏好、无回退链；购买页显式选择后才渲染价格行（单一货币为唯一选项自动选中）；程序化解析须显式传 currency（规则正文见 §4.1）；取代 DEC-004/006/007/011。理由：用户裁决（2026-08-15）：「具体展示什么货币对于用户来讲必须强制限定的，不能做默认」——货币必须是显式选择，不允许任何默认/偏好兜底链；偏好属投机性设计（收益仅展示高亮，成本含 realm_config 校验、purchase-options 权限绕行字段、前后端解析链）。落点：§2.1、§2.2、§4.1、§6。重开条件：用户改判需要租户级/用户级货币偏好。


---

## 8. 参考资料

- 用户故事来源见 §1 表格
- 相关 PRD：`docs/prd/billing/subscription.md`（订阅计费/多价格目录基线）
- 相关 PRD：`docs/prd/billing/stripe-payment.md`（Stripe 集成）
- 相关 PRD：`docs/prd/billing/credit-bucket.md`（积分账户与履约路由）
- 角色定义：`docs/user-stories/_roles.md`
