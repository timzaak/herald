# Google Pay / Apple Pay 钱包支付 产品需求文档 (PRD)

**创建时间**: 2026-08-31
**优先级**: P1

> 场景背景：Google Pay / Apple Pay 钱包支付是既有 Stripe / Creem 收款链路上的资金来源扩展，不是新支付渠道。钱包支付不引入新业务规则、不改变购买流程、不改变权益授予或订阅状态机。本文档不承载接口端点、请求/响应 schema、HTTP 状态码、数据库建表/迁移或代码类型定义；技术方案细节请参见对应技术设计。

---

## 1. 相关用户故事

> 详细故事与验收标准请查看 `docs/user-stories/` 中对应文档。

### 1.1 相关故事

本需求**不新增**用户故事。钱包支付的用户旅程、履约规则与权益授予完全由既有用户故事覆盖，钱包只是上述旅程中的资金来源之一：

**通用支付尝试生命周期（已发布，钱包复用）**，来源 `docs/user-stories/billing/payment-attempt.md`：
- `[US-PA-001]` 创建支付尝试（订阅或积分包） —— P0（移动 App PaymentIntent 流是对本故事的流程声明扩展）
- `[US-PA-002]` 查询支付尝试状态 —— P0
- `[US-PA-003]` 处理支付成功后的履约 —— P0
- `[US-PA-004]` 关闭过期的支付尝试 —— P1

**通用支付平台配置（已发布，钱包复用）**，来源 `docs/user-stories/billing/payment-provider.md`：
- `[US-PV-001]` 配置支付平台（Creem/Stripe） —— P0
- `[US-PV-002]` 查看支付平台配置 —— P0（Stripe 条目现包含非密钥 publishable key）

**集成方前端充值/购买旅程（已发布，移动 App 钱包支付复用）**，来源 `docs/user-stories/integration/custom-user-ui.md`：
- `[US-CUI-008]` 集成方前端完成充值/购买 —— P0（移动 App 通过浏览器 token 走同一旅程）

**Stripe 支付旅程（已发布，钱包场景的承载基线）**，来源 `docs/prd/billing/stripe-payment.md` 与相关用户故事：
- 一次性支付 / 订阅支付 / Webhook 履约 —— 由既有 Stripe 用户故事覆盖

> 关键边界（来自已发布 PRD `docs/prd/billing/stripe-payment.md`）：Stripe 支付的用户体验在第三方应用中完成，Herald 系统只负责配置管理和 Webhook 处理。钱包支付完全落在此边界内。

### 1.2 优先级汇总

本需求不新增故事，不产生新的优先级计数。本 PRD 的优先级 **P1** 反映其对核心购买流程的影响范围与多平台覆盖价值，而非新增故事数量。

---

## 2. 范围界定

### 2.1 包含功能

- **Web 端 Stripe 钱包支付**：在既有"跳转 Stripe Hosted Checkout"链路上，Stripe 账号侧启用钱包后，Apple Pay / Google Pay 自动出现在托管结算页；Herald 站内不渲染钱包按钮
- **Web 端 Creem 钱包支付**：在既有"跳转 Creem 托管结算页"链路上，Apple Pay / Google Pay 等钱包方式由 Creem Merchant-of-Record 托管能力提供
- **移动 App Stripe 钱包支付接入**：移动 App 集成方通过既有购买发起能力声明 PaymentIntent 流，获取真实 `client_secret`（仅限 Stripe 一次性购买），由客户端原生钱包 SDK（iOS PassKit / Android Google Pay）完成钱包确认
- **Stripe publishable key 暴露**：支付平台配置查看能力中，Stripe 条目返回非密钥的 publishable key 值，供移动 App 的 Stripe SDK 初始化使用；其他服务商条目不包含该信息
- **hosted 流凭证语义**：托管页跳转流（默认流）只返回托管页跳转地址，不返回 `client_secret`——托管 Checkout 会话不存在该凭证
- **配置 / 运维说明**：管理端与文档说明钱包可用性的前置条件（账号侧启用、买家设备 / 浏览器条件、Creem 地区 / 货币限制），避免被误判为"Herald 不支持"

### 2.2 不包含功能 (Out of Scope)

- **嵌入式 Stripe Elements / Payment Request Button**：不在 Herald React 站内渲染钱包按钮，不引入 Stripe 前端 SDK 依赖
- **Apple Pay 域名注册**：不实现支付方式域名自动注册。该注册仅对"商户自有域名嵌入 Stripe Elements"生效；托管页与移动 App 场景均不需要
- **Herald 移动 SDK**：不交付 iOS / Android 钱包 SDK，不托管各租户移动 App 的 Apple Merchant ID / Google Play 配置
- **新支付渠道**：钱包支付是既有 Stripe / Creem 链路上的资金来源，不新增第三种收款渠道
- **Stripe 账号侧钱包启用**：账号侧在 Stripe Dashboard 启用 Apple Pay / Google Pay 属运营 / 账号配置工作，不纳入 Herald 代码范围
- **钱包来源报表**：不新增支付资金来源字段；履约链路对资金来源透明，若后续报表需要再独立评估
- **前端功能性改动**：Web 端跳转、状态轮询、处理 / 完成反馈逻辑保持不变；API 生成客户端随契约自动更新
- **退款 / 争议的钱包专属规则**：钱包支付退款与争议沿用现有 Stripe / Creem 既有模型，不引入钱包专属规则

### 2.3 依赖项

- **既有 Billing 体系**：订阅计费（`docs/prd/billing/subscription.md`）、统一支付尝试与履约（`docs/user-stories/billing/payment-attempt.md`）、Stripe 支付（`docs/prd/billing/stripe-payment.md`）
- **既有自建用户 UI 集成模式**：移动 App 经浏览器 token + 购买发起 scope 接入（`docs/user-stories/integration/custom-user-ui.md` US-CUI-008）
- **既有自建 Stripe / Creem 客户端**：钱包支付不改变 TLS 路线，不引入 `native-tls` / `openssl`
- **Stripe / Creem 账号侧配置（运营前置）**：钱包可用性最终取决于 Stripe Dashboard 启用状态与买家设备 / 浏览器条件；Creem 钱包可用性受买家地区 / 货币（USD / EUR）限制
- **移动 App 集成方自有 Apple Developer / Google Play 账号**：移动端钱包确认所需的 Merchant ID 与钱包 SDK 由集成方负责

---

## 3. 需求概述

### 3.1 功能描述

为 Herald 多租户计费系统在现有 Stripe 与 Creem 收款链路上启用 Google Pay / Apple Pay 钱包支付能力，覆盖 Web 端与移动 App 端两类入口。钱包支付不引入新的业务规则或新的支付渠道，只是既有"跳转托管页 / 创建 PaymentIntent"链路上的一种资金来源；履约、回调幂等、凭据存储完全复用现有统一计费链路。

### 3.2 关键特性

- **零新依赖**：现有自建 Stripe / Creem 客户端、既有浏览器路由、现有 webhook 处理链路与现有履约服务足以覆盖；不引入任何前端或后端钱包 SDK
- **托管页原生钱包**：Web 端钱包支付发生在 Stripe / Creem 托管页，Herald 站内不渲染嵌入式钱包按钮
- **移动 App 显式声明流**：移动 App 在发起购买时声明 PaymentIntent 流，获取真实 `client_secret` 由客户端原生钱包 SDK 完成确认；未声明时默认走托管页跳转流，与既有 Web 行为完全一致
- **凭证暴露边界清晰**：`client_secret` 仅随购买发起响应一次性返回给该支付尝试的所有者，状态查询不重复返回；Stripe publishable key（非密钥）可在配置查看中返回，secret key / webhook secret 永不暴露
- **多租户隔离不变**：每个 Realm 仍使用独立 Stripe / Creem 配置，钱包可用性按 Realm 配置 + 账号侧启用分别生效
- **运维可观测**：管理端与运维文档明确说明钱包可用性的前置条件与限制，避免误判

---

## 4. 业务规则与状态

### 4.1 业务规则

**配置管理规则（复用既有，无新增）**：
- 每个 Realm 仍使用独立 Stripe / Creem 账户配置，复用既有配置管理界面
- 钱包支付不引入新的配置项；不需要 Apple Pay 域名注册相关字段
- 仅 Realm Admin（`settings.manage`）可修改配置；查看需 `settings.view`（provider 凭证配置走通用 realm_config 通道，与现有 provider 一致；`billing.*` 用于权益映射与发票面）

**流程声明规则（移动 App PaymentIntent 流）**：
- 客户端可在发起购买时声明托管页流（默认，含不声明）或 PaymentIntent 流
- PaymentIntent 流仅支持 Stripe + 一次性购买（积分包 / 买断）；订阅等周期性购买与其他服务商的 PaymentIntent 声明被拒绝并返回明确错误
- 未声明的流量与既有 Web 购买行为完全一致（hosted），存量集成方无感知

**凭证暴露规则**：
- `client_secret` 仅随购买发起响应返回给该支付尝试所有者且具备购买发起 scope 的请求方；非所有者请求不得获得他人支付尝试的 `client_secret`
- 状态查询能力不返回 `client_secret`（凭证只在发起时一次性下发）
- 托管页流不返回 `client_secret`——托管 Checkout 会话不存在该凭证，避免移动集成方误用
- publishable key 为非密钥信息，可在支付平台配置查看中按 Stripe 条目返回；secret key / webhook secret 永不出现在任何客户端可见响应中

**钱包可用性规则**：
- Web 端 Stripe 钱包可用性取决于：Stripe 账号侧已启用 Apple Pay / Google Pay + 买家设备 / 浏览器满足条件（Apple Pay 需 Safari + Apple 设备，Google Pay 需 Chrome）+ HTTPS
- Web 端 Creem 钱包可用性取决于：Creem 账号配置 + 买家地区 / 货币（USD / EUR）
- 移动 App Stripe 钱包可用性取决于：移动 App 集成方已配置 Apple Merchant ID / Google Pay，且已取得 `client_secret` 与 publishable key
- 移动 App Creem 钱包可用性取决于：Creem 托管页在该买家地区 / 货币下展示钱包选项
- 上述任一前置条件不满足时，Herald 不负责启用或兜底，相关说明由管理端配置界面与运维文档承载

**下单与履约规则（复用既有，无新增）**：
- 钱包支付作为托管页 / PaymentIntent 的一种资金来源，对履约链路透明
- 履约完全走既有统一链路：支付尝试成功 → 按购买类型（订阅 / 积分包 / 一次性积分）完成发放，provider 无关、资金来源无关
- 回调幂等复用既有支付事件表，钱包支付产生的回调事件类型与卡片支付一致，已由既有 Stripe / Creem webhook 处理覆盖

**数据隔离规则（复用既有，无新增）**：
- 不同 Realm 的 Stripe / Creem 配置、支付数据完全隔离
- Regular User 只能看到自己的支付与购买记录
- Realm Admin 只能查看与操作本 Realm 的配置与数据

### 4.2 关键状态与异常

**支付尝试状态**（复用既有，非钱包新增）：待支付、已成功、已失败、已过期；语义见 `docs/prd/billing/subscription.md` §4.2 与 `docs/user-stories/billing/payment-attempt.md`。

**异常场景**：
- **非法流程声明**：PaymentIntent 流声明用于非 Stripe 服务商或周期性购买时，请求被拒绝并返回明确错误（指明仅支持 Stripe / 仅支持一次性购买）；未知流程值同样被拒绝，不静默回退为托管页流——静默回退会让移动 App 拿到无法打开的托管页地址
- **钱包选项未在托管页出现**：买家设备 / 浏览器不满足条件，或 Stripe / Creem 账号侧未启用，或 Creem 地区 / 货币不支持。Herald 不负责启用或兜底；管理端配置界面与运维文档需说明前置条件
- **移动 App 取得 `client_secret` 后钱包确认失败**：移动 App 集成方钱包 SDK 自行处理失败反馈与重试；Herald 侧支付尝试保持待支付，回调或主动查询确认失败后按既有失败流程处理
- **回调延迟到达**：前端 / 移动 App 按支付尝试状态轮询，回调到达后状态更新；与既有 Stripe / Creem 行为一致

---

## 5. 功能需求

### 5.1 核心需求

**Web 端 Stripe 钱包支付（基于既有跳转链路）**：
- 在已配置 Stripe 的 Realm，用户选择套餐 / 积分包并发起支付时，沿用既有"跳转 Stripe Hosted Checkout"链路
- Stripe 托管结算页在账号侧启用钱包且买家条件满足时，自动展示 Apple Pay / Google Pay 选项
- 支付成功后按购买类型走既有统一履约

**Web 端 Creem 钱包支付（基于既有跳转链路）**：
- 在已配置 Creem 的 Realm，用户选择套餐 / 积分包并发起支付时，沿用既有"跳转 Creem 托管结算页"链路
- Creem 托管结算页在账号配置与买家地区 / 货币支持时，展示 Apple Pay / Google Pay / 卡片 / PayPal / SEPA 等方式
- 支付成功后按购买类型走既有统一履约

**移动 App Stripe 钱包支付接入**：
- 移动 App 集成方通过既有购买发起能力声明 PaymentIntent 流，获取真实 `client_secret` 与托管页流二选一
- 移动 App 用原生钱包 SDK（PassKit / Google Pay）凭 `client_secret` 完成钱包确认
- 支付成功后按购买类型走既有统一履约；履约对资金来源透明

**移动 App Creem 钱包支付接入**：
- 移动 App 集成方通过既有购买发起能力取得 Creem 托管页地址，以 WebView 打开 Creem 托管结算页完成钱包支付
- 支付成功后按购买类型走既有统一履约

**publishable key 暴露（Stripe 条目）**：
- 支付平台配置查看能力中，Stripe 条目返回该 Realm 已配置的 publishable key 值（`pk_` 前缀的非密钥信息）
- 其他服务商条目完全不包含该信息（无占位空值），避免移动集成方误判其他服务商支持 PaymentIntent 流

**配置与运维说明（轻量，非代码必需）**：
- 管理端 Stripe / Creem 配置入口附近提供钱包可用性前置条件说明（账号侧启用、买家设备 / 浏览器条件、Creem 地区 / 货币限制）
- 运维文档说明移动 App 集成方责任边界（Apple Merchant ID / Google Play 配置、钱包 SDK）

**统一履约复用**：
- 支付成功后按购买类型（订阅 / 积分包 / 一次性积分）走既有统一履约，不在钱包场景新建独立订单表或履约服务
- 复用既有支付事件表保证回调幂等

### 5.2 验收目标

- 在 Stripe 账号侧已启用 Apple Pay / Google Pay 且买家条件满足的 Realm，Web 用户在 Stripe 托管结算页能看到并完成钱包支付，支付成功后权益按购买类型正确发放
- 在 Creem 账号配置与买家地区 / 货币支持的 Realm，Web 用户在 Creem 托管结算页能看到并完成钱包支付，支付成功后权益按购买类型正确发放
- 移动 App 集成方声明 PaymentIntent 流 + Stripe + 一次性购买时获得真实 `client_secret`（可被钱包 SDK 使用），且不返回托管页地址；`client_secret` 仅返回给该支付尝试所有者
- PaymentIntent 流声明用于非 Stripe 服务商或周期性购买时被明确拒绝；未知流程值被拒绝且不静默回退
- 托管页流（默认）不返回 `client_secret`；状态查询永不返回 `client_secret`
- 支付平台配置查看中 Stripe 条目包含已配置的 publishable key 值；secret key / webhook secret 不出现在任何客户端可见响应中；其他服务商条目不含 publishable key 字段
- 重复回调不会重复发放权益或重复创建订阅（复用既有幂等保证）
- 全栈不引入新的前端或后端钱包 SDK；rustls / 去 openssl 路线不被破坏
- 不同 Realm 的钱包支付可用性按各自 Stripe / Creem 配置独立生效，互不影响
- 所有支付操作记录审计日志

---

## 6. API 相关约束

**适用性**: 适用（约束级，不承载端点 / schema / 状态码细节）

- **接口能力范围**：钱包支付不引入新的接口端点。Web 端复用既有"创建支付尝试 → 跳转 Stripe / Creem 托管页 → webhook 履约"链路；移动 App 复用既有购买发起、凭证返回、状态查询与 webhook 履约能力，仅扩展"流程声明"与"Stripe 条目 publishable key"两个既有能力面。Herald 不暴露钱包专属端点。
- **访问控制原则**：
  - 必须遵守 realm 隔离与既有购买发起 / 状态查询 / 配置查看 scope 鉴权
  - `client_secret` 仅返回给该支付尝试所有者且具备购买发起 scope 的请求方；非所有者请求不得获得他人支付尝试的 `client_secret`
  - 配置写入需 `settings.manage`，查看需 `settings.view`
  - 金额与积分变更必须可追溯；回调必须幂等
- **租户 / realm 边界**：每个 Realm 使用独立的 Stripe / Creem 配置，钱包可用性按 Realm 配置 + 账号侧启用分别生效；与既有 Stripe / Creem 边界一致。
- **兼容性要求**：不声明流程的既有客户端行为完全不变（默认托管页流）。与 Stripe、Creem、积分账本、订阅系统的详细契约下沉到技术设计或接口说明。Herald 不交付移动 SDK；移动 App 端钱包 SDK 与 Apple Merchant ID / Google Play 配置由各客户端集成方负责。

---

## 7. 前端/交互约束

**适用性**: 适用（约束级，无功能性前端改动）

- **管理入口**：支付平台配置管理页面，Realm Admin 管理 Stripe / Creem 配置（与现有入口一致）。钱包可用性前置条件说明应出现在该入口附近。
- **关键操作路径**：无新增前端路径。Web 用户购买路径保持"选包 → 选服务商 → 跳转 Stripe / Creem 托管页 → 处理 → 完成"，钱包选项在托管页展示，Herald 站内不渲染钱包按钮。
- **状态反馈**：跳转、处理中、成功、过期、失败的反馈沿用既有购买向导；敏感信息脱敏显示、配置状态展示沿用既有 Stripe / Creem 配置表单。
- **权限可见性**：仅 Realm Admin 可访问配置管理页面；终端用户在 Realm 已配置 Stripe / Creem 时看到对应服务商选项，钱包选项可见性由托管页根据账号侧与买家条件决定。
- **金额 / 积分变化**：支付场景沿用既有金额变化与不可逆风险提示。
- **移动 App 交互**：移动 App 集成方自行实现钱包按钮、唤起、确认与失败反馈；Herald 仅提供下单、凭证透传与状态查询能力。

---

## 8. 参考资料

- 技术预研：`.ai/tech-research/support-googlepay-applepay.md`
- 相关 PRD：`docs/prd/billing/stripe-payment.md`（Stripe 支付，钱包场景的承载基线）
- 相关 PRD：`docs/prd/billing/subscription.md`（订阅计费，含 One-time 购买规则与支付尝试状态机）
- 相关 PRD：`docs/prd/billing/points.md`（积分系统）
- 用户故事（通用支付尝试）：`docs/user-stories/billing/payment-attempt.md`（US-PA-001～004）
- 用户故事（通用支付平台配置）：`docs/user-stories/billing/payment-provider.md`（US-PV-001～005）
- 用户故事（集成方前端充值/购买）：`docs/user-stories/integration/custom-user-ui.md`（US-CUI-008，移动 App 接入基线）
- 同类支付渠道 PRD：`docs/prd/billing/wechat-support.md`（独立立项，互不依赖）
- Stripe 官方文档：[Apple Pay（Web）](https://docs.stripe.com/apple-pay?platform=web)、[Register domains for payment methods](https://docs.stripe.com/payments/payment-methods/pmd-registration)
- Creem 官方文档：[Supported Payment Methods](https://docs.creem.io/merchant-of-record/finance/payment-methods)
