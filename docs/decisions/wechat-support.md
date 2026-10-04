# wechat-support Decision Log

> 来源：`.ai/tech-research/wechat-support.md`（2026-07-26 首版，2026-08-13 刷新）+ 2026-08-13 `/t-tech-research wechat-support` 用户裁决 + 2026-08-13 `/t-design wechat-support` 新增 D2 决策（DEC-010/011）。
> 背景：WeChat Pay 曾完整实现后于 `df67772e` 等提交整体删除；2026-07-26 完成再接入技术预研；2026-08-13 对齐 pay_model 重构（引入 `BillingType::NonRenewing`）与当前代码库，并联网核实 WeChat 订阅能力后收敛范围。

## Active Decisions

| ID | Level | Topic | Decision | Rationale | Decided By | Source | Affects | Reopen When | Supersedes |
|---|---|---|---|---|---|---|---|---|---|
| DEC-wechat-support-001 | D1 | scope.payment-scenarios | 本期实现 Native（PC 扫码）+ JSAPI（微信内）两类场景，均走统一下单；不实现 H5/App/付款码/委托代扣 | 用户在 07-26 与 08-13 两轮确认；覆盖微信生态主要购买路径 | user | conversation (07-26 + 08-13) | prd/design/task | 需要覆盖 H5/App 场景或启动委托代扣 | — |
| DEC-wechat-support-002 | D1 | scope.recurring-deferred | 自动续费（委托代扣 / Recurring）延后为独立后续 feature，本期不实现；本期"订阅型"产品用 NonRenewing 表达 | 委托代扣是独立 API 流程（签约→预约扣费→受理扣款），需商户资质（企业/政府/事业单位/社会组织，不含个体工商户）+ 扣款调度 worker，复杂度约为统一下单的 2–3 倍；本期先收敛统一下单 | user | conversation (08-13 AskUserQuestion) | prd/design/task | 启动委托代扣 feature | — |
| DEC-wechat-support-003 | D2 | fulfillment.subscription-model | WeChat 订阅型产品用 `BillingType::NonRenewing` + `service_duration_days`，履约生成 Subscription 行（固定 `current_period_end` + `cancel_at`，可重复购买）；积分包/买断用 `OneTime` | pay_model 已引入 NonRenewing（DEC-pay_model-002）；WeChat 统一下单无法自动续费，NonRenewing 正好表达"固定期、单次付款"；复用全部订阅查询/管理视图。该选择改变 PRD §8.1 旧表述，已交回 `/t-prd` 更新（见 Deferred） | agent | docs/decisions/pay_model.md DEC-pay_model-002；本报告 §3 | prd/design/task | 委托代扣落地后需要真 Recurring 语义 | PRD §8.1 "recurring 以一次性付款完成履约" 旧表述 |
| DEC-wechat-support-004 | D1 | dependency.no-openssl | 不得引入 openssl / openssl-sys / native-tls / wechat-pay-rust-sdk 或任何间接拉入 native-tls 的依赖；用 workspace `reqwest`（rustls）+ 纯 Rust 加密（rsa/sha2/aes-gcm/pem）自建 | rustls 全栈迁移后的硬约束；第三方 wechat-pay-rust-sdk 的 reqwest 未关 default-features 而拉入 native-tls，是历史删除根因 | user | conversation (07-26) | prd/design/task | 出现官方维护且兼容 rustls 的纯 Rust WeChat SDK | — |
| DEC-wechat-support-005 | D1 | fulfillment.reuse-unified | 履约完全复用 `payment_attempt` → `complete_succeeded_payment_attempt` → `FulfillmentService`；回调幂等复用 `payment_event`；不另建订单表或独立履约服务 | 现架构已统一；旧的 `wechat_payment_order` 表与 `WechatSubscriptionService` 基于已废弃计费架构，不还原 | user | conversation (07-26) | prd/design/task | 统一履约链路无法表达 WeChat 特有履约需求 | — |
| DEC-wechat-support-006 | D1 | product.catalog-skip | WeChat 无托管产品目录：跳过 provider 产品同步（`fetch_products` 对 wechat 返回空为无害成功）；管理员手工建 entitlement_mapping（`external_product_id` 自由字符串 + `provider_product_info` JSONB 存价格/币种） | WeChat Pay v3 是订单制（统一下单传金额），协议上无 Product/Price 概念；Herald 已原生支持"无目录 provider"（`external_price_id` 可空、价格在 JSONB、Stripe 已有 price-less 回退） | user | conversation (07-26 确认) + 08-13 代码核实（provider_product_api.rs、purchase_service.rs resolve_target） | prd/design/task | WeChat 推出官方产品目录 API | — |
| DEC-wechat-support-007 | D2 | credential.storage | 商户私钥（PEM）与 APIv3 Key 存既有 `realm_config`（`config_type='wechat'`，`is_secret=true`）；本期不做应用层加密 | 与现有 Stripe/Creem 凭据存储一致；不改变技术路线/依赖/兼容性；后续若全 provider 凭据统一加密，WeChat 一并受益 | agent | 本报告 §6.2 | prd/design/task | 全 provider 凭据统一加密立项 | — |
| DEC-wechat-support-008 | D2 | cert.platform-cache | 平台证书运行时按需下载（GET /v3/certificates，APIv3 Key 解密）+ moka 内存缓存 + 过期阈值前自动重下载 | 免手工运维；moka 已在依赖树；平台证书用于回调验签，过期会导致验签全失败 | agent | 本报告 §4.3 | design/task | 缓存方案无法满足多实例一致性 | — |
| DEC-wechat-support-009 | D1 | jsapi.openid-source | JSAPI 所需 openid 由调用方（已登录链路）通过既有微信 OAuth 取得并随下单请求传入；本期支付代码不实现登录态获取 | openid 获取属既有登录能力（api-oauth/wechat），非支付渠道职责；支付接口只负责"拿到 openid 后下单" | user | conversation (07-26) | prd/design/task | openid 无法由登录链路可靠取得 | — |
| DEC-wechat-support-010 | D2 | fulfillment.wechat-via-unified-attempt | WeChat 下单走统一 `create_payment_attempt` 路径（在 `CreatePaymentAttemptRequest` 增 `paymentScene`/`openid`，在 `build_payment_context` 增 `"wechat"` 分支），不走 IAP `submit_iap_receipt` 路径 | WeChat 是 web checkout 流程（建 attempt → 渲染 code_url/JSAPI 参数 → 轮询），与 stripe/creem 同形；IAP receipt 路径面向原生商店票据，前端 web 根本不消费 `submitIapReceipt`；IAP 独立 `create_iap_payment_attempt` 仅因跳过 build_payment_context，WeChat 无需跳过 | agent | .ai/design/wechat-support.md §4.1 取舍 1、§5.3（2026-08-13 `/t-design`） | task | WeChat 需要消费原生商店票据语义 |
| DEC-wechat-support-011 | D2 | api.payment-context-shape | `PaymentContext` 沿用 flat provider 字段模式，新增 `wechat_code_url` + `wechat_jsapi_params`（嵌套结构体），不引入通用 `provider_payload` JSONB 字段 | 与现有 `stripe_checkout_url`/`creem_checkout_url`/`client_secret` flat 模式一致；引入通用 payload 需连带重构 stripe/creem（最小改动原则）；JSAPI 需 6 参数，平铺会污染 PaymentContext，故用嵌套结构体。不改变产品语义、风险接受、成本或兼容承诺 | agent | .ai/design/wechat-support.md §4.1 取舍 2、§4.2（2026-08-13 `/t-design`） | task | 多 provider 场景下 flat 字段膨胀到需要通用载荷收敛 |
| DEC-wechat-support-012 | D2 | jsapi.openid-frontend-contract | web 前端把 URL search param（`wechatOpenid`）作为"调用方传入 openid"的显式契约：微信内置浏览器内仅当该参数存在才走 `paymentScene=jsapi` 下单；缺失时展示"需先完成微信登录"（US-WP-003 场景 2），不派发订单；非微信环境一律 native。不改后端、不建登录链路 | DEC-009 的重开条件被仓库事实触发：`UserProfile={email,id,nickname,status}`，OpenAPI 175 路径中无任何端点把微信 openid 暴露给前端会话（openid 在登录时被服务端消费为 provider_user_id）。2026-08-14 `/t-super-run --phase frontend` 已用 AskUserQuestion 提问（含推荐项），用户未作答；agent 按推荐项落地并显式记录。2026-08-14 `/t-prd-publish` AskUserQuestion 用户确认维持本契约。缺参降级行为与 PRD §7"缺少 openid 时禁用或拒绝"一致，不改变业务规则 | user（08-14 /t-prd-publish 确认；08-14 frontend phase 问询未答，agent 按推荐项落地） | conversation (08-14 AskUserQuestion 无回答) + 仓库事实核实（frontend/src/stores/auth-store.ts、frontend/api.json）+ /t-prd-publish (08-14) 用户确认 | task/frontend | 用户否决本契约，或前端会话出现真实 openid 来源（如公众号登录 feature） | — |

## Resolved Questions

| ID | Topic | Resolution | Decision ID | Source |
|---|---|---|---|---|
| Q-wechat-support-001 | product.no-catalog | WeChat 无托管产品目录 → 跳过同步 + 手工建 mapping + Herald 侧定价（JSONB）。直接回答用户"WeChat 不支持设置产品怎么办" | DEC-wechat-support-006 | conversation (07-26) + 08-13 代码核实 |
| Q-wechat-support-002 | fulfillment.subscription | WeChat 订阅建模为 NonRenewing（取代 recurring-履约一次）；积分包/买断为 OneTime | DEC-wechat-support-003 | conversation (08-13) + pay_model DEC-pay_model-002 |
| Q-wechat-support-003 | scope.recurring | 自动续费（委托代扣）本期延后为独立 feature | DEC-wechat-support-002 | conversation (08-13 AskUserQuestion) |
| Q-wechat-support-004 | dependency.sdk-reuse | 不引入第三方 WeChat SDK，全自建；旧删除根因（openssl 冲突）已规避 | DEC-wechat-support-004 | conversation (07-26) + git 历史 |
| Q-wechat-support-007 | prd.wechat-subscription-wording | `/t-prd wechat-support` 已更新 PRD §2.2/§4.1/§8.1，将旧"recurring 以一次性付款完成履约"表述改为 NonRenewing（固定期、单次付款、可重复购买），自动续费（委托代扣）延后；draft user story 补"订阅到期不自动续费、可重新购买"场景 | DEC-wechat-support-003 | conversation (08-13) + `/t-prd` (08-13) |
| Q-wechat-support-008 | jsapi.openid-frontend-contract | 前端会话无 openid 来源（触发 DEC-009 重开条件）→ 以 URL search param `wechatOpenid` 作为调用方传入 openid 的显式契约，缺参时按 US-WP-003 场景 2 拒绝并提示；2026-08-14 /t-prd-publish 用户确认维持 | DEC-wechat-support-012 | conversation (08-14 AskUserQuestion 无回答) + 仓库事实核实 + /t-prd-publish (08-14) 用户确认 |

## Deferred Questions

| ID | Topic | Why Non-blocking Now | Owner Stage | Must Resolve Before | User Informed |
|---|---|---|---|---|---|
| Q-wechat-support-005 | recurring.entrust-deduction | 委托代扣是独立 API 流程，本期范围已收敛到统一下单；不影响本期技术路线 | t-tech-research（新 feature） | 启动委托代扣 feature 的 t-task | yes (08-13) |
| Q-wechat-support-006 | recurring.deduction-worker-placement | 将来委托代扣的周期扣款调度复用 `backend/worker` 现有 job 框架（方向已定，本期不实现） | t-design（新 feature） | 启动委托代扣 feature 的 t-task | yes (08-13) |

## Superseded Decisions

| ID | Superseded By | Previous Decision | Source |
|---|---|---|---|
| — | — | — | — |

> 说明：07-26 产物未建立正式 DEC 账本；DEC-wechat-support-003 的 `Supersedes` 列指向的是 PRD §8.1 的一段非正式表述（非 DEC ID），该表述已由 08-13 `/t-prd wechat-support` 更新（Resolved Q-wechat-support-007）。
