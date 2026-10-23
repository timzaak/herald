# WeChat Pay 用户故事

> 角色定义以 `docs/user-stories/_roles.md` 为准。
> 复用的既有用户故事（不重复创建）：
> - 通用支付平台配置：`docs/user-stories/billing/payment-provider.md`（US-PV-001~005）
> - 通用支付尝试生命周期：`docs/user-stories/billing/payment-attempt.md`（US-PA-001~004）
> 本文件仅承载 WeChat Pay 渠道特有的场景：商户凭据配置、PC 扫码 Native 支付、微信内 JSAPI 唤起支付、回调验签解密与幂等履约、平台证书自动维护。

## 用户故事

### 故事 1：配置 WeChat Pay 凭据 [US-WP-001]

**优先级**: P0

> 与 US-PV-001/US-PV-002 同属"配置支付平台"旅程的 WeChat 场景；本故事聚焦 WeChat 特有商户凭据（商户私钥 PEM、APIv3 Key、证书序列号）的配置与保护。微信平台证书的自动获取与刷新见 [US-WP-005]。

**【用户故事】**
**作为**：Realm Admin（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：为本 Realm 配置 WeChat Pay 商户凭据，并对敏感字段做脱敏与保留式编辑
**从而**：启用 WeChat Pay 收款并保证商户私钥与 APIv3 Key 不泄露

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：配置 WeChat Pay 凭据**
```gherkin
Given 我是 realm-1 的管理员
When 我在支付平台管理页面选择新增 WeChat Pay 配置
And 我填写 WeChat 商户凭据（appId、mchId、商户私钥 PEM、证书序列号、APIv3 Key、回调通知地址）
And 我提交表单
Then 配置创建成功
And 商户私钥与 APIv3 Key 以脱敏方式展示
And WeChat Pay 出现在本 Realm 可用支付平台列表中
```

**场景 2：编辑时保留敏感凭据**
```gherkin
Given 我已存在 WeChat Pay 配置
When 我编辑配置且未修改商户私钥与 APIv3 Key（留空）
And 我修改回调通知地址等非敏感字段并保存
Then 配置更新成功
And 原有商户私钥与 APIv3 Key 保持不变
And 非敏感字段更新为新值
```

**场景 3：删除有活跃订阅的配置被拒绝**
```gherkin
Given 我已存在 WeChat Pay 配置
And 该配置下存在活跃订阅
When 我尝试删除该配置
Then 系统拒绝删除并提示活跃订阅数量
And 配置保持不变
```

---

### 故事 2：PC 扫码 Native 支付 [US-WP-002]

**优先级**: P0

**【用户故事】**
**作为**：Regular User（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：在 PC 浏览器购买订阅或积分包时，用微信扫描二维码完成支付
**从而**：在不跳转第三方页面的情况下用微信钱包付款

**【验收标准】**

**场景 1：生成二维码并扫码支付成功**
```gherkin
Given 我在 PC 浏览器选择了支持 WeChat Pay 的套餐或积分包
And 我选择 WeChat Pay 作为支付方式
When 系统发起支付
Then 页面展示 WeChat 收款二维码与倒计时
When 我用微信扫码并完成支付
And 微信回调到达系统
Then 页面停止轮询并展示支付成功
And 系统按购买类型完成履约（订阅或积分发放）
```

**场景 2：二维码过期**
```gherkin
Given 我已展示 WeChat 收款二维码
And 倒计时结束（超过支付有效期）
When 前端轮询支付状态
Then 页面提示二维码已过期
And 前端停止轮询
And 提供"重新获取二维码"入口
```

**场景 3：取消支付或支付未成功**
```gherkin
Given 我已展示 WeChat 收款二维码但支付未完成（如取消支付、余额不足）
When 我取消本次支付或支付未成功
Then 系统不发放任何权益
And 购买页提供"重新支付"入口
```

> 重复回调不重复履约的验收由 [US-WP-004] 场景 4（系统视角：回调幂等）统一覆盖，此处不重复定义。

**场景 4：订阅型产品到期不自动续费、可重新购买**
```gherkin
Given 我通过 WeChat Pay 购买了一份订阅型产品
And 该订阅按固定有效期生效（与 Stripe 订阅的自动续费不同）
When 订阅有效期到达终点
Then 系统不自动向我扣费
And 我的订阅进入到期状态
And 我可再次发起 WeChat Pay 购买以续期
```

---

### 故事 3：微信内 JSAPI 唤起支付 [US-WP-003]

**优先级**: P1

> JSAPI 所需的 openid 由调用方通过既有微信登录链路（见 `docs/prd/auth/wechat-oauth.md`）取得并随下单请求传入；登录态获取不在本故事范围。

**【用户故事】**
**作为**：Regular User（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：在微信内网页/小程序中购买时，直接唤起微信支付完成付款
**从而**：在微信生态内获得无缝的支付体验，无需扫码

**【验收标准】**

**场景 1：已微信登录用户唤起 JSAPI 支付成功**
```gherkin
Given 我在微信内网页中已通过微信登录（系统已获得我的 openid）
And 我选择了支持 WeChat Pay 的套餐或积分包
When 系统以我的 openid 发起 JSAPI 支付
Then 微信支付被唤起
When 我在微信内完成支付
And 微信回调到达系统
Then 我看到支付成功反馈
And 系统按购买类型完成履约
```

**场景 2：缺少 openid 时拒绝下单**
```gherkin
Given 我尚未通过微信登录（系统无法获得 openid）
When 系统尝试发起 JSAPI 支付
Then 系统拒绝下单并提示需先完成微信登录
And 不发起支付
```

**场景 3：JSAPI 支付失败**
```gherkin
Given 我已唤起 JSAPI 支付
When 我取消支付或支付失败
Then 我看到支付未成功反馈
And 提供"重新支付"入口
And 不发放任何权益
```

---

### 故事 4：WeChat 回调验签、解密与幂等履约 [US-WP-004]

**优先级**: P0

> 与 US-PA-003（处理支付成功后的履约）同属履约旅程；本故事聚焦 WeChat 回调特有的验签/解密与平台证书维护。

**【用户故事】**
**作为**：Herald 系统（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：安全验证并解密 WeChat 回调，幂等地完成履约
**从而**：保证只接受来自微信的合法通知，且重复回调不会重复发放权益

**【验收标准】**

**场景 1：合法回调验签解密并履约**
```gherkin
Given 微信向本 Realm 的 WeChat 回调地址发送支付成功通知
And 该通知经平台证书签名且密文用 APIv3 Key 加密
When 系统接收回调
Then 系统用缓存的平台证书验证签名通过
And 用 APIv3 Key 解密回调密文成功
And 按商户订单号定位支付尝试并更新为已成功
And 按购买类型完成履约（订阅或积分发放）
```

**场景 2：签名验证失败被拒绝**
```gherkin
Given 微信回调的签名不合法（如被篡改、签名错误、平台证书缺失）
When 系统接收回调
Then 系统拒绝处理该回调
And 不更新任何支付状态
And 不触发履约
```

**场景 3：金额不符被拒绝**
```gherkin
Given 微信回调的支付金额与本地支付尝试记录的金额不一致
When 系统解密并比对金额
Then 系统拒绝履约并记录诊断
And 不发放权益
```

**场景 4：重复回调幂等**
```gherkin
Given 一笔支付尝试已通过回调完成履约
When 微信重复发送同一外部事件的回调
Then 系统识别为已处理的回调
And 不重复改变支付状态、订阅或积分
```

---

### 故事 5：平台证书自动获取与刷新 [US-WP-005]

**优先级**: P0

> 平台证书用于 WeChat 回调验签，由系统运行时维护，无需管理员手工预置。验签失败处理见 [US-WP-004]。

**【用户故事】**
**作为**：Herald 系统（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：在回调验签需要时自动获取微信平台证书，并在证书过期前自动刷新
**从而**：保证回调验签长期可用，无需人工干预，也不因证书过期中断支付

**【验收标准】**

**场景 1：平台证书自动获取（无需手工预置）**
```gherkin
Given 我已配置 WeChat Pay 商户凭据
And 尚未手工填写微信平台公钥
When 系统首次需要验证 WeChat 回调签名时
Then 系统自动从微信下载平台证书并缓存
And 后续回调验签使用已缓存的平台证书
And 管理员无需手工获取或粘贴平台证书即可完成回调验签
```

**场景 2：平台证书过期前自动刷新**
```gherkin
Given 已缓存的微信平台证书即将过期
When 系统检测到证书接近有效期终点
Then 系统自动重新下载并替换缓存的平台证书
And 回调验签不中断
```

---

## 用户故事优先级汇总

| 优先级 | 用户故事数量 | 关键故事 |
|--------|------------|---------|
| P0 | 4 | US-WP-001 配置 WeChat Pay 凭据、US-WP-002 PC 扫码 Native 支付、US-WP-004 回调验签解密与幂等履约、US-WP-005 平台证书自动获取与刷新 |
| P1 | 1 | US-WP-003 微信内 JSAPI 唤起支付 |
| P2 | 0 | - |

---

## 相关文档

- **PRD**: `docs/prd/billing/wechat-support.md`
- **通用支付平台配置**: `docs/user-stories/billing/payment-provider.md`（US-PV-001～005，WeChat 复用其通用 CRUD）
- **通用支付尝试生命周期**: `docs/user-stories/billing/payment-attempt.md`（US-PA-001～004，WeChat 复用其统一履约）
- **订阅计费 PRD**: `docs/prd/billing/subscription.md`
- **履约模型扩展 PRD**: `docs/prd/billing/pay_model.md`（非续期订阅建模）
- **Stripe 支付 PRD**: `docs/prd/billing/stripe-payment.md`（同类支付渠道 PRD 参考）
- **微信 OAuth PRD**: `docs/prd/auth/wechat-oauth.md`（JSAPI openid 来源链路）
