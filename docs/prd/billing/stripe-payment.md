# Stripe 支付集成产品需求文档 (PRD)

**创建时间**: 2026-03-20
**优先级**: P1

---

## 1. 相关用户故事

> 详细故事与验收标准请查看 `docs/user-stories/` 中对应文档。

| US-ID | 标题 | 优先级 | 来源 |
|-------|------|--------|------|
| US-PV-001 | 配置支付平台（含配置 Stripe Webhook 端点，场景 2 涵盖）——US-PP-001 已由支付平台产品管理取代 | P0 | `docs/user-stories/billing/payment-provider.md` |
| US-PV-002 | 查看支付平台配置——US-PP-002 已由支付平台产品管理取代 | P0 | `docs/user-stories/billing/payment-provider.md` |
| — | 使用 Stripe 支付订阅（通过 Stripe Checkout 实现，不在 Herald 前端） | — | — |
| — | 管理支付方式（通过 Stripe Customer Portal 实现，不在 Herald 前端） | — | — |

---

## 2. 范围界定

### 2.1 包含功能

- Stripe 作为支付平台选项之一（与 Creem 并列）
- Stripe 配置管理——通过通用 `realm_config` API（`/api/configs`，realm 由 admin 会话钉定，ConfigType::Stripe）统一管理，支持 api_key、webhook_secret、publishable_key、timeout、webhook_endpoint_id、async_points_strategy 配置项；base_url 仅用于测试替换 Stripe 服务地址，生产环境在配置写入层直接拒绝（400）
- 订阅支付处理（周期性计费：创建 Stripe Subscription → 处理首次支付 → 处理续费事件 → 取消订阅）
- 一次性支付处理（Payment Intents：创建 Payment Intent → 获取 Client Secret → 确认支付 → 处理支付结果）
- Webhook 事件处理（支付状态同步）
- 争议处理——`charge.dispute.created`/`charge.dispute.closed` 事件处理，标记订阅 Disputed 状态，争议解决后根据结果恢复或取消订阅（证据提交由 Stripe Dashboard 完成）
- 退款处理——`charge.refunded` 事件处理：topup 退款按单笔退款增量占原支付金额的比例回收积分（多次部分退款的累计回收对齐累计退款比例，同一退款单重复推送不二次回收），一次性购买的角色仅在累计退款达到原支付金额时回收（见 `docs/prd/billing/refund-clawback.md`）；subscription 退款在周期配额模型下按立即取消模式撤销订阅配额权益（不回收未使用积分）
- 支付历史记录查询

### 2.2 不包含功能

- 批量导入配置
- 平台健康检查与 Webhook 连接测试（本期均未提供专用测试端点）
- 其他支付网关的详细实现（Creem 是与 Stripe 并列的真实支付渠道，其集成语义分散承载于 subscription.md、invoice.md 等 PRD，本 PRD 不展开；其他平台需单独 PRD）
- 多币种转换（使用 Stripe 原生币种支持）
- 税务计算（使用 Stripe Tax 或后续集成）
- Disputes 证据提交（Herald 只处理争议状态标记、审计记录和权益/积分策略；证据提交由 Stripe Dashboard 完成）
- `payment_intent.payment_failed` 和 `invoice.payment_failed` 事件处理（✅ RESOLVED — 已在 `stripe_webhook_handlers.rs` 中实现）

### 2.3 依赖项

- 通用支付平台配置系统（见 Billing PRD）
- Billing 订阅计费系统（`docs/prd/billing/subscription.md`）
- Realm 管理系统
- 用户管理系统
- Stripe 账户和 API 密钥（需配置）

---

## 4. 业务规则与状态

### 4.1 业务规则

- **配置管理规则**：每个 Realm 可配置独立的 Stripe 账户；通过通用 `realm_config` API（`/api/configs`，realm 由 admin 会话钉定，ConfigType::Stripe）管理，配置项包括 api_key（Secret Key）、webhook_secret（Webhook Signing Secret）、publishable_key（Publishable Key）、timeout（HTTP 请求超时秒数）、webhook_endpoint_id（Webhook 端点 ID）
  - **配置项差异说明**：支持 `async_points_strategy`；`base_url` 仅供测试替换 Stripe 服务地址，生产环境在配置写入层直接拒绝（400，防 SSRF）。Account ID（`account_id`）未作为独立 config_key 实现，仅在 ConfigType::Stripe 注释中声明为可选键，全仓无读取方（可经通用 realm_config 写入但不被消费）；Herald 不解析或校验 `sk_test_*` / `sk_live_*` 前缀（密钥原样交给 Stripe，实际环境由 Stripe 密钥本身决定）。Webhook Endpoint URL 由 `public_base_url` 动态拼接，不作为独立配置项；`webhook_endpoint_id` 可经通用 realm_config 写入，但无任何读取方，仅作配置记录，当前不参与验签——验签以 `webhook_secret` 为准，不存在「用于校验（verification）」的用途
- **凭据存储**：凭据以 realm_config 明文存储并以 `is_secret` 标记（响应脱敏、不回显），应用层加密为后续统一工作（若所有 provider 凭据统一加密，Stripe 一并受益）
- **密钥脱敏**：Secret Key 查看时显示脱敏信息
- **编辑时密钥保留**：更新配置时，敏感字段（Secret Key、Webhook Secret）为可选，留空则保留旧值；非敏感字段正常更新
- **权限控制**：配置写入需 `settings.manage`，查看需 `settings.view`（Stripe 凭据走通用 realm_config 管理通道，与 WeChat/IAP 等现有 provider 共用统一的 Realm 配置权限面；`billing.*` 权限用于账单与产品管理面，不控制 provider 凭据配置）
- **删除保护**：删除前存在活跃订阅则拒绝删除；无活跃订阅时才可删除配置
- **数据隔离**：不同 Realm 的支付数据完全隔离；用户只能查看自己的支付历史；Realm Admin 只能查看所属 Realm 的支付数据
- **Webhook 事件处理**：验证 Stripe Signature（HMAC-SHA256 + 时间戳重放防护，见 §4.2）→ 解析事件类型 → 执行业务逻辑 → 更新本地状态 → 记录事件日志。事件覆盖：checkout.session.completed/expired/async_payment_succeeded/async_payment_failed、customer.subscription.created/updated/deleted/paused/resumed、charge.refunded、charge.dispute.created/closed、credit_note.created/updated/voided、payment_intent.succeeded、payment_intent.payment_failed、invoice.payment_succeeded、invoice.payment_failed、invoice.payment_action_required、invoice.created/finalized/paid/voided
  - checkout.session.completed（mode=payment）：为一次性购买创建 provider=stripe 的外部发票记录（与 Creem inline 同步模式一致）
  - checkout.session.expired：Checkout 会话过期未支付时标记 PaymentAttempt 为 failed
  - checkout.session.async_payment_*（延迟支付方式）：默认 Conservative 策略不会在未结算时履约，Realm 显式配置 Eager 时允许 `completed` 立即履约并承担异步失败后的回收风险
  - customer.subscription.paused/resumed：订阅暂停/恢复状态同步
  - invoice.payment_action_required：支付需额外操作（3D Secure 等）时记录日志
  - charge.dispute.created/closed：无法从 metadata 映射本地订阅时记录并忽略

### 4.2 关键状态与异常

- **支付失败处理**：返回用户友好的错误信息，支持支付重试（针对临时性错误），记录所有支付失败事件
- **Webhook 重试**：除依赖 Stripe 自身的重试发送策略外，代码对进程内处理实行最多 3 次的瞬态重试（`MAX_ATTEMPTS = 3`）；非瞬态失败仍依赖 Stripe 重发与补偿框架
- **安全约束**：API Key 不得暴露给前端（仅 Publishable Key 可暴露）；Webhook 端点必须验证 Stripe Signature；所有支付操作必须通过 HTTPS；支付敏感信息不得存储在本地数据库；支付生命周期以 `payment_event` 与支付账本追踪，配置变更进入统一审计日志
- **Webhook 签名验证**：使用 HMAC-SHA256 验证，签名格式为 `stripe-signature` 头中的 `t=...,v1=...`；包含时间戳重放攻击防护（15 分钟窗口，即 900 秒），拒绝过旧或未来时间戳的请求
- **Creem 回调防护模型（对照）**：Creem 回调仅做 HMAC-SHA256 验签（对原始请求体的 `creem-signature` 头），签名不含时间戳成分，因此没有 Stripe/WeChat 的 900 秒时间戳重放窗口；重放缓解依赖事件级幂等（payment_event 去重），防护弱于 Stripe/WeChat

---

## 5. 验收目标

- 持 `settings.manage` 的管理员可以创建、更新、删除 Stripe 配置；持 `settings.view` 可查看（默认角色下由 Realm Admin 承担）
- 一次性支付和订阅支付流程正常工作
- Webhook 事件正确处理并更新本地状态
- 支付历史可以正确查询、按时间和支付提供商筛选并分页
- 不同 Realm 的数据完全隔离
- 支付配置变更记录统一审计；购买与 Webhook 履约由 `payment_attempt`、`payment_event` 和账本记录追踪

---

## 6. 边界与约束

**适用性**: 适用（API 与前端/交互边界合并陈述）

**API / 集成边界:**
- 接口能力范围：计费、套餐、积分、支付配置、订阅变更、webhook 处理的能力边界；不在 PRD 中列出端点、schema 或状态码细节
- 支付历史查询：用户查看自己的成功支付历史；持 `billing.view` 的 Realm 管理员查看该 Realm 全部用户的成功支付记录（记录包含 `userId`）
- 访问控制原则：必须遵守 realm 隔离、管理员权限、金额与积分变更可追溯、回调幂等和失败补偿要求
- 兼容性要求：与支付平台、积分账本、订阅系统的详细契约应下沉到技术设计或接口说明

**前端 / 交互边界:**
- 管理入口：支付平台配置管理页面，持 `settings.view` / `settings.manage` 的管理员可管理 Stripe 配置
- 关键操作路径：配置创建表单、配置编辑（密钥轮换）、配置删除、支付历史查看
- 状态反馈：敏感信息脱敏显示、配置状态展示、操作成功/失败反馈
- 权限可见性：配置管理页面按 `settings.view` / `settings.manage` 权限控制访问
- 金额/积分变化：支付场景必须突出金额变化、变更影响范围、不可逆风险提示和回调同步中的状态说明

---

## 7. 已确认决策

- Stripe 支付用户体验在第三方应用中完成，Herald 只负责配置管理和 Webhook 处理
- Stripe 与 Creem 作为支付平台选项并列存在
- 复用通用支付平台配置系统

---

## 8. 参考资料

- 相关 PRD：`docs/prd/billing/subscription.md`
- Stripe 官方文档：[Stripe API](https://stripe.com/docs/api)、[Webhooks 指南](https://stripe.com/docs/webhooks)、[Stripe.js](https://stripe.com/docs/js)
- 用户故事来源见 §1 表格
