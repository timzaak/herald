# App Store / Google Play 内购(IAP) 用户故事

> 角色定义以 `docs/user-stories/_roles.md` 为准。
> 复用的已发布用户故事（不重复创建）：
> - 通用支付尝试生命周期：`docs/user-stories/billing/payment-attempt.md`（US-PA-001~004）
> - 通用支付平台配置：`docs/user-stories/billing/payment-provider.md`（US-PV-001~005）
> - 集成方前端充值/购买（移动 App 接入基线）：`docs/user-stories/integration/custom-user-ui.md`（US-CUI-008）
> - Entitlement 映射：`docs/user-stories/billing/entitlement-mapping.md`
> 本文件仅承载 IAP 渠道独有的场景：客户端凭证提交履约（主路径）、Apple 通知驱动生命周期与兜底、Google 定时轮询驱动生命周期、权益查询、定时拉取对账。

## 用户故事

### 故事 1：配置 IAP 支付渠道凭证 [US-IAP-001]

**优先级**: P0

**【用户故事】**
**作为**：Realm Admin（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：在支付平台配置页配置 App Store / Google Play 的服务端校验与通知接收凭证
**从而**：让本 Realm 的移动 App 集成方能够通过 IAP 完成购买与订阅

**【验收标准】**

**场景 1：配置 Apple App Store IAP 凭证**
```gherkin
Given 我是 realm-1 的管理员
When 我在支付平台管理页面点击 "Add Provider" 按钮
And 我选择平台类型为 "Apple App Store IAP"
And 我填写配置信息（Bundle ID、Issuer ID、Key ID、.p8 私钥、通知环境 sandbox/production）
And 我提交表单
Then IAP 渠道配置创建成功
And 系统显示成功消息
And .p8 私钥显示为脱敏格式
And 系统提示我需在 App Store Connect 配置服务端通知 URL
```

**场景 2：配置 Google Play Billing 凭证**
```gherkin
Given 我是 realm-1 的管理员
When 我在支付平台管理页面点击 "Add Provider" 按钮
And 我选择平台类型为 "Google Play Billing"
And 我填写配置信息（Package Name、Service Account JSON）
And 我提交表单
Then IAP 渠道配置创建成功
And Service Account JSON 显示为脱敏格式
And 系统提示我需在 Google Play Console → API Access 关联该 Service Account（无需配置 RTDN / Pub/Sub）
```

**场景 3：删除有活跃订阅的 IAP 配置**
```gherkin
Given realm-1 已有用户通过 IAP 处于订阅中
When 我尝试删除该 IAP 渠道配置
Then 系统拒绝删除
And 提示活跃订阅数量，引导我先处理订阅
```

---

### 故事 2：建立 IAP 商品与权益的映射 [US-IAP-002]

**优先级**: P0

**【用户故事】**
**作为**：Realm Admin（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：把 App Store Connect / Google Play Console 中的 IAP 商品 ID 映射到 Herald 的 entitlement_key，并指定商品类型（订阅 / 消耗型积分包）与积分策略
**从而**：移动 App 集成方购买某个 IAP 商品后，Herald 能按既定规则发放订阅或积分

**【验收标准】**

**场景 1：配置自动续期订阅型 IAP 映射**
```gherkin
Given realm-1 已配置 Apple App Store IAP 凭证
And 我在 App Store Connect 创建了自动续期订阅产品 "pro_monthly"
When 我在 Entitlement 映射管理页新建映射
And 选择 provider = "Apple App Store IAP"
And 填入商品 ID = "pro_monthly"
And 指定 entitlement_key = "pro" 和计费类型 = "订阅(recurring)"
And 配置订阅积分策略（首次订阅积分、续费积分）
Then 映射保存成功并启用
Then 该映射出现在购买页与 SDK 查询结果中
```

**场景 2：配置消耗型积分包 IAP 映射**
```gherkin
Given realm-1 已配置 Google Play Billing 凭证
And 我在 Play Console 创建了消耗型商品 "points_pack_1000"
When 我在 Entitlement 映射管理页新建映射
And 选择 provider = "Google Play Billing"
And 填入商品 ID = "points_pack_1000"
And 指定计费类型 = "一次性积分包(one_time)"
And 配置积分数量与有效期
Then 映射保存成功并启用
Then 用户在移动 App 购买后获得对应积分
```

**场景 3：商品 ID 在同一 provider 内重复**
```gherkin
Given realm-1 已存在 provider="Apple App Store IAP" + 商品 ID="pro_monthly" 的启用映射
When 我再次创建相同 provider + 商品 ID 的新映射
Then 系统拒绝创建
And 提示该商品 ID 已有启用映射
```

---

### 故事 3：客户端提交凭证触发履约（主路径） [US-IAP-003]

**优先级**: P0

**【用户故事】**
**作为**：第三方应用开发者（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：移动 App 在用户完成 IAP 购买后，把 Apple `jwsRepresentation` 或 Google `purchaseToken` 通过既有 api-billing 浏览器路由提交给 Herald，触发即时校验与履约
**从而**：用户购买后能在 App 内立即看到权益到账反馈，权益发放不依赖 App 是否在线或平台通知是否到达

**【验收标准】**

**场景 1：提交凭证触发即时校验与履约（Apple）**
```gherkin
Given 用户已在移动 App 完成 StoreKit 2 购买
And 持有有效的 jwsRepresentation
When App 通过既有 api-billing 浏览器路由（Bearer token + PurchaseInitiate scope）提交该凭证
Then Herald 用自管的 Apple Root CA 对 JWS 做 x5c + ES256 本地验签（无需回调 Apple）
And 验签通过后按商品类型履约（订阅 / 积分）
And 返回该 attempt 的当前状态给 App
```

**场景 2：提交凭证触发即时校验与履约（Google）**
```gherkin
Given 用户已在移动 App 完成 Google Play 购买
And 持有有效的 purchaseToken
When App 通过既有 api-billing 浏览器路由提交该凭证
Then Herald 调 Google 服务端 API 回查真实状态
And 校验通过且状态为已购买后按商品类型履约
And 订阅 acknowledge（订阅）/ consume（消耗型）在履约事务前执行（ack-first，3 天内完成；顺序取舍与 consume 后履约失败的死区恢复见 PRD §4.1 确认截止规则）
And 返回该 attempt 的当前状态给 App
```

**场景 3：凭证校验失败或归属不符**
```gherkin
Given App 提交的凭证验签失败、API 回查不通过或不属于当前用户
When Herald 完成校验
Then Herald 拒绝履约
And 返回明确的失败原因（凭证无效 / 归属不符 / 已消耗）
And 校验失败不产生支付尝试记录，由客户端修正凭证后重新提交
```

**场景 4：与平台通知 / 定时拉取幂等一致**
```gherkin
Given 某笔交易已由平台服务端通知（Apple）或定时拉取（Google）履约
When App 又提交同一交易的凭证
Then Herald 因幂等约束（以 originalTransactionId / purchaseToken 为去重键）不重复发放
And 返回当前状态给 App
```

---

### 故事 4：Apple 服务端通知驱动生命周期与兜底 [US-IAP-004]

**优先级**: P0

**【用户故事】**
**作为**：Herald 系统（System Actor，详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：在收到 Apple App Store Server Notifications V2 时，驱动订阅续期、退款、取消等后续生命周期，并对客户端漏提交的购买兜底履约
**从而**：即使用户卸载 App 或客户端凭证漏提交，Apple 订阅状态变更与漏发购买也能被可靠、幂等地处理

**【验收标准】**

**场景 1：订阅续期、退款、取消（Apple）**
```gherkin
Given 用户在 realm-1 持有 Apple IAP 订阅
When Herald 收到 Apple 续期 / 取消 / 退款通知（DID_RENEW / DID_CHANGE_RENEWAL_STATUS / REFUND 等）
Then 订阅状态按通知内容转换（Active / Canceled / Expired / Past Due）
And 退款按现有积分回收规则回收积分
And 全部变更记录在订阅变更历史中
```

**场景 2：客户端漏提交时由通知兜底履约（Apple）**
```gherkin
Given 用户在移动 App 完成 Apple IAP 购买但客户端未提交凭证（如 App 卸载、网络丢失）
When Herald 收到对应的 Apple 服务端通知（SUBSCRIBED 等）
And 通知通过 JWS 签名校验
Then Herald 按通知中的商品 ID 解析 entitlement_mapping 并按商品类型履约
And 重复通知不重复发放
```

**场景 3：通知签名校验失败**
```gherkin
Given Herald 收到一个自称来自 App Store 的通知
When 其 JWS 签名 / 证书链校验失败
Then Herald 拒绝处理该通知
And 记录诊断日志，不改变任何权益或积分
```

**场景 4：商品 ID 无对应映射**
```gherkin
Given Herald 收到合法服务端通知
But 通知中的商品 ID 在本 Realm 没有启用的 entitlement_mapping
Then Herald fail loud，记录诊断并跳过履约
And 不静默降级为默认积分策略
```

**场景 5：sandbox 通知丢失或乱序**
```gherkin
Given sandbox 环境下 Apple 通知可能丢失或乱序到达
When 某笔购买的首次通知未到达 Herald
Then 客户端提交（US-IAP-003）保证购买即时履约
And 后续漏发的生命周期事件由定时拉取（US-IAP-006）在下个周期兜底
```

---

### 故事 5：查询 IAP 订阅与权益状态 [US-IAP-005]

**优先级**: P1

**【用户故事】**
**作为**：第三方应用开发者（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：通过既有 SDK / api-billing 浏览器路由查询用户当前通过 IAP 获得的订阅状态与 entitlement_key
**从而**：第三方应用能基于权益状态决定功能可见性与配额

**【验收标准】**

**场景 1：查询当前用户的 IAP 订阅权益**
```gherkin
Given 用户持有 realm-1 的 IAP 订阅
When 第三方应用以用户身份查询订阅状态
Then 返回该用户的 entitlement_key、计费类型、provider 与订阅状态
And IAP 渠道的订阅与 Stripe / Creem 订阅以统一格式返回
```

**场景 2：订阅过期或退款后的权益降级**
```gherkin
Given 用户曾在 realm-1 持有 IAP 订阅
And 退款或订阅过期已经平台通知（Apple）/ 定时拉取（Google）处理
When 第三方应用查询该用户权益
Then 返回的 entitlement 反映降级后的状态
And 历史订阅保留在订阅变更历史中
```

---

### 故事 6：定时拉取对账（Google 生命周期主驱动 / Apple 补偿）[US-IAP-006]

**优先级**: P0（Google 侧无服务端通知，轮询是其生命周期唯一驱动）

**【用户故事】**
**作为**：Herald 系统（System Actor，详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：定时对 App Store Server API / Google Play Developer API 拉取近期交易与订阅状态，驱动 Google 生命周期并识别本地缺失的 IAP 履约事件
**从而**：即使 Apple 通知丢失、Google 无服务端通知，IAP 购买与状态变更也能在下一个对账周期被处理，不产生权益漏发或状态滞留

**【验收标准】**

**场景 1：发现并补偿缺失的 IAP 履约 / 状态变更**
```gherkin
Given realm-1 已配置 IAP 凭证
When 对账任务向 Apple App Store Server API 拉取通知历史与订阅状态、向 Google 服务端 API 逐 token 回查活跃订阅并拉取作废购买
And 发现本地未履约的成功交易或滞后的状态变更
Then 复用与正常服务端通知相同的领域处理与幂等机制完成履约 / 状态转换
And 单个 Realm / 交易 / 平台 API 失败不阻塞其他对象
And 输出对账统计（拉取数、回放数、成功数、失败数；不维护独立的笼统"缺失数"——可恢复缺失以回放数体现，状态漂移以 drift detected 诊断体现）
```

**场景 2：API 配额与重放窗口约束**
```gherkin
Given Apple / Google 服务端 API 有调用配额与历史查询窗口限制
When 补偿任务运行
Then 对账间隔小于平台事件保留窗口
And 拉取分页与限流，不触发平台限流
```
