# App Store / Google Play 内购(IAP) 支持 产品需求文档 (PRD)

**创建时间**: 2026-07-28
**优先级**: P1

> 场景背景：移动端 App 受 Apple/Google 数字商品政策约束，无法使用 Stripe/Creem，必须走平台内购。IAP 是与 Stripe、Creem 并列的独立支付渠道，不是钱包（Apple Pay / Google Pay）。本文档不承载接口端点、请求/响应 schema、HTTP 状态码、数据库建表/迁移或代码类型定义；技术方案细节请参见对应技术设计。

---

## 1. 相关用户故事

> 详细故事与验收标准请查看 `docs/user-stories/` 中对应文档。

### 1.1 相关故事

IAP 渠道独有场景，来源 `docs/user-stories/billing/support-iap.md`：

- `[US-IAP-001]` 配置 IAP 支付渠道凭证 —— P0（角色：Realm Admin）
- `[US-IAP-002]` 建立 IAP 商品与权益的映射 —— P0（角色：Realm Admin）
- `[US-IAP-003]` 客户端提交凭证触发履约（主路径） —— P0（角色：第三方应用开发者）
- `[US-IAP-004]` Apple 服务端通知驱动生命周期与兜底 —— P0（角色：Herald 系统）
- `[US-IAP-005]` 查询 IAP 订阅与权益状态 —— P1（角色：第三方应用开发者）
- `[US-IAP-006]` 定时拉取对账（Google 生命周期主驱动 / Apple 补偿） —— P0（角色：Herald 系统）

复用的既有用户故事（IAP 复用，不重复创建）：

**通用支付尝试生命周期**，来源 `docs/user-stories/billing/payment-attempt.md`：
- `[US-PA-001]` 创建支付尝试 —— P0
- `[US-PA-002]` 查询支付尝试状态 —— P0
- `[US-PA-003]` 处理支付成功后的履约 —— P0
- `[US-PA-004]` 关闭过期的支付尝试 —— P1

**通用支付平台配置**，来源 `docs/user-stories/billing/payment-provider.md`：
- `[US-PV-001]` 配置支付平台 —— P0
- `[US-PV-002]` 查看支付平台配置 —— P0

**集成方前端充值/购买旅程（移动 App 接入基线）**，来源 `docs/user-stories/integration/custom-user-ui.md`：
- `[US-CUI-008]` 集成方前端完成充值/购买 —— P0

**Entitlement 映射**，来源 `docs/user-stories/billing/entitlement-mapping.md`：多价格映射、webhook 解析链等模型，IAP mapping 直接复用其语义。

### 1.2 优先级汇总

**IAP 独有故事**

| 优先级 | 数量 | 关键故事 |
|--------|------|----------|
| P0 | 5 | IAP 凭证配置、商品映射、客户端提交履约（主路径）、Apple 通知驱动生命周期与兜底、定时拉取（Google 生命周期主驱动） |
| P1 | 1 | 权益查询 |
| P2 | 0 | - |

**复用既有（IAP 履约链路依赖，不重复创建）**

| 优先级 | 故事 ID | 来源 |
|--------|---------|------|
| P0 | US-PA-001、US-PA-002、US-PA-003 | `docs/user-stories/billing/payment-attempt.md`（创建/查询/履约） |
| P1 | US-PA-004 | `docs/user-stories/billing/payment-attempt.md`（关闭过期 attempt） |
| P0 | US-PV-001、US-PV-002 | `docs/user-stories/billing/payment-provider.md`（配置/查看 provider） |
| P0 | US-CUI-008 | `docs/user-stories/integration/custom-user-ui.md`（集成方前端购买旅程） |

> 上述两张表分别计 IAP 独有故事与履约链路复用故事；Entitlement 映射（`docs/user-stories/billing/entitlement-mapping.md`）以模型语义复用，未单列故事 ID。

---

## 2. 范围界定

### 2.1 包含功能

- **App Store + Google Play 双平台内购**：作为与 Stripe、Creem 并列的独立支付渠道接入，覆盖 iOS（StoreKit 2）与 Android（Google Play Billing）
- **IAP 凭证配置**：Realm Admin 在支付平台管理页配置 Apple（Bundle ID / Issuer ID / Key ID / `.p8` 私钥 / 通知环境 sandbox 或 production）与 Google（Package Name / Service Account JSON）的服务端校验凭证；Apple 侧另含服务端通知接收配置
- **IAP 商品 → entitlement 映射**：复用现有 `provider_entitlement_mappings` 模型，provider 取 IAP 类型，external_product_id 取商店商品 ID；支持订阅（recurring）与消耗型积分包（one_time）两种商品类型
- **客户端凭证提交驱动履约（主路径）**：移动 App 通过既有 api-billing 浏览器路由提交 Apple `jwsRepresentation`（JWS 密码学证明）或 Google `purchaseToken`，Herald 校验后履约，作为购买履约的权威触发源
- **Apple 服务端通知兑付（事件流 + 兜底）**：接收 Apple App Store Server Notifications V2，驱动续费、退款、取消等后续生命周期，并对漏发的购买事件兜底
- **Google 生命周期定时轮询（无 RTDN）**：定时任务对活跃 Google 订阅逐 token 回查 Developer API 并拉取 voidedpurchases，驱动 Google 续费、退款、取消等生命周期，事件延迟以对账间隔为界
- **IAP 权益查询**：第三方应用通过既有 SDK / api-billing 路由查询 IAP 订阅状态与 entitlement_key，与 Stripe / Creem 订阅统一格式返回
- **定时拉取对账（IAP 适配）**：定时向 App Store Server API / Google Play Developer API 拉取近期交易与订阅状态；对 Apple 识别并补偿本地缺失的履约事件，对 Google 驱动全部生命周期事件

### 2.2 不包含功能 (Out of Scope)

- **Apple Pay / Google Pay 钱包支付**：钱包支付是依附于 Stripe/Creem 的资金来源，独立立项承载，与本 PRD 互不依赖
- **Herald 移动 SDK**：Herald 不交付 iOS/Android 原生 SDK；StoreKit 2 / Google Play Billing 的客户端集成、商品展示、购买 UI 与购买失败反馈由各移动 App 集成方自行实现
- **IAP 商品定价与商店管理 UI**：商品定价、订阅周期、退款规则、佣金档位由各 Realm 在 App Store Connect / Google Play Console 自行配置并维护，Herald 不维护本地商品目录、不提供商店管理界面
- **Apple/Google 佣金（15–30%）核算**：佣金不纳入 Herald 代码范围；与现有促销 / 税务边界一致，Herald 不在本地实现佣金分摊或定价建议
- **IAP 发票 / 税务**：Apple/Google 作为 merchant-of-record 对终端用户承担发票与税务义务，Herald 既不为 IAP 交易创建 manual 发票，也不同步外部发票（IAP 无发票 API）；与 `docs/prd/billing/invoice.md` 中 Creem MoR 约束同性质，IAP 交易不进入 Herald 发票体系
- **非消耗型买断（buyout）与非续期订阅（non-renewing subscription）**：本 PRD 不定义这两种形态；其产品规则见 [履约模型扩展](pay_model.md)
- **跨平台订阅共享（universal entitlement）**：用户在 App Store 与 Google Play 各自独立订阅；同一用户跨平台共享订阅状态不在范围内
- **家庭共享、优惠代码、引导价等商店侧能力**：由商店侧承载，Herald 只接收其通知并履约，不实现本地逻辑
- **Google Play RTDN（实时开发者通知）**：RTDN 只能经 GCP Cloud Pub/Sub 投递，当前不接入以避免 GCP 运维负担；Google 生命周期由定时轮询驱动，事件延迟以对账间隔为界；后续可增量接入 RTDN 获得实时性，接入后业务语义不变

### 2.3 依赖项

- **既有 Billing 体系**：订阅计费（`docs/prd/billing/subscription.md`）、统一支付尝试与履约（`docs/user-stories/billing/payment-attempt.md`）、积分系统（`docs/prd/billing/points.md`）
- **既有 Entitlement 映射模型**：provider-agnostic 的 `provider_entitlement_mappings`（`docs/prd/billing/subscription.md` §4.1）；IAP mapping 直接复用其语义，provider 字段取 IAP 类型
- **既有自建用户 UI 集成模式**：移动 App 经 api-billing 浏览器路由 + Bearer token + scope 接入（`docs/user-stories/integration/custom-user-ui.md` US-CUI-008）
- **既有 Webhook 补偿模型**：缺失事件补偿复用 Stripe/Creem 已建立的领域处理与幂等机制（`docs/prd/billing/subscription.md` §4.1 Webhook 补偿规则），IAP 适配的是平台通知 API 而非 webhook 签名
- **集成方自有 Apple Developer / Google Play 账号**：IAP 商品创建、商店发布、Apple/Google 平台凭据由集成方负责；Herald 只持有服务端校验与通知接收所需凭证
- **Apple Root CA 信任锚**：Herald 自管 Apple Root CA 证书（如 `AppleRootCA-G3.cer`），作为 JWS 验签信任根，不依赖第三方

---

## 3. 需求概述

### 3.1 功能描述

为 Herald 多租户计费系统新增 App Store 与 Google Play 内购（IAP）作为独立支付渠道，覆盖移动端 App 在 Apple/Google 数字商品政策下无法使用 Stripe/Creem 的场景。IAP 接入复用现有 provider-agnostic 的 entitlement 映射与统一履约链路，履约对资金来源与渠道透明；新增的能力集中在 IAP 凭证配置、客户端凭证提交校验（履约主路径）、Apple 服务端通知接收与 Google 定时轮询（生命周期与兜底）三类渠道专属环节。

### 3.2 关键特性

- **第三种支付渠道，与 Stripe/Creem 并列**：在现有支付平台配置体系内新增 Apple App Store IAP 与 Google Play Billing 两类 provider；定价与商品生命周期仍由 Apple/Google 作为 source of truth，Herald 不维护本地商品目录
- **两种商品类型履约**：订阅（recurring）复用现有订阅状态机；消耗型积分包（one_time）复用 topup_credit 发放
- **客户端凭证提交为履约主路径**：移动 App 提交 Apple `jwsRepresentation`（JWS 密码学证明，Herald 本地验签无需 Apple 回调）或 Google `purchaseToken`（Herald 调 Developer API 回查真实状态）；这是购买履约的权威触发源
- **Apple 服务端通知为事件流与兜底**：App Store Server Notifications V2 驱动续费、退款、取消等后续生命周期，并对客户端漏提交或通知延迟的购买事件兜底
- **Google 生命周期由定时轮询驱动**：当前不接 RTDN；定时任务对活跃订阅逐 token 回查 Developer API（`subscriptionsv2.get`）并拉取 `voidedpurchases.list`，以 API 状态为准，事件延迟以对账间隔为界
- **幂等保证不重复发放**：以 Apple `originalTransactionId` / Google `purchaseToken` 为去重键，客户端提交与平台通知两条路径各履约一次、最终一致
- **多租户隔离不变**：每个 Realm 配置独立的 IAP 凭证，IAP 通知按 Realm 隔离路由
- **定时拉取兜底**：定时对 App Store Server API / Google Play Developer API 拉取近期交易与订阅状态；对 Apple 补偿漏发通知，对 Google 即生命周期主驱动

---

## 4. 业务规则与状态

### 4.1 业务规则

**配置管理规则**：
- 每个 Realm 使用独立的 App Store / Google Play IAP 凭证配置，复用现有 `realm_config` 与管理端配置 UI，与 Stripe/Creem 配置入口一致
- 私钥、Service Account JSON 等敏感凭证以 realm_config 明文存储并以 `is_secret` 标记（响应脱敏、不回显），应用层加密为后续统一工作（与 Stripe/Creem 等现有 provider 的凭据存储口径一致）；编辑时留空则保留旧值
- 仅 Realm Admin（`settings.manage`）可修改配置；查看需 `settings.view`（与 WeChat/Stripe 等现有 provider 的通用 realm_config 权限面一致；permissions.md 中对应的 `billing.*` 表述以本节为准）
- 删除 IAP 凭证前需检查是否有活跃订阅，存在活跃订阅时拒绝删除并提示数量

**IAP 商品映射规则**：
- 映射承载于现有 `provider_entitlement_mappings`，provider 取 IAP 类型（Apple App Store IAP / Google Play Billing），external_product_id 取商店商品 ID
- 商品定价、订阅周期、退款规则、佣金档位由 Apple/Google 作为 source of truth；Herald 不维护本地商品目录
- 同一 provider + 商品 ID 在同一 Realm 内唯一，重复创建被拒绝
- mapping 必须标注商品类型（订阅 / 消耗型积分包），履约路径由商品类型决定
- 禁用 mapping 后，匹配该商品的通知仍更新订阅投影，但不触发积分发放或权益授予；重新启用后恢复
- 同步失败应 fail loud（返回 partial 状态 + 错误列表），不静默降级为默认积分策略

**客户端凭证提交规则（履约主路径）**：
- 移动 App 通过既有 api-billing 浏览器路由（Bearer token + `PurchaseInitiate` scope）提交 Apple `jwsRepresentation` 或 Google `purchaseToken`
- Apple：Herald 用自管的 Apple Root CA 信任锚对 JWS 做 x5c 证书链 + ES256 签名本地验签（密码学证明，无需回调 Apple），验签失败拒绝履约
- Google：Herald 调 Google Play Developer API（`purchases.subscriptionsv2.get` / `purchases.products.get`）回查真实状态，以 API 返回状态为准
- 凭证校验失败或归属不符当前用户时拒绝履约，返回明确失败原因（4xx）；凭证校验先于支付尝试创建，校验失败不产生 attempt 记录，由客户端修正后重新提交。已创建但履约中断的 attempt 保持待处理，2 小时未完结由过期任务标记 Expired
- 客户端提交不被信任，必须经上述密码学或 API 校验后才予履约
- 用户绑定：购买 attempt 创建时建立 Herald user_id 与凭证的关联；IAP 凭证本身可能不携带 Herald user_id

**Apple 服务端通知处理规则（事件流 + 兜底）**：
- 通知必须通过 JWS 签名验证（x5c 证书链 + Apple Root CA 信任锚），校验失败拒绝处理并记录诊断
- 通知以商品 ID 解析本地 mapping；通知到达次序可能乱序，履约必须幂等
- 商品 ID 在本 Realm 无启用 mapping 时 fail loud，记录诊断并跳过履约，不静默降级
- sandbox 与 production 通知的区分以解码 payload 内的 `environment` 字段为准，不以接收 URL 区分；sandbox 通知可能丢失或乱序，须由定时拉取兜底

**Google 生命周期轮询规则（无 RTDN）**：
- 定时对全部活跃 Google IAP 订阅逐 token 调 `purchases.subscriptionsv2.get` 刷新真实状态，以 API 返回状态为准驱动状态机
- 定时调 `voidedpurchases.list` 发现退款/作废，驱动退款回收
- 轮询间隔必须小于平台事件保留窗口并满足业务时效要求；分页与限流，不触发平台配额
- 轮询发现的变更与客户端提交路径幂等一致，重复轮询不重复副作用

**履约幂等规则**：
- 以 Apple `originalTransactionId` / Google `purchaseToken` 为去重键；客户端提交与平台通知两条路径各履约一次、最终一致，重复通知或重复提交不重复发放
- 复用既有 `payment_event` 表的幂等约束（external_id = IAP 交易标识）

**确认（acknowledge）截止规则**：
- Google 订阅与一次性商品必须在购买后 3 天内 acknowledge（订阅）/ consume（消耗型），否则 Google 静默退款；ack/consume 必须与权益提交事务绑定，履约成功立即执行
- Apple 无对应硬截止，但 StoreKit 2 transaction 应由客户端 finish

**履约分发规则（按商品类型）**：
- 订阅（recurring）：复用现有订阅状态机；收到 SUBSCRIBED / RENEWED / CANCELED / EXPIRED / GRACE_PERIOD / REFUND（Apple 通知）或轮询得到的 API 状态变化（Google）后更新订阅状态，按现有积分策略发放或回收积分
- 消耗型积分包（one_time）：购买成功后发放 topup_credit，不创建 subscription 记录，复用现有 one_time 履约

**安全约束**：
- 私钥、Service Account JSON 等敏感凭证不得暴露给前端
- 客户端提交的 `jwsRepresentation` / purchase token 不被信任，必须经 Herald 密码学验签（Apple）或 Developer API 回查（Google）校验真实性与归属后才予履约
- 所有 IAP 操作必须通过 HTTPS
- 所有 IAP 操作必须记录审计日志

**数据隔离规则（复用既有）**：
- 不同 Realm 的 IAP 凭证、支付数据、订阅与权益完全隔离
- Regular User 只能看到自己的订阅与购买记录
- Realm Admin 只能查看与操作本 Realm 的配置与数据

**发票 / 税务边界（复用既有 MoR 约束）**：
- Apple/Google 作为 merchant-of-record 对终端用户承担发票与税务义务，与 `docs/prd/billing/invoice.md` 中 Creem MoR 约束同性质
- IAP 交易不进入 Herald 发票体系：Herald 既不为 IAP 交易创建 manual 发票（`invoice_policy` 不影响该约束），也不同步外部发票（Apple/Google 不向第三方提供发票/税务同步 API）
- IAP 交易记录（subscription / payment_attempt）仍可查询，但不触发任何发票创建或归属逻辑

**定时拉取对账规则（IAP 适配）**：
- 定时按 Realm 调用 App Store Server API / Google Play Developer API 拉取近期交易与订阅状态，识别本地缺失或滞后的履约事件
- 对 Google，该拉取同时承担生命周期主驱动（见上节轮询规则）；对 Apple，承担通知补偿，分两层：Notification History 拉取（`onlyFailures=true`，补偿投递失败）+ 对本地仍存活的订阅做 getAllSubscriptionStatuses 状态比对，发现 Apple 侧已 Expired/Revoked 的漂移时按 `transactionId` 定向拉取该交易的通知历史（`onlyFailures=false`，覆盖「已投递但本地处理失败」的通知）并复用通知处理管道回放；漂移在通知历史中无可回放事件时仅记诊断，不自动改写订阅状态
- 拉取到的事件复用与正常服务端通知相同的领域处理与数据库幂等约束，不重复改变订阅或积分
- 单个 Realm、交易或平台 API 失败不阻塞其他对象；运行统计分别记录 Apple 拉取/状态轮询/漂移发现/回放/失败与 Google token 轮询/voided 拉取/回放/失败。当前不维护独立的笼统“缺失数”：可恢复的缺失以 replayed 计数，只有状态漂移但找不到可回放事件时以 drift detected 与上下文诊断体现
- 对账间隔必须小于平台事件保留窗口；拉取支持分页与限流控制，不触发平台限流（Apple/Google 回溯窗口参数默认 1800s/900s，实际任务共用定时器，可经 `WORKER_IAP_RECONCILIATION_INTERVAL_SECS` 与 `WORKER_IAP_APPLE_INTERVAL_SECS` / `WORKER_IAP_GOOGLE_INTERVAL_SECS` 调整）
- 状态不一致但不存在缺失事件时只记录诊断，不自动改写数据；不提供手动触发、管理页面或报警通知

### 4.2 关键状态与异常

**订阅状态**（复用既有，非 IAP 新增）：Active / Past Due / Canceled / Expired / Grace Period 等；语义见 `docs/prd/billing/subscription.md` §4.2 与 IAP 平台通知映射。

**异常场景**：
- **客户端凭证校验失败或归属不符**：Herald 拒绝履约，返回明确失败原因（凭证无效 / 归属不符 / 已消耗）；校验先于 attempt 创建，失败时不产生支付尝试记录，由客户端修正后重新提交。已创建但履约中断的 attempt 保持待处理，2 小时未完结由过期任务标记 Expired
- **通知签名或来源校验失败**：拒绝处理，记录诊断，不改变任何权益或积分
- **商品 ID 无对应 mapping**：fail loud，记录诊断并跳过履约，不静默降级
- **客户端提交与平台通知次序错乱**：幂等约束（以 `originalTransactionId` / `purchaseToken` 为去重键）保证两者各履约一次、最终一致，不重复发放
- **Apple 通知丢失、延迟或乱序（尤其 sandbox）**：sandbox 通知丢失/乱序是常态；客户端提交为主路径保证购买即时履约，定时拉取（Notification History / getAllSubscriptionStatuses）在下一周期兜底漏发的后续事件
- **Google 轮询间隔内事件延迟**：无 RTDN，续费/退款/取消最迟在下一个拉取周期反映到本地；间隔须小于平台事件保留窗口
- **Google 3 天 acknowledge 截止**：订阅未 acknowledge 或消耗型未 consume，Google 静默退款；ack/consume 必须与权益提交事务绑定，履约成功立即执行
- **同一用户跨 App Store / Google Play 各有订阅**：视为两条独立订阅，不合并、不共享

---

## 5. 功能需求

### 5.1 核心需求

**IAP 凭证配置**：
- Realm Admin 在支付平台管理页为 Apple App Store IAP 与 Google Play Billing 各自配置服务端校验与通知接收凭证
- 配置创建、查看（脱敏）、更新、删除；删除受活跃订阅保护
- 配置完成后系统提示：Apple 需在 App Store Connect 设置服务端通知 URL；Google 需在 Play Console → API Access 关联 Service Account（无需配置 RTDN）

**IAP 商品映射**：
- 复用 Entitlement 映射管理 UI 与能力，provider 选择 IAP 类型，填入商店商品 ID、entitlement_key、商品类型与积分/权益策略
- 支持订阅、消耗型积分包两种商品类型
- 商品 ID 在同一 provider + Realm 内唯一；同步失败 fail loud

**客户端凭证提交与履约（主路径）**：
- 移动 App 通过既有 api-billing 浏览器路由（Bearer token + `PurchaseInitiate` / `PurchaseStatusRead` scope）提交 Apple `jwsRepresentation` 或 Google `purchaseToken`
- Apple：Herald 本地 JWS 验签（x5c + ES256 + Apple Root CA 信任锚）；Google：Herald 调 Developer API 回查真实状态
- 校验通过后按商品类型履约（订阅 / 积分），校验失败拒绝并返回明确原因
- 履约成功后立即执行 Google acknowledge / consume（3 天硬截止），ack 与权益提交事务绑定
- 已由平台通知履约的交易不重复发放

**Apple 服务端通知接收与兑付（事件流 + 兜底）**：
- 接收 Apple App Store Server Notifications V2（经 JWS 签名验证）
- 驱动续费、退款、取消等后续生命周期
- 对客户端漏提交或通知延迟的购买事件兜底
- 全程幂等，重复通知不重复副作用
- 商品 ID 无映射时 fail loud 并跳过

**Google 生命周期定时轮询（无 RTDN）**：
- 定时对活跃订阅逐 token 回查 `subscriptionsv2.get`，以 API 状态驱动续费、宽限、过期、取消
- 定时拉取 `voidedpurchases.list` 驱动退款回收
- 全程幂等，与客户端提交路径最终一致，重复轮询不重复副作用

**IAP 权益查询**：
- 第三方应用通过既有 SDK / api-billing 路由查询 IAP 订阅与 entitlement，返回格式与 Stripe / Creem 订阅统一
- 退款或过期后权益按订阅状态机降级，历史保留在订阅变更历史

**定时拉取对账**：
- 对账 job 随应用接线运行（`WORKER_IAP_RECONCILIATION_INTERVAL_SECS` 与 Apple / Google 查询回溯窗口参数默认 1800s / 900s，实际调度共用一个定时器，可经 `WORKER_IAP_APPLE_INTERVAL_SECS` / `WORKER_IAP_GOOGLE_INTERVAL_SECS` 调整），定时向 App Store Server API / Google Play Developer API 拉取近期交易与订阅状态，复用既有补偿领域处理与幂等机制；对 Apple 补偿漏发通知，对 Google 驱动全部生命周期
- Apple 补偿分两层：Notification History（`onlyFailures=true`）补偿投递失败；再对本地仍存活的订阅轮询 getAllSubscriptionStatuses，发现 Expired/Revoked 漂移时按 `transactionId` 定向拉取通知历史（`onlyFailures=false`，覆盖已投递但本地处理失败的通知）回放；无匹配通知仅记诊断
- 对账间隔小于平台事件保留窗口；分页与限流；单对象失败不阻塞其他对象

### 5.2 验收目标

- Realm Admin 可为 Apple App Store IAP 与 Google Play Billing 分别创建、查看、更新、删除凭证配置，敏感字段脱敏、删除受活跃订阅保护
- Realm Admin 可建立 IAP 商品到 entitlement 的映射，覆盖订阅、消耗型积分包两种类型
- 用户在移动 App 完成 IAP 购买后，移动 App 提交 `jwsRepresentation` / `purchaseToken`，Herald 经密码学验签或 Developer API 回查校验后完成履约，订阅 / 积分按商品类型正确授予
- Apple 服务端通知与 Google 定时轮询分别驱动续费、退款、取消等后续生命周期，且与客户端提交路径幂等一致、不重复发放
- Google 订阅 acknowledge / 消耗型 consume 在履约成功后立即执行，3 天内完成，不触发 Google 静默退款
- 商品 ID 无映射时 fail loud 并记录诊断，不静默降级
- 通知签名 / 来源 / 凭证校验失败时拒绝处理，不改变权益或积分
- IAP 订阅状态、退款、过期经 Apple 通知 / Google 轮询正确投影，与 Stripe / Creem 订阅以统一格式查询返回
- 缺失的 Apple 通知与 Google 状态变更能在下一拉取周期被处理，结果与正常路径一致且不产生重复副作用
- 全栈不引入 Herald 移动 SDK；rustls / 去 openssl 路线不被破坏
- 不同 Realm 的 IAP 配置与履约按各自凭证独立生效，互不影响
- 所有 IAP 操作记录审计日志

---

## 6. API 相关约束

**适用性**: 适用（约束级，不承载端点 / schema / 状态码细节）

- **接口能力范围**：IAP 在 IAP 凭证配置、商品映射、服务端通知接收、客户端 receipt 提交、权益查询与定时拉取对账上复用既有能力骨架，新增的 IAP 专属能力集中在 Apple 平台通知接收、receipt 校验与定时拉取对账。Herald 不暴露 IAP 专属业务端点给前端；移动 App 通过既有 api-billing 浏览器路由发起 attempt、提交 receipt、查询状态。
- **访问控制原则**：
  - 必须遵守 realm 隔离与既有 `PurchaseInitiate` / `PurchaseStatusRead` / `PurchaseRead` scope 鉴权
  - 配置写入需 `settings.manage`，查看需 `settings.view`
  - 客户端提交的 receipt / token 不被信任，必须经 Herald 密码学验签（Apple）或调用 Google 服务端 API 校验真实性与归属后才予履约
  - 金额与积分变更必须可追溯；履约必须幂等
- **租户 / realm 边界**：每个 Realm 使用独立的 IAP 凭证，平台通知按 Realm 隔离路由；与既有 Stripe / Creem 边界一致。
- **兼容性要求**：与 Apple App Store Server API、Google Play Developer API、积分账本、订阅系统的详细契约应下沉到技术设计或接口说明。Herald 不交付移动 SDK；移动 App 端 StoreKit 2 / Google Play Billing 集成与商店侧发布由各客户端集成方负责。

---

## 7. 前端/交互约束

**适用性**: 适用（约束级，复用既有管理 UI，无重大新增前端）

- **管理入口**：支付平台配置管理页面，Realm Admin 管理 IAP 凭证（与 Stripe / Creem 入口一致）。Entitlement 映射管理页 provider 选项中新增 IAP 类型。
- **关键操作路径**：IAP 凭证创建 / 编辑（敏感字段脱敏 + 留空保留）/ 删除（活跃订阅保护）；IAP 商品映射创建（选 provider = IAP、填商品 ID、选商品类型、配权益策略）。终端用户购买路径在移动 App 内完成，Herald 站内不渲染购买 UI。
- **状态反馈**：Apple 配置成功后提示需在 App Store Connect 设置服务端通知 URL；Google 配置成功后提示需在 Play Console → API Access 关联 Service Account（无需配置 RTDN）；敏感信息脱敏显示；同步失败以 partial + 错误列表呈现，不静默降级。
- **权限可见性**：仅 Realm Admin 可访问配置管理页面；终端用户在移动 App 内的购买能力由商店与客户端 App 决定，Herald 不控制其可见性。
- **金额 / 积分变化**：IAP 购买金额由商店决定，佣金不在 Herald 展示；积分 / 权益变化沿用既有不可逆风险提示。

---

## 8. 已确认决策

### 8.1 范围与场景决策

- **IAP 是独立支付渠道，不是钱包**：App Store / Google Play 内购由 Apple/Google 作为 merchant-of-record 处理扣款、订阅生命周期与佣金，与 Stripe / Creem 钱包场景完全不同，独立立项
- **双平台同时纳入**：Apple App Store（StoreKit 2）+ Google Play Billing 同期接入，避免 iOS/Android 中间状态
- **履约以客户端凭证提交为权威触发源**：Apple 用客户端 `jwsRepresentation` 的 JWS 密码学证明（Herald 本地验签，无需回调 Apple）；Google 用客户端 `purchaseToken`（Herald 调 Developer API 回查真实状态）。Apple SSV V2 作为事件流与兜底，驱动续费/退款/取消与漏发补偿。此决策经技术预研确认，取代此前"服务端通知为主"的设想
- **Google 不接 RTDN / Pub/Sub**：RTDN 只能经 GCP Cloud Pub/Sub 投递，但其本身可选；当前以定时轮询（per-token `subscriptionsv2.get` + `voidedpurchases.list`）驱动 Google 全部生命周期，事件延迟以对账间隔为界，换取零 GCP 运维负担。后续可增量接入 RTDN 获得实时性，接入后业务语义不变
- **佣金不纳入 Herald**：Apple/Google 的 15–30% 佣金不在 Herald 代码范围，与现有促销 / 税务边界一致；定价由各 Realm 在 App Store Connect / Google Play Console 自行决定
- **不交付移动 SDK**：StoreKit 2 / Google Play Billing 客户端集成由各移动 App 集成方负责，Herald 只承担服务端通知接收、receipt 校验、履约与查询

### 8.2 商品类型与履约决策（约束 PRD 边界，不承载实现细节）

- **两种商品类型为本 PRD 编目边界**：自动续期订阅（recurring）、消耗型积分包（one_time）
- **复用现有履约**：recurring 复用订阅状态机与续费积分策略，one_time 复用 topup_credit 发放；不扩展 `BillingType`
- **非消耗型买断（buyout）与非续期订阅（non-renewing）溢入实现**：履约链路按 [履约模型扩展](pay_model.md) 的通用规则完整承载这两种形态（buyout 走 one_time 语义、non-renewing 建订阅行并到期不续），IAP 侧（凭证提交、Apple 通知、Google 轮询）与 Stripe/Creem 路径行为一致；商品规则本体仍由 pay_model.md 维护
- **跨平台订阅不共享**：App Store 与 Google Play 各自独立订阅，不合并、不共享状态

### 8.3 显式假设（列为约束）

- **IAP 商品定价与生命周期由 Apple/Google 作为 source of truth**：Herald 不维护本地商品目录，仅在 mapping 中以商店商品 ID 引用
- **Apple 服务端通知可达性与延迟受 Apple 控制**：sandbox 通知丢失/乱序是常态；客户端提交为主路径保证购买即时履约，定时拉取兜底漏发的后续事件，但不保证 100% 实时
- **Google 生命周期事件延迟以对账间隔为界**：无 RTDN，续费/退款/取消最迟在下一个拉取周期反映到本地；间隔须小于平台事件保留窗口
- **Apple App Store Server API / Google Play Developer API 的配额与历史查询窗口限制**：补偿对账间隔必须小于平台事件保留窗口，分页与限流避免触发平台限流

---

## 9. 参考资料

- 用户故事：`docs/user-stories/billing/support-iap.md`（US-IAP-001~006）
- 相关 PRD：`docs/prd/billing/subscription.md`（订阅计费，含支付尝试状态机与 webhook 补偿规则）
- 相关 PRD：`docs/prd/billing/stripe-payment.md`（Stripe 支付，provider 接入参考基线）
- 相关 PRD：`docs/prd/billing/points.md`（积分系统，topup_credit 发放模型）
- 相关 PRD：`docs/prd/billing/invoice.md`（发票系统，IAP 复用其 MoR 不出 Herald 发票的约束）
- 用户故事（通用支付尝试）：`docs/user-stories/billing/payment-attempt.md`（US-PA-001~004）
- 用户故事（通用支付平台配置）：`docs/user-stories/billing/payment-provider.md`（US-PV-001~005）
- 用户故事（Entitlement 映射）：`docs/user-stories/billing/entitlement-mapping.md`
- 用户故事（集成方前端充值/购买）：`docs/user-stories/integration/custom-user-ui.md`（US-CUI-008，移动 App 接入基线）
- Apple 官方文档：[App Store Server Notifications V2](https://developer.apple.com/documentation/appstoreservernotifications)、[App Store Server API](https://developer.apple.com/documentation/appstoreserverapi)、[StoreKit 2](https://developer.apple.com/documentation/storekit)
- Google 官方文档：[Google Play Developer API](https://developers.google.com/android-publisher)、[Google Play Billing](https://developer.android.com/google/play/billing)、[Real-time developer notifications](https://developer.android.com/google/play/billing/rtdn)（后续项，当前不接入）
