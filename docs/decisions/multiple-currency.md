# multiple-currency Decision Log

> 来源：2026-08-13 用户问题「Stripe、Creem 等是否支持一个产品不同货币不同价格，若支持本项目是否要做调整」+ `/t-tech-research multiple-currency` 范围裁决。
> 背景：技术预研确认现有 `provider_entitlement_mappings` 已按 Price 粒度建映射行（一产品多货币多价格在目录层已被支持）；用户裁决范围为本特性**新增**「按货币选择/本地化体验」，属新功能，需 PRD。

## Active Decisions

| ID | Level | Topic | Decision | Rationale | Decided By | Source | Affects | Reopen When | Supersedes |
|---|---|---|---|---|---|---|---|---|---|
| DEC-multiple_currency-001 | D1 | scope.feature-route | 新增「按货币选择/本地化体验」功能（Realm/用户偏好货币、按货币自动匹配价格行、缺失回退、购买页按货币分组、api-ext 暴露可选货币），构建在现有「一 Price 一映射行」目录模型之上；**非**「仅确认现状」，**也非**「改走 Stripe currency_options」 | 用户在范围裁决中明确选择（AskUserQuestion 2026-08-13）；现有目录已支持多货币多价格，本特性在其上叠加选择/本地化层 | user | conversation | prd/design/task | 用户改判为仅确认现状或改走 currency_options | — |
| DEC-multiple_currency-002 | D2 | architecture.catalog-model | 货币选择层基于现有「一映射行对应一个 provider Price」目录模型实现，不切换到 Stripe `currency_options`、不改同步逻辑 | 同步已按 `product.prices` 逐 Price 建行（多货币多价格天然支持）；`currency_options` 是另一范式需改同步且相对多 Price 无额外收益；保持零新增依赖；不改变产品语义 | agent | .ai/tech-research/multiple-currency.md §4 | design/task | 出现必须以单 Price 多货币（currency_options）建模的强需求 | — |
| DEC-multiple_currency-003 | D2 | scope.provider-coverage | 按货币选择/本地化仅对 Stripe 多 Price 产品生效；Creem（产品级单一价格、无 price 对象）与 IAP（Apple/Google 商店侧按地区定价）保持 provider 侧定价，不纳入 Herald 货币解析 | Creem 无每货币价格对象可选；IAP 价格由商店按地区管理，Herald 只验凭证；对二者强加货币选择无可选内容且徒增复杂度 | agent | .ai/tech-research/multiple-currency.md §3/§4 | prd/design/task | Creem 提供「一产品多货币多价格」的真实价格对象 | — |
| DEC-multiple_currency-005 | D2 | resolution.key | 货币是价格行解析的「过滤维度」而非唯一键；解析键 = (产品/权益 + 计费维度[类型/周期] + 货币)；同一产品在同一货币下可并存多个计费周期/类型（如 USD 月付 + USD 年付），此时货币选择只缩小候选范围，用户仍须在所选货币内选择计费周期/类型 | 既有目录多价格模型允许同产品多计费周期，货币单独无法唯一定位价格行（tech §4.1/§5.5）；沿用既有 fail-loud 解析约束，不改变产品语义 | agent | .ai/tech-research/multiple-currency.md §4.1/§5.5；docs/prd/billing/subscription.md §5.1 | prd/design/task | 目录模型改为单 Price 多货币（Stripe currency_options） | — |
| DEC-multiple_currency-008 | D2 | resolution.exposure | 程序化默认货币解析经 api-ext 暴露：`GET /api/ext/{realmId}/entitlements/{entitlementKey}/currencies`（货币集合聚合）+ `GET .../default-price?currency=&billingType=&billingPeriod=`（按货币解析，命中返回/无匹配 404/歧义 409）；终端用户购买（浏览器）始终走显式 `target_id`，由前端对扁平 `PurchaseOptionView[]` 做货币分组，不改 `CreatePaymentAttemptRequest` | 终端用户在分组 UI 总能选到具体行（target_id），仅第三方应用「仅传货币」场景需服务端解析；分离避免改动核心支付请求契约；解析语义（fail-loud、无二级回退）由 DEC-005/006 决定，本 DEC 只定暴露位置 | agent | .ai/design/multiple-currency.md §4.2/§5.2；DEC-multiple_currency-005/006 | task | 终端用户也需「仅传货币」的浏览器默认解析 | — |
| DEC-multiple_currency-009 | D2 | purchase.fallback-fix | `resolve_target`（`backend/infra/src/purchase/purchase_service.rs:418-438`）缺失价格时的硬编码 `(0, "usd")` 兜底改为 fail-loud：新增 `CoreError::PriceInfoMissing` → 422；显式 `target_id` 正常路径（JSONB 含价）行为不变，仅数据异常路径报错 | 硬编码兜底导致零金额/串货币风险（下游 `payment_attempts CHECK(amount>0)` 强制 amount=1）；PRD §4.1「Checkout 构造规则」要求替换为 fail-loud；新增错误变体沿用既有 `CoreError→ApiError` 范式 | agent | .ai/design/multiple-currency.md §4.2/§5.3；.ai/prd/billing/multiple-currency.md §4.1 | task | — | — |
| DEC-multiple_currency-010 | D2 | currency.validation | 货币码校验：`^[A-Z]{3}$` 格式 + 拒绝 ISO 4217 保留码（`XXX`/`XTS`）；不维护完整货币字典、不引入 ISO 4217 crate；前后端共用规则 | 维持零新增依赖（DEC-002）；格式合法但无对应价格行的货币在解析端已 fail-loud（DEC-006），无需字典级校验；用户故事 US-MC-001 场景 3 要求 `XXX`/非标码被拒 | agent | .ai/design/multiple-currency.md §4.5/§5.1/§5.4；DEC-multiple_currency-002；US-MC-001 | task | 需要完整 ISO 4217 真实货币字典级校验 | — |
| DEC-multiple_currency-012 | D2 | currency.case-normalization | 目录数据货币码以 ASCII 大小写不敏感方式解析：Stripe 同步存储小写码（"usd"），偏好写入与 api-ext 解析入参用大写 ISO 码（"USD"）；`resolve_price_row`/`collect_currencies` 匹配时忽略大小写并归一化输出为大写；`resolve_target` 不对存量 JSONB 货币码做格式校验（仅缺失时 fail-loud），透传原值给 Checkout | 设计 §5.3 片段在 `resolve_target` 内调用 `validate_currency_code` 会把所有小写存量货币（Stripe 事实格式）的显式 `target_id` 正常购买变成 422，与 DEC-009「正常路径行为不变」冲突；冲突裁决取不变量侧（Rule: 更强约束优先），Stripe Checkout API 也要求小写货币码 | agent | 实现：`backend/domain/src/billing/currency_resolution.rs`、`backend/infra/src/purchase/purchase_service.rs`（super-run backend/dev 2026-08-14，由 paywall_m3 场景回归失败暴露） | task/test | 目录改为统一大写存储货币码 | — |
| DEC-multiple_currency-013 | D2 | purchase.fallback-scope | `PriceInfoMissing` fail-loud 仅对 `payment_provider='stripe'` 的映射行生效（Stripe 是唯一由 Herald 价格驱动扣款的 provider）；store/provider 侧定价的 apple/google/wechat/creem 映射缺价信息是合法状态，保留历史 amount=0 快照（IAP 路径由 `create_iap_payment_attempt` 0→1 哨兵约束满足 CHECK） | 设计 §1.1 非目标「不改 Creem/IAP 集成」+ DEC-003（Creem/IAP provider 侧定价）与 DEC-009「缺价 fail-loud」冲突；仓库事实：admin API 创建的 IAP/Creem/WeChat 映射无 provider_product_info（`CreateEntitlementMappingRequest` 无该字段），`create_iap_payment_attempt` 注释明确依赖 amount=0；若无条件 fail-loud，Google IAP 购买主链路（iap_receipt_scenarios）被破坏 | agent | 实现：`backend/infra/src/purchase/purchase_service.rs` resolve_target provider 分支（super-run backend/test 2026-08-14，由 iap_receipt_scenarios 失败暴露） | task/test | Creem/IAP 映射开始承载服务端解析的价格信息，或产品裁决 store-priced 渠道也须 fail-loud | — |
| DEC-multiple_currency-014 | D1 | currency.explicit-selection | **取消「默认/偏好货币」概念**：删除 Realm 默认货币（realm_config `billing/default_currency` + `ConfigType::Billing`）与用户偏好货币（`profile.preferred_currency`，migration 0008 一并删除）。购买页展示全部已配置货币但**无预选**——多货币时用户显式选择后才渲染价格行，单一货币自动选中（唯一选项，非默认）；api-ext default-price 维持显式 `currency` 参数、无回退。下单仍只认显式选中的 mapping id | 用户裁决（2026-08-15）：「具体展示什么货币对于用户来讲必须强制限定的，不能做默认」——货币必须是显式选择，不允许任何默认/偏好兜底链；偏好属投机性设计（收益仅展示高亮，成本含 realm_config 校验、purchase-options 权限绕行字段、前后端解析链） | user | conversation（2026-08-15） | prd/design/task/test/docs | 用户改判需要租户级/用户级货币偏好 | DEC-multiple_currency-004/006/007/011 |

## Resolved Questions

| ID | Topic | Resolution | Decision ID | Source |
|---|---|---|---|---|
| Q-multiple_currency-000 | scope.feature-route | 本特性范围：新增按货币选择/本地化体验（非仅确认、非 currency_options 路线） | DEC-multiple_currency-001 | conversation |
| Q-multiple_currency-002 | preference.layering | 偏好货币承载两级（Realm 默认 + 用户覆盖，用户优先），不设 Client App 级 | DEC-multiple_currency-004 | tech-research §6.2 + DEC-001 |
| Q-multiple_currency-003 | resolution.key | 解析键含计费维度，货币为过滤维度；同货币多计费周期并存时由用户在货币内选周期 | DEC-multiple_currency-005 | tech-research §4.1/§5.5 |
| Q-multiple_currency-001 | preference.fallback | 展示侧：全展示可用货币并按 偏好→Realm 默认→首个可用 依次高亮，允许手动切换；程序化默认解析侧：仅按生效偏好货币解析一次，无二级回退，无匹配即 fail-loud | DEC-multiple_currency-006 | conversation（/t-prd-check AskUserQuestion 2026-08-13） |

## Deferred Questions

| ID | Topic | Why Non-blocking Now | Owner Stage | Must Resolve Before | User Informed |
|---|---|---|---|---|---|
| — | — | — | — | — | — |

## Superseded Decisions

| ID | Superseded By | Previous Decision | Source |
|---|---|---|---|
| DEC-multiple_currency-004 | DEC-multiple_currency-014 | 偏好货币两层承载（Realm 默认 + 用户覆盖，用户优先），不设 Client App 级 | .ai/tech-research/multiple-currency.md §4.1/§6.2 |
| DEC-multiple_currency-006 | DEC-multiple_currency-014 | 偏好缺失回退：展示侧按 偏好→Realm 默认→首个可用 高亮；程序化侧仅按生效偏好解析一次 | conversation（/t-prd-check 2026-08-13） |
| DEC-multiple_currency-007 | DEC-multiple_currency-014 | 偏好字段形态：realm_config `billing/default_currency` + `profile.preferred_currency` | .ai/design/multiple-currency.md §4.3/§5.1 |
| DEC-multiple_currency-011 | DEC-multiple_currency-014 | 偏好货币权限门控（settings.manage/view + profile 自助） | .ai/design/multiple-currency.md §4.5 |
