# WeChat Pay 支持产品需求文档 (PRD)

**创建时间**: 2026-07-26
**优先级**: P1

> 场景背景：WeChat Pay v3 是面向微信生态的收款渠道，与 Stripe、Creem、App Store / Google Play 内购（IAP）并列，覆盖微信用户的订阅与积分包购买。本文档不承载接口端点、请求/响应 schema、HTTP 状态码、数据库建表/迁移或代码类型定义；技术方案细节请参见对应技术设计。

---

## 1. 相关用户故事

> 详细故事与验收标准请查看 `docs/user-stories/` 中对应文档。

### 1.1 相关故事

**WeChat Pay 特有**，来源 `docs/user-stories/billing/wechat-support.md`：
- `[US-WP-001]` 配置 WeChat Pay 凭据，优先级 P0，角色 Realm Admin
- `[US-WP-002]` PC 扫码 Native 支付，优先级 P0，角色 Regular User
- `[US-WP-003]` 微信内 JSAPI 唤起支付，优先级 P1，角色 Regular User
- `[US-WP-004]` WeChat 回调验签、解密与幂等履约，优先级 P0，角色 System
- `[US-WP-005]` 平台证书自动获取与刷新，优先级 P0，角色 System

**通用支付平台配置（已发布，WeChat 复用）**，来源 `docs/user-stories/billing/payment-provider.md`：
- `[US-PV-001]` 配置支付平台（Creem/Stripe）—— P0，WeChat 场景由 US-WP-001 细化
- `[US-PV-002]` 查看支付平台配置 —— P0
- `[US-PV-003]` 编辑支付平台配置 —— P1
- `[US-PV-004]` 删除支付平台配置 —— P1

**通用支付尝试生命周期（已发布，WeChat 复用）**，来源 `docs/user-stories/billing/payment-attempt.md`：
- `[US-PA-001]` 创建支付尝试 —— P0
- `[US-PA-002]` 查询支付尝试状态 —— P0
- `[US-PA-003]` 处理支付成功后的履约 —— P0，WeChat 场景由 US-WP-004 细化
- `[US-PA-004]` 关闭过期的支付尝试 —— P1

### 1.2 优先级汇总

| 优先级 | 数量（WeChat 特有） | 关键故事 |
|--------|------|----------|
| P0 | 4 | 配置 WeChat Pay 凭据、PC 扫码 Native 支付、回调验签解密与幂等履约、平台证书自动获取与刷新 |
| P1 | 1 | 微信内 JSAPI 唤起支付 |
| P2 | 0 | - |

> 通用配置 CRUD 与支付尝试生命周期的优先级见各自来源文件，此处不重复汇总。

---

## 2. 范围界定

### 2.1 包含功能

- WeChat Pay v3 作为与 Stripe、Creem、IAP（App Store / Google Play）并列的收款渠道接入
- 两类支付场景：
  - **Native（PC 扫码）**：PC 浏览器内生成 WeChat 收款二维码、前端轮询支付状态、扫码支付成功后履约
  - **JSAPI（微信内网页/小程序唤起）**：在微信生态内直接唤起微信支付完成付款
- WeChat Pay 凭据的多租户配置（每 Realm 独立）：appId、mchId、商户私钥、证书序列号、APIv3 Key、回调通知地址
- 微信平台证书运行时自动获取与过期前自动刷新（无需手工预置）
- WeChat 回调接收：平台证书 RSA-SHA256 验签、APIv3 Key 的 AES-256-GCM 解密、回调幂等
- 履约完全复用既有统一链路：`payment_attempt` → 统一履约（订阅 / 积分包 / 一次性积分），不另建独立订单表或独立履约服务
- 回调幂等复用既有 `payment_event` 表
- 凭据存储复用既有 `realm_config`（与 Stripe/Creem 同表同模式）
- 前端在购买入口新增 WeChat 分支：Native 渲染二维码 + 状态轮询，JSAPI 唤起支付

### 2.2 不包含功能 (Out of Scope)

- **WeChat OAuth 登录链路改造**：JSAPI 所需 openid 由调用方通过既有微信登录链路取得并随下单请求传入；获取 openid 本身不在本期支付渠道范围（见 `docs/prd/auth/wechat-oauth.md`）
- **私钥/凭据的应用层加密**：本期沿用 `realm_config` 现有明文存储 + `is_secret` 标记，与现有 Stripe/Creem 凭据一致；后续若所有 provider 凭据统一加密，WeChat 一并受益
- **WeChat 托管产品目录同步**：WeChat Pay v3 无等价的产品目录概念，本期跳过 provider 产品同步流程；entitlement mapping 中 WeChat 的 external_product_id/价格由管理员手工配置
- **H5 支付、小程序支付以外的其他 WeChat Pay 场景**（如 App 支付、付款码支付）
- **WeChat 侧的退款/争议处理**：本期不实现退款回调与争议状态（沿用现有 Stripe/Creem 退款模型时再统一规划）
- **WeChat 自动续费（委托代扣）**：WeChat 委托代扣（商户按周期主动扣款的自动续费）延后为独立 feature；本期 WeChat 订阅型产品为固定期、单次付款、到期后需用户重新购买，系统不自动扣款（订阅建模见 §8.1 与 `docs/prd/billing/pay_model.md`）
- **第三方 WeChat Pay SDK**：不引入任何会拉入 native-tls/openssl 的第三方 SDK（见 §8.2）

### 2.3 依赖项

- **既有 Billing 体系**：订阅计费（`docs/prd/billing/subscription.md`）、积分系统（`docs/prd/billing/points.md`）、统一支付尝试与履约（`docs/user-stories/billing/payment-attempt.md`）
- **非续期订阅建模**：WeChat 订阅型产品复用履约模型扩展引入的非续期订阅（`docs/prd/billing/pay_model.md`）
- **既有 Stripe/Creem 集成模式**：作为新增 WeChat 渠道的结构与边界参考（`docs/prd/billing/stripe-payment.md`）
- **既有微信 OAuth 登录**：JSAPI 场景的 openid 来源（`docs/prd/auth/wechat-oauth.md`）
- **Realm 管理与权限系统**：配置写入需 `settings.manage`，查看需 `settings.view`（与 Stripe/Creem 等所有支付平台配置共用统一的 Realm 配置权限面；`billing.*` 权限用于账单与产品管理面）
- **微信商户资质**：Realm Admin 需自行在微信支付平台完成商户入驻并取得凭据

---

## 3. 需求概述

### 3.1 功能描述

WeChat Pay 支持为 Herald 系统新增面向微信生态的收款能力，让使用微信的终端用户能在 PC 扫码或微信内网页中完成订阅与积分包购买。WeChat Pay 作为与 Stripe、Creem、IAP 并列的支付渠道接入；其履约、回调幂等、凭据存储完全复用现有统一计费链路，不引入独立订单表或独立履约服务。

### 3.2 关键特性

- **双场景收款**：PC 扫码（Native）与微信内唤起（JSAPI）覆盖微信生态主要购买路径
- **多租户隔离**：每个 Realm 配置独立的 WeChat 商户凭据，互不影响
- **平台证书自动维护**：运行时自动下载与刷新微信平台证书，免除手工运维
- **统一履约复用**：不重复建设订单表与履约逻辑，与 Stripe/Creem 共用同一套支付尝试与履约链路
- **零新依赖**：用现有 workspace 加密栈实现验签/解密，不引入任何会破坏 rustls 全栈迁移的第三方 SDK

---

## 4. 业务规则与状态

### 4.1 业务规则

**配置管理规则**：
- 每个 Realm 可配置独立的 WeChat 商户账户；通过通用支付平台配置能力管理（与 Stripe/Creem 同一配置入口与模式）
- WeChat 特有配置项：appId、mchId、商户私钥（PEM 文本）、证书序列号、APIv3 Key、回调通知地址；平台公钥可选，手工配置时作为验签覆盖来源，未配置时由系统运行时自动下载平台证书用于验签
- 敏感字段（商户私钥、APIv3 Key）必须以 `is_secret` 标记存储；查看时显示脱敏信息
- 编辑时敏感字段为可选，留空则保留现有值；非敏感字段正常更新（与现有 Stripe/Creem 行为一致）
- 只有持 `settings.manage` 的管理员可更新、持 `settings.view` 可查看 WeChat 配置（与其他 Realm 配置共用统一配置权限面）
- 删除配置前必须检查是否有活跃订阅，存在活跃订阅时拒绝删除（与现有 provider 删除保护一致）

**平台证书规则**：
- 平台证书由系统运行时按需从微信自动下载并本地缓存，不要求管理员手工预置
- 系统校验证书有效期，在过期阈值前自动重新下载替换
- 平台证书用于 WeChat 回调验签；证书缺失、过期或下载失败时回调被拒绝，错误通过请求失败与服务端结构化日志暴露；当前无独立的证书临期告警指标

**下单与履约规则**：
- 下单复用统一 `payment_attempt`：商户订单号（`out_trade_no`）写入支付尝试的 provider 引用，回调按它反查 attempt
- 金额以整数分表示（WeChat 协议要求），与现有 Stripe/Creem 的最小货币单位口径一致
- Native 二维码有效期与支付尝试过期时间取一致（不超过 WeChat 单订单 2 小时上限）
- 履约完全走既有统一链路：支付尝试成功 → 按购买类型完成发放，provider 无关
- WeChat 订阅型产品以非续期订阅履约：单次付款、有效期固定、到期后需用户重新购买，系统不自动扣费（积分包/买断为一次性发放）；与 Stripe 订阅的自动续费语义不同（非续期订阅语义见 `docs/prd/billing/pay_model.md`）
- JSAPI 下单必须校验调用方传入的 openid；缺失 openid 时拒绝下单
- 下单 `payment_scene` 仅接受 `native` / `jsapi`；未知 `payment_scene` 值将被拒绝（400），不静默回退
- WeChat 不参与既有 provider 产品目录同步流程（无托管目录概念）

**回调与幂等规则**：
- WeChat 回调必须用缓存的平台证书进行 RSA-SHA256 验签；验签失败拒绝处理
- 回调签名时间戳仅接受服务器当前时间前后 900 秒的窗口，超出窗口按重放风险拒绝
- 回调密文必须用 APIv3 Key 进行 AES-256-GCM 解密
- 回调按外部事件 ID 幂等，复用既有 `payment_event` 表（key: `(external_event_id, "wechat")`）
- 回调金额必须与本地支付尝试记录的金额一致，不一致时拒绝履约并记录诊断
- 回调接收层不在同一 HTTP 请求内重试；已通过验签并落入 `payment_event`、但业务处理失败的事件由 worker 每 5 分钟自动扫描重试，同时仍可依赖微信重发
- 回调事件纳入既有 Webhook 补偿入口，支持自动或人工重放；重放同样幂等

**数据隔离规则**：
- 不同 Realm 的 WeChat 配置、支付数据完全隔离
- Regular User 只能看到自己的支付与购买记录
- Realm Admin 只能查看与操作本 Realm 的 WeChat 配置与数据

**安全约束**：
- 商户私钥、APIv3 Key 不得暴露给前端
- 回调端点必须验证微信签名
- 所有支付操作必须通过 HTTPS
- 已认证或已验签的 WeChat 配置变更与支付处理结果记录系统审计：配置写入/删除、履约成功、非成功状态、金额不符等拒绝分支及补偿重放。验签/解密前即失败的未认证请求只写结构化运行日志，避免外部伪造请求污染租户审计流

### 4.2 关键状态与异常

**支付尝试状态**（复用既有，非 WeChat 新增）：待支付、已成功、已失败、已过期；语义见 `docs/prd/billing/subscription.md` §4.2 与 `docs/user-stories/billing/payment-attempt.md`。

**异常场景**：
- 二维码过期：前端停止轮询并提示"二维码已过期"，提供重新获取入口
- 扫码后支付失败：前端展示支付失败并提供重新支付入口，不发放任何权益
- 回调延迟到达：前端按支付尝试状态轮询，回调到达后状态更新；与既有 Stripe/Creem 行为一致
- 平台证书过期未及时刷新：回调验签全失败、支付中断；需监控证书剩余有效期并提前触发刷新
- 签名验证失败、金额不符、解密失败：拒绝处理，不更新支付状态，不触发履约
- JSAPI 缺少 openid：拒绝下单并提示需先完成微信登录

---

## 5. 功能需求

### 5.1 核心需求

**WeChat 配置管理**：
- Realm Admin 可创建、查看（脱敏）、更新、删除本 Realm 的 WeChat Pay 配置（与现有 Stripe/Creem 同一配置入口与模式）
- 配置创建后 WeChat Pay 出现在本 Realm 可用支付平台列表中
- 删除受活跃订阅保护

**平台证书维护**：
- 系统在首次需要验签时自动从微信下载平台证书并缓存
- 系统在证书接近过期时自动重新下载替换
- 证书异常通过请求错误与服务端结构化日志诊断；当前无主动临期告警，手工配置的平台公钥可作为验签覆盖兜底

**Native（PC 扫码）支付**：
- 用户在 PC 浏览器选择支持 WeChat Pay 的套餐或积分包并发起支付
- 系统创建支付尝试并取得 WeChat 收款二维码（`code_url`）
- 前端渲染二维码、倒计时，并轮询支付尝试状态
- 支付成功后按购买类型履约；过期/失败给出口径一致的重新支付或重新获取入口

**JSAPI（微信内）支付**：
- 调用方传入 openid 后，系统创建支付尝试并取得 JSAPI 预支付参数
- 前端用预支付参数唤起微信支付
- 支付成功后按购买类型履约；缺失 openid 时拒绝下单

**回调处理**：
- 接收 WeChat 回调 → 用平台证书验签 → 用 APIv3 Key 解密 → 按外部事件 ID 幂等 → 按商户订单号定位支付尝试 → 校验金额 → 更新状态并触发统一履约
- 验签失败、解密失败、金额不符、重复回调均按 §4.1 规则处理

**统一履约复用**：
- 支付成功后按购买类型（订阅 / 积分包 / 一次性积分）走既有统一履约，不在 WeChat 侧新建独立订单表或履约服务
- 复用既有 `payment_event` 表保证回调幂等

### 5.2 验收目标

- Realm Admin 可创建、查看（脱敏）、更新、删除 WeChat Pay 配置
- 系统可自动获取并刷新微信平台证书，回调验签不因证书过期中断
- Native 下单 → 扫码 → 回调 → 履约端到端成功
- JSAPI 唤起 → 支付 → 履约端到端成功
- 重复回调不会重复发放权益或重复创建订阅
- 验签失败、金额不符、解密失败的回调被拒绝且不触发履约
- JSAPI 缺失 openid 时下单被拒绝
- 不同 Realm 的 WeChat 配置与支付数据完全隔离
- 所有已认证的配置变更与已验签支付处理记录审计日志；验签/解密前的拒绝仅进入运行日志

---

## 6. API 相关约束

**适用性**: 适用

- **接口能力范围**：WeChat 配置的读写复用通用支付平台配置能力；下单复用统一支付尝试发起能力；回调接收为 WeChat 专用的公共端点（与 Stripe/Creem 的公共回调端点并列）；履约复用统一履约链路。不在 PRD 中列出端点、schema 或状态码细节。
- **访问控制原则**：必须遵守 realm 隔离；配置写入需 `settings.manage`，查看需 `settings.view`（与所有支付平台配置共用统一 Realm 配置权限面）；回调端点为公共端点但必须通过微信签名验证；金额与积分变更必须可追溯；回调必须幂等。
- **租户/realm 边界**：每个 Realm 使用独立的 WeChat 回调地址，realm_id 从路径提取实现多租户隔离（与 Stripe/Creem 一致）。
- **兼容性要求**：WeChat 不参与既有 provider 产品目录同步；entitlement mapping 中 WeChat 的 external_product_id/价格由管理员手工配置。与微信 API、积分账本、订阅系统的详细契约应下沉到技术设计或接口说明。

---

## 7. 前端/交互约束

**适用性**: 适用

- **管理入口**：支付平台配置管理页面，Realm Admin 可管理 WeChat Pay 配置（与 Stripe/Creem 同一入口）
- **关键操作路径**：WeChat 配置创建表单（含商户凭据与回调地址）、配置编辑（密钥轮换、可选平台公钥覆盖）、配置删除
- **用户购买路径（Native）**：用户在 PC 浏览器选择套餐/积分包 → 选择 WeChat Pay → 展示二维码与倒计时 → 轮询状态 → 成功/过期/失败反馈
- **用户购买路径（JSAPI）**：用户在微信内网页选择套餐/积分包 → 选择 WeChat Pay → 唤起微信支付 → 成功/失败反馈；openid 由集成方经页面 URL 参数传入，缺少 openid 时禁用或拒绝下单并提示需先完成微信登录
- **状态反馈**：敏感信息脱敏显示、二维码倒计时、支付成功/过期/失败的明确反馈、回调同步中的状态说明；平台证书由系统自动维护，证书异常经运行诊断发现，手工配置的平台公钥作为兜底覆盖
- **权限可见性**：仅 Realm Admin 可访问 WeChat 配置管理；终端用户仅在 Realm 已启用 WeChat 且所选套餐/积分包已配置 WeChat 映射时看到 WeChat Pay 选项
- **金额/积分变化**：支付场景必须突出金额变化与不可逆风险提示

---

## 8. 已确认决策

### 8.1 范围与场景决策

- **支付场景**：本期实现 Native（PC 扫码）与 JSAPI（微信内网页/小程序唤起）两类场景；不实现 H5、App、付款码等其他 WeChat Pay 场景
- **订阅建模与自动续费延后**：WeChat 订阅型产品用非续期订阅表达（固定有效期、单次付款、可重复购买），履约生成真实订阅记录；WeChat 自动续费（委托代扣）延后为独立 feature，本期不实现商户周期性主动扣款
- **履约与幂等复用**：履约完全走既有支付尝试统一链路，不引入独立订单表或独立履约服务；回调幂等复用既有 `payment_event` 表，不另建幂等存储
- **JSAPI openid 来源**：JSAPI 的 openid 由调用方（前端/集成方）通过既有微信登录链路取得并随下单请求传入；获取 openid 不在本期支付渠道范围。微信内置浏览器内以页面 URL 参数作为 openid 的显式传入契约，缺参时不派发订单并提示需先完成微信登录
- **WeChat 产品目录同步跳过**：WeChat 无托管产品目录，entitlement mapping 中 WeChat 的 external_product_id/价格由管理员手工配置

### 8.2 技术与依赖决策（约束 PRD 边界，不承载实现细节）

- **不引入第三方 WeChat Pay SDK**：直接用现有 HTTP 客户端（rustls）+ 纯 Rust 加密栈自建，与现有 Stripe/Creem 的自建 provider 模式一致
- **不得引入 openssl / native-tls**：这是当前 rustls 全栈 TLS 栈下不可破坏的硬约束
- **平台证书自动获取**：运行时按需下载并本地缓存，过期前自动刷新，不要求手工预置；平台公钥配置项保留为手工覆盖兜底
- **凭据存储**：商户私钥（PEM）与 APIv3 Key 存入既有 `realm_config`，沿用 `is_secret` 标记；本期不实现私钥应用层加密（与现有 Stripe/Creem 凭据一致）

### 8.3 显式假设（列为约束）

- **私钥应用层加密不在本期范围**：假设 `realm_config` 现有明文存储（`is_secret` 标记）在本期可接受；后续若所有 provider 凭据统一加密，WeChat 一并受益
- **JSAPI openid 获取不在新代码内实现**：假设调用方通过既有微信登录链路取得 openid 并随下单请求传入
- **WeChat 产品目录同步跳过**：假设管理员手工配置 entitlement mapping 中 WeChat 的 external_product_id/价格

---

## 9. 参考资料

- 用户故事（WeChat 特有）：`docs/user-stories/billing/wechat-support.md`（US-WP-001~005）
- 用户故事（通用配置）：`docs/user-stories/billing/payment-provider.md`
- 用户故事（通用支付尝试）：`docs/user-stories/billing/payment-attempt.md`
- 相关 PRD：`docs/prd/billing/subscription.md`（订阅计费）
- 相关 PRD：`docs/prd/billing/pay_model.md`（非续期订阅与买断建模）
- 相关 PRD：`docs/prd/billing/stripe-payment.md`（同类支付渠道参考）
- 相关 PRD：`docs/prd/billing/points.md`（积分系统）
- 相关 PRD：`docs/prd/auth/wechat-oauth.md`（JSAPI openid 来源链路）
- WeChat Pay v3 官方文档：[支付场景](https://pay.weixin.qq.com/doc/global/v3/en/4012356799)
