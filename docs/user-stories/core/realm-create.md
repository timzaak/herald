# SaaS 自助注册开通 Realm 用户故事

> 角色定义见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)
> 与既有 `US-AR-001`（手动内部开通，actor：Admin Realm 管理员）的边界说明见本文末“与既有故事的关系”。

## 角色说明

**SaaS 自助注册访客（SaaS Self-Signup Visitor）**：未登录的匿名访客，希望通过公共注册页面开通一个属于自己的 Herald realm，并成为该 realm 的 realm-admin。该角色在本 feature 之前不存在独立的入口；注册成功后其身份转化为所开通 realm 的 realm-admin（详见 `docs/user-stories/_roles.md`）。

> 该访客是未认证的瞬态前置 actor（注册前），不是持久技术身份或 Principal，因此不单独列入 `_roles.md` 的角色清单；注册成功后即转化为 `realm-admin`。

---

## 用户故事

### 故事 1：自助注册开通新 Realm [US-SR-001]

**优先级**: P0

**【用户故事】**
**作为**：SaaS 自助注册访客
**我希望**：在公开注册页面填写信息即可开通一个新的、与我绑定的 realm
**从而**：无需联系平台管理员，自助获得一个独立的认证与授权租户

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：注册并开通成功**
```gherkin
Given 我是未登录的访客
And 我访问 admin realm 托管的公开注册页面
When 我填写注册信息（realm 名称、我的邮箱、我的密码）
And 我提交注册表单
Then 系统为我开通一个新的 realm
And 该 realm 自动包含默认角色、权限、策略、管理控制台客户端应用和管理员用户
And 我的账号被设为该 realm 的 realm-admin
And 我被引导进入新 realm 的管理界面
```

**场景 2：注册信息校验失败**
```gherkin
Given 我是未登录的访客
When 我在注册表单中提交不满足校验的信息（如邮箱格式错误、密码强度不足、realm 名称缺失）
Then 系统显示明确的校验错误提示，且不创建任何 realm
```

**场景 3：realm 标识冲突**
```gherkin
Given 我是未登录的访客
And 我指定了一个已被占用或为保留词的 realm 标识
When 我提交注册表单
Then 系统显示标识冲突提示，引导我更换标识
```

**场景 4：同一 IP 注册超出限额**
```gherkin
Given 同一 IP 在过去 24 小时内已通过自助注册开通了 2 个 realm
When 我从该 IP 再次提交注册表单
Then 注册被拒绝
And 系统提示注册数量已达上限
```

**场景 5：未通过人机验证（Turnstile 已启用时）**
```gherkin
Given 平台自助开通已开启
And 绑定自助注册页面的 Client App 的 Turnstile 已启用
When 我未完成或未通过 Cloudflare 人机验证即提交注册表单
Then 注册被拒绝，并提示需完成人机验证
```

---

### 故事 2：开通后立即管理新 Realm [US-SR-002]

**优先级**: P0

**【用户故事】**
**作为**：刚通过自助注册开通 realm 的用户（新 realm-admin）
**我希望**：注册成功后能立即登录并进入我新 realm 的管理界面
**从而**：无需额外的开通等待或人工审核即可开始配置

**【验收标准】**

**场景 1：注册成功后即进入新 realm**
```gherkin
Given 我刚通过自助注册成功开通了一个 realm
When 系统完成开通
Then 我自动获得该 realm 的会话
And 我进入该 realm 的管理控制台首页
And 我拥有 realm-admin 能看到的管理入口（用户、设置、客户端应用等）
```

**场景 2：新 realm 与其他 realm 严格隔离**
```gherkin
Given 我通过自助注册开通了 realm A
When 我尝试访问 realm B 的资源
Then 访问被拒绝，因为我仅被授权 realm A
```

---

### 故事 3：Admin Realm 管理员查看自助开通的 Realm [US-SR-003]

**优先级**: P1

**【用户故事】**
**作为**：Admin Realm 管理员（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：通过自助注册开通的 realm 出现在既有 Realm 管理列表中，与手动创建的 realm 一致
**从而**：平台运营可以统一管理所有 realm，无论其来源是手动创建还是自助注册

**【验收标准】**

**场景 1：自助开通的 realm 出现在 Realm 列表**
```gherkin
Given 一个访客刚通过自助注册开通了 realm X
And 我是 Admin Realm 管理员（拥有 realm.view 权限）
When 我访问 Realms 管理页面
Then realm X 出现在 Realm 列表中
And 它与手动创建的 realm 在可见字段上一致
```

---

### 故事 4：平台自助开通开关控制 [US-SR-004]

**优先级**: P0

**【用户故事】**
**作为**：Admin Realm 管理员（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：能够开启或关闭平台的公共自助开通入口
**从而**：在需要时（例如维护或限制增长）停止接受新的自助注册

**【验收标准】**

**场景 1：关闭自助开通后访客无法注册**
```gherkin
Given Admin Realm 管理员已关闭平台自助开通开关
When 一个未登录访客访问公开注册页面
Then 注册入口不可用或被明确拒绝，并提示自助开通当前不可用
```

**场景 2：开启自助开通后访客可注册**
```gherkin
Given Admin Realm 管理员已开启平台自助开通开关
When 一个未登录访客访问公开注册页面
Then 注册页面可用，访客可完成注册并开通 realm
```

---

### 故事 5：自助开通邮箱验证 [US-SR-005]

**优先级**: P0

**【用户故事】**
**作为**：Admin Realm 管理员（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：当我为 admin realm 开启了"注册需邮箱验证"时，自助开通 realm 的访客也必须先验证邮箱所有权才能开通
**从而**：防止任何人使用不属于自己的邮箱开通 realm、抢占 realm 标识与平台资源

**【验收标准】**

> 验证要求复用 admin realm 的注册邮箱验证配置：开启且平台邮件服务可用时才强制（详细规则见 PRD `docs/prd/core/realm-create.md` §4.1）。

**场景 1：开启邮箱验证后需先验证邮箱**
```gherkin
Given Admin Realm 已开启注册邮箱验证
And 平台自助开通已开启
When 我作为未登录访客在公开注册页面填写信息
Then 注册页面在提交前要求我先完成邮箱验证
And 我请求发送验证码后，在我的邮箱收到验证码
And 我填写正确的验证码并提交
Then 系统为我开通新的 realm
And 我直接进入新 realm 的管理控制台
```

**场景 2：验证码缺失或无效时开通被拒绝**
```gherkin
Given Admin Realm 已开启注册邮箱验证
When 我未填写验证码，或填写了错误、过期、或属于其他邮箱的验证码提交注册
Then 注册被拒绝并提示验证码无效
And 提示不区分具体是哪一种无效情形
And 系统不创建任何 realm
```

**场景 3：重新发送验证码后旧码失效**
```gherkin
Given Admin Realm 已开启注册邮箱验证
And 我已请求过一次验证码
When 我重新请求发送验证码
Then 我收到新的验证码
And 旧验证码立即失效
```

**场景 4：验证码请求过于频繁被限制**
```gherkin
Given Admin Realm 已开启注册邮箱验证
When 我在短时间内重复请求发送验证码，超出允许频率
Then 请求被拒绝并提示稍后再试
```

**场景 5：邮件服务未配置时自动降级**
```gherkin
Given Admin Realm 已开启注册邮箱验证
And 平台尚未配置可用的邮件服务
When 我作为未登录访客访问公开注册页面
Then 注册页面不出现邮箱验证步骤
And 我可一次提交直接开通 realm
```

**场景 6：关闭注册邮箱验证后回到一次提交开通**
```gherkin
Given Admin Realm 关闭了注册邮箱验证
When 我作为未登录访客填写注册信息并提交
Then 注册页面不出现邮箱验证步骤，行为与故事 1 一致
```

---

## 与既有故事的关系

- **不复用 `US-AR-001`（创建 Realm）**：`US-AR-001` 的 actor 是已登录的 Admin Realm 管理员，入口是管理后台“Create Realm”对话框，前置权限为 `realm.manage`；本组故事的 actor 是未登录访客，入口是公开注册页面，无需平台权限。两者 actor、入口与前置条件不同，故新建独立故事（见 DEC-realm-create-005）。
- **复用既有 realm 初始化规则**：新 realm 的自动初始化（默认 RBAC、`admin-web-console`、`admin-api-client`、`registration.enabled=false`、Normal 管理员）遵循 `docs/prd/core/realm.md` §4.1，不在本故事中重复定义（见 DEC-realm-create-003）。
