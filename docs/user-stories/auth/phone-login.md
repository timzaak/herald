# 手机号登录 用户故事

> 角色定义见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)。

---

## 用户故事

### 故事 1：用户自助绑定手机号 [US-PL-001]

**优先级**: P0

**【用户故事】**
**作为**：普通用户（Regular User，详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：在账户 Security 设置中为我的账号绑定手机号，并通过短信验证码证明这是我的手机
**从而**：之后可以用手机号接收验证码登录，不必依赖邮箱

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：绑定成功**
```gherkin
Given 当前 Realm 已配置短信通道并开启手机号登录
And 用户已在第一方浏览器会话登录并完成身份重新验证（reauth）
When 用户输入手机号并请求验证码，在收到短信后输入正确的验证码
Then 绑定成功
And Security 设置页显示脱敏形式的手机号（如 138****8000）与已验证状态
And 该手机号此后可用于本账号登录
```

**场景 2：手机号已被同 Realm 其他账号占用**
```gherkin
Given 用户输入的手机号（无论何种书写形式）与同 Realm 另一账号已绑定的手机号是同一号码
When 用户提交绑定
Then 系统提示手机号已被使用
And 绑定不生效
```

**场景 3：验证码错误或过期**
```gherkin
Given 用户已请求绑定验证码
When 用户输入错误验证码，或超过有效期后才提交
Then 系统提示"验证码错误或已失效"
And 绑定不生效，用户可重新输入或重新发送
And 连续错误达到上限后该次验证码作废，需重新发送
```

**场景 4：短信通道未配置或 Realm 未开启手机号登录**
```gherkin
Given 当前 Realm 未配置短信通道，或未开启手机号登录
When 用户访问 Security 设置页
Then 页面不展示手机号绑定入口（或说明原因）
And 直接发起绑定的请求被明确拒绝而非假成功
```

**场景 5：换绑手机号**
```gherkin
Given 用户账号已绑定并验证手机号 A
And 用户已登录并完成 reauth
When 用户发起换绑并完成新手机号 B 的短信验证码验证
Then 绑定更新为手机号 B
And 手机号 A 不再可用于本账号登录
```

**场景 6：解绑手机号**
```gherkin
Given 用户账号已绑定手机号
And 用户已登录并完成 reauth
When 用户确认解绑
Then 手机号解绑，不再可用于本账号登录
And 账号仍可用邮箱/用户名加密码登录
```

---

### 故事 2：用户用短信验证码登录已有账号 [US-PL-002]

**优先级**: P0

**【用户故事】**
**作为**：普通用户（Regular User，详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：用手机收到的短信验证码直接登录，而不必记忆或输入密码
**从而**：在手机使用场景下快速回到应用

**【验收标准】**

**场景 1：已绑定用户验证码登录成功**
```gherkin
Given 当前 Realm 已开启手机号登录
And 用户账号已绑定并验证手机号，账号处于正常状态
When 用户在登录入口输入手机号并完成人机验证（若启用）
And 在限定时间内收到并输入正确的验证码
Then 登录成功
And 会话按客户端会话方向（Bearer token）建立，而非依赖浏览器 cookie
And 用户进入应用首页或指定回跳页
```

**场景 2：验证码错误**
```gherkin
Given 用户已请求发送登录验证码
When 用户输入错误的验证码
Then 系统提示"验证码错误或已失效"
And 登录不成功，用户可重新输入
And 达到连续错误上限后该次验证码作废，需要重新发送
```

**场景 3：发送频率限制**
```gherkin
Given 用户针对同一手机号连续请求发送验证码
When 两次请求间隔低于限流阈值
Then 系统提示"请稍后再试"
And 不发送新的验证码
```

**场景 4：手机号未注册或未绑定任何账号**
```gherkin
Given 用户输入的手机号在当前 Realm 不匹配任何账号
When 用户发起短信验证码登录
Then 系统不创建任何账户，也不发送验证码
And 用户得到该手机号未注册的提示，或被引导到注册入口/其他登录方式
```

**场景 5：账号被禁用**
```gherkin
Given 用户的账号已被禁用
When 用户提交正确的验证码
Then 登录被拒绝
And 系统提示账号已被禁用
```

**场景 6：验证码被一次性消费后不能重复使用**
```gherkin
Given 用户已用某验证码成功登录
When 同一验证码被再次提交
Then 用户看到"验证码已失效"的提示
And 登录不成功，需要重新发送验证码
```

**场景 7：Realm 未开启手机号登录**
```gherkin
Given 当前 Realm 未开启手机号登录
When 用户访问登录页
Then 登录页不提供手机号登录入口
And 已绑定用户仍可使用密码 / TOTP / Passkey 登录
```

---

### 故事 3：用户在密码登录中使用手机号作为登录标识 [US-PL-003]

**优先级**: P1

**【用户故事】**
**作为**：普通用户（Regular User，详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：在密码登录框直接输入手机号和密码登录
**从而**：不必记忆注册邮箱或用户名

**【验收标准】**

**场景 1：手机号加密码登录成功**
```gherkin
Given 当前 Realm 已开启手机号登录
And 用户账号已绑定并验证手机号并设有密码
When 用户在密码登录入口输入手机号和正确密码
Then 登录成功
```

**场景 2：密码错误**
```gherkin
Given 用户在密码登录入口输入已绑定的手机号
When 用户输入错误密码
Then 系统按既有密码登录错误口径提示
And 用户可重试
```

**场景 3：手机号不匹配任何账号**
```gherkin
Given 用户在密码登录入口输入的手机号在当前 Realm 不匹配任何账号
When 用户提交登录
Then 得到与既有未知登录标识一致的登录失败反馈
And 不额外暴露该手机号是否曾经注册
```

---

### 故事 4：Realm 管理员配置短信通道与手机号登录开关 [US-PL-004]

**优先级**: P0

**【用户故事】**
**作为**：Realm 管理员（Realm Admin，详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：为本 Realm 配置短信服务商通道，并开启或关闭手机号登录
**从而**：为（尤其国内的）用户提供手机号登录能力，同时控制短信成本

**【验收标准】**

**场景 1：配置短信通道成功**
```gherkin
Given Realm 管理员持有设置管理权限
And 已在短信服务商侧完成签名与模板报备
When 管理员在 Settings 的短信配置中选择服务商（阿里云或 Twilio）
And 填写服务凭证、签名名/模板标识与默认国家码并保存
Then 配置保存成功
And 凭证类字段在页面上脱敏展示
And 配置变更记入审计日志
```

**场景 2：发送测试短信**
```gherkin
Given 本 Realm 短信通道已配置
When 管理员向指定手机号发送测试短信
Then 该手机号收到测试短信且页面显示发送结果
And 发送受频率限制保护
```

**场景 3：短信通道未配置时开关不可开启**
```gherkin
Given 本 Realm 短信通道未配置或配置不完整
When 管理员尝试开启"手机号登录"开关
Then 开关呈现禁用状态并提示需先配置短信服务
And 直接调用配置接口启用被明确拒绝
```

**场景 4：开启后手机号登录能力生效**
```gherkin
Given 本 Realm 已配置短信通道
When 管理员开启"手机号登录"开关
Then 本 Realm 登录页提供短信验证码登录入口
And 密码登录入口接受手机号作为登录标识
```

**场景 5：关闭后平滑降级**
```gherkin
Given 本 Realm 已开启手机号登录且有用户绑定了手机号
When 管理员关闭该开关
Then 登录页不再提供手机号登录能力
And 已绑定用户仍可使用密码 / TOTP / Passkey 登录
And 已绑定的手机号数据保留，重新开启后无需再次绑定
```

**场景 6：无法修改其他 Realm 的配置**
```gherkin
Given 管理员属于 realm-1
When 尝试访问或修改 realm-2 的短信通道配置与手机号登录开关
Then 系统提示权限不足并拒绝访问
```

---

### 故事 5：Realm 管理员处置用户手机号 [US-PL-005]

**优先级**: P1

**【用户故事】**
**作为**：Realm 管理员（Realm Admin，详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：在用户管理中查看（脱敏）、搜索、解绑与换绑用户的手机号
**从而**：在二次放号、换号等场景下完成事后处置，保障账号安全

**【验收标准】**

**场景 1：脱敏展示与搜索**
```gherkin
Given 本 Realm 存在已绑定手机号的用户
When 管理员查看用户列表或用户详情
Then 手机号以脱敏形式展示（如 138****8000）
When 管理员输入完整手机号搜索
Then 可定位绑定该手机号的用户
```

**场景 2：解绑用户手机号**
```gherkin
Given 某用户的手机号因二次放号需要处置
When 管理员对该用户执行解绑并完成二次确认
Then 该手机号解绑，不再可用于该账号登录
And 操作记入审计日志
```

**场景 3：换绑需新手机验证码验证**
```gherkin
Given 管理员为某用户发起手机号换绑
When 管理员输入新手机号
Then 系统向新手机发送验证码
When 新手机完成验证码验证
Then 换绑生效
And 验证未完成前旧绑定保持不变
```

**场景 4：无法处置其他 Realm 的用户**
```gherkin
Given 管理员属于 realm-1
When 尝试对 realm-2 的用户执行手机号查看、搜索、解绑或换绑
Then 系统提示权限不足并拒绝访问
```

---

## 业务规则

> 业务规则的唯一定义点是 PRD §4：[docs/prd/auth/phone-login.md](/docs/prd/auth/phone-login.md)。本文场景中的数值口径（验证码有效期、发送频率与尝试次数上限）以技术设计锚定为准，不属于 PRD 契约。

---

## 相关文档

- PRD：[docs/prd/auth/phone-login.md](/docs/prd/auth/phone-login.md)
- 既有可引用用户故事：
  - [docs/user-stories/core/regular-user.md](/docs/user-stories/core/regular-user.md)（US-RU-001 注册、US-RU-002 登录、US-RU-016 修改邮箱）
  - [docs/user-stories/auth/email-otp-login.md](/docs/user-stories/auth/email-otp-login.md)（邮箱验证码登录对应故事）
  - [docs/user-stories/core/realm-admin.md](/docs/user-stories/core/realm-admin.md)（US-RA-013~015 邮件服务配置与门控先例）
- 角色定义：[docs/user-stories/_roles.md](/docs/user-stories/_roles.md)
