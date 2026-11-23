# Webhook 事件推送（Outbound Webhooks）用户故事

> 角色定义以目标项目中的 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md) 为准。
> 与既有 US-WC-001/002（[docs/user-stories/billing/webhook-compensation.md](/docs/user-stories/billing/webhook-compensation.md)，入站支付 Provider webhook 补偿）方向相反，互不替代。

## 用户故事

### 故事 1：配置 Webhook 事件订阅 [US-WEP-001]

**优先级**: P0

**【用户故事】**
**作为**：Realm Admin（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：在本 Realm 登记一个或多个事件回调地址，并为每个订阅勾选要接收的事件类型类别，还能停用、恢复或删除订阅、轮换签名密钥
**从而**：让我接入的应用后端按需接收 Herald 侧状态变更，避免全量噪声和多余回调

**【验收标准】**

**场景 1：新建订阅成功**
```gherkin
Given Realm Admin 已登录管理台并持有 Webhook 订阅管理权限
When 填写公网回调地址，勾选要订阅的事件类型类别（如用户生命周期 + 角色授予/撤销）并保存
Then 订阅创建成功并处于活跃状态，系统生成该订阅专属的签名密钥，且仅在此时完整展示一次
```

**场景 2：私网回调地址被拒绝**
```gherkin
Given Realm Admin 正在新建或编辑订阅
When 填写内网或环回地址（如 localhost、127.0.0.1、192.168.x.x）作为回调地址并保存
Then 保存被拒绝，并提示回调地址必须是公网可达地址
```

**场景 3：停用与恢复订阅**
```gherkin
Given 本 Realm 存在一个活跃订阅
When Realm Admin 停用该订阅
Then 该订阅状态变为停用且不再收到任何事件；之后恢复订阅时，仅恢复接收恢复之后发生的新事件，停用期间的事件不被补发
```

**场景 4：无权限者不可管理订阅**
```gherkin
Given 一个不持有 Webhook 订阅管理权限的本 Realm 普通用户
When 尝试访问 Webhooks 管理页面或订阅管理能力
Then 管理入口不可见，直接调用管理能力被拒绝
```

**【备注】**
事件类型类别以 PRD 事件目录（用户生命周期 / 角色授予与撤销 / 订阅生命周期）为边界；密钥不回显，重新生成后旧密钥不再用于新事件签名。

### 故事 2：状态变更生成并投递签名事件 [US-WEP-002]

**优先级**: P0

**【用户故事】**
**作为**：Herald 系统（System Actor，详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：在用户、角色、订阅状态变更生效后，把对应事件以该订阅的密钥签名并推送到所有匹配的活跃订阅
**从而**：接入方无需轮询即可秒级感知 Herald 侧状态变化

**【验收标准】**

**场景 1：用户禁用事件投递**
```gherkin
Given 本 Realm 存在勾选了用户生命周期类别的活跃订阅
When 本 Realm 内某用户被禁用
Then 系统在该状态变更生效后生成用户禁用事件，推送到该订阅的回调地址，事件携带全局唯一的事件 ID 与发生时间，并以该订阅的密钥完成 HMAC-SHA256 签名
```

**场景 2：支付驱动角色授予事件投递（paywall 联动）**
```gherkin
Given 本 Realm 存在勾选了角色授予/撤销类别的活跃订阅
When 用户支付成功触发自动授予 role（或退款/过期触发自动撤销 role）
Then 对应的角色授予/撤销事件被投递，与管理端手工授予/撤销的事件同等投递，不因来源不同被过滤
```

**场景 3：订阅生命周期事件投递**
```gherkin
Given 本 Realm 存在勾选了订阅生命周期类别的活跃订阅
When 用户的订阅生效、续期、过期或发生退款
Then 对应的订阅生命周期事件被投递到该订阅
```

**场景 4：事件类型过滤生效**
```gherkin
Given 本 Realm 某订阅仅勾选了角色授予/撤销类别
When 本 Realm 内发生用户创建
Then 该订阅不收到用户创建事件，仅勾选类别的事件被投递
```

**场景 5：Realm 隔离**
```gherkin
Given Realm A 存在活跃订阅，Realm B 内发生用户禁用
When Realm B 的状态变更触发生成事件
Then Realm A 的订阅不收到任何与 Realm B 相关的事件
```

### 故事 3：投递失败重试与持续失败自动暂停 [US-WEP-003]

**优先级**: P0

**【用户故事】**
**作为**：Herald 系统（System Actor，详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：投递失败时按指数退避自动重试，持续失败时仅暂停该订阅并通知 Realm Admin
**从而**：接入方故障不丢失事件，故障持续时也不产生无意义的重试风暴，且管理员能感知并处置

**【验收标准】**

**场景 1：失败重试直至送达**
```gherkin
Given 某事件投递到订阅的回调地址返回非成功响应或超时
When 投递失败
Then 系统按指数退避自动重试该事件，直到投递成功或达到持续失败判定，整体满足至少投递一次（at-least-once）
```

**场景 2：持续失败自动暂停并通知**
```gherkin
Given 某订阅的投递持续失败达到系统设定的持续失败判定
When 自动暂停触发
Then 仅该订阅被暂停（同 Realm 其他订阅不受影响），Realm Admin 在管理台可见该订阅的暂停状态；该 Realm 已配置邮件通道时，管理员同时收到暂停通知邮件
```

**场景 3：暂停期间事件不投递、恢复后不补发**
```gherkin
Given 某订阅处于自动暂停状态
When 暂停期间本 Realm 发生已订阅类型的状态变更，随后 Realm Admin 恢复该订阅
Then 暂停期间的事件不被投递也不被补发，恢复后仅投递恢复之后发生的新事件
```

**【备注】**
退避曲线、持续失败判定阈值、投递并发上限为运行参数，不在用户故事与 PRD 中承诺数值。

### 故事 4：接入方验签后幂等消费事件 [US-WEP-004]

**优先级**: P0

**【用户故事】**
**作为**：第三方应用开发者（Third-Party App，详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：按官方文档在自有后端验证事件签名，并以事件 ID 幂等处理收到的推送
**从而**：把授权判断、provisioning 和 paywall 状态更新从轮询升级为事件驱动，且重复/乱序投递不产生副作用

**【验收标准】**

**场景 1：paywall 秒级解锁**
```gherkin
Given 接入方后端已按文档实现验签与按事件 ID 幂等处理，其 Realm 已订阅角色授予/撤销事件
When 用户在接入方应用完成支付，Herald 自动授予 role 并投递角色授予事件
Then 接入方后端验签通过后即可解锁该用户功能，全程无需轮询 Herald API
```

**场景 2：账号禁用即时下线**
```gherkin
Given 接入方后端已按文档实现验签与按事件 ID 幂等处理，其 Realm 已订阅用户生命周期事件
When 用户在 Herald 侧被禁用并投递用户禁用事件
Then 接入方后端验签通过后即时下线或冻结该用户在接入方应用中的会话与访问
```

**场景 3：重复与乱序投递不产生副作用**
```gherkin
Given 同一事件因重试被重复投递，或同一用户的多个事件乱序到达
When 接入方按事件 ID 去重、并以后到状态为准处理
Then 接入方不产生重复副作用，最终状态与 Herald 侧一致
```

**场景 4：验签失败丢弃**
```gherkin
Given 有人向接入方回调地址伪造事件，或接入方尚未更新已轮换的新密钥
When 接入方验签失败
Then 该请求被接入方丢弃，不产生任何业务副作用
```

---

## 相关文档

- **PRD**: [docs/prd/integration/webhook-event-push.md](/docs/prd/integration/webhook-event-push.md)
- **入站对照**: [docs/user-stories/billing/webhook-compensation.md](/docs/user-stories/billing/webhook-compensation.md)
