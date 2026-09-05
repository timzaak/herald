# Device Code 登录用户故事

> 角色定义见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)

## 用户故事

### 故事 1：CLI 工具发起设备授权 [US-DC-001]

**优先级**: P0

**【用户故事】**
**作为**：第三方应用（CLI 工具）（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：通过 Device Authorization Grant 向 Herald 请求设备授权码
**从而**：在无浏览器或输入受限的环境中安全完成用户认证

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：成功发起设备授权请求**
```gherkin
Given 第三方应用 "my-cli" 已在 realm-1 中注册为 Client App 且已启用
When CLI 工具发起设备授权请求
Then 系统返回设备码（device_code）、用户码（user_code）、验证地址（verification_uri）、有效期（15 分钟）和轮询间隔（5 秒）
```

**场景 2：用户码格式与可读性**
```gherkin
Given 系统返回了用户码（user_code）
When 用户查看 CLI 输出
Then 用户码为 8 字符的大写辅音字母组合，以连字符分隔（如 "BCDF-GHJK"），不包含易混淆字符（元音和易混淆辅音）
```

**场景 3：Client App 已禁用**
```gherkin
Given Client App "my-cli" 处于禁用状态
When CLI 工具发起设备授权请求
Then 系统提示"该应用已被禁用"，请求失败
```

**场景 4：Client App 不存在**
```gherkin
Given CLI 工具使用的 client_id 未在系统中注册
When CLI 工具发起设备授权请求
Then 系统提示"无效的客户端标识"，请求失败
```

**场景 5：设备授权码过期**
```gherkin
Given 设备授权请求已超过有效期（15 分钟）
When CLI 工具使用该设备码轮询令牌
Then 系统提示"设备码已过期"，CLI 工具应引导用户重新发起授权
```

---

### 故事 2：用户在验证页面完成授权 [US-DC-002]

**优先级**: P0

**【用户故事】**
**作为**：普通用户（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：在 Herald 验证页面输入设备码并完成登录授权
**从而**：授权 CLI 工具以我的身份访问受保护资源

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：成功输入设备码并授权**
```gherkin
Given 用户在浏览器访问 Herald 的设备验证页面
When 用户输入正确的用户码（如 "BCDF-GHJK"）并提交
Then 系统提示用户登录（如未登录），登录后显示授权确认页面，展示请求授权的 Client App 名称
When 用户点击"授权"
Then 系统提示"授权成功，请返回 CLI 工具"
And CLI 工具通过轮询获取到 access token
```

**场景 2：通过完整验证链接直接授权**
```gherkin
Given CLI 工具提供了包含用户码的完整验证链接
When 用户通过该链接访问验证页面
Then 系统自动填入用户码，用户只需登录并确认授权
```

**场景 3：设备码无效或已过期**
```gherkin
Given 用户输入的用户码不存在或已过期
When 用户提交验证
Then 系统提示"设备码无效或已过期，请在 CLI 工具中重新获取"
```

**场景 4：设备码已被使用**
```gherkin
Given 用户已经使用该用户码完成了授权
When 另一个用户尝试使用相同的用户码
Then 系统提示"设备码已使用"
```

**场景 5：用户拒绝授权**
```gherkin
Given 用户在授权确认页面看到 Client App 信息
When 用户点击"拒绝"
Then 系统提示"授权已拒绝"
And CLI 工具轮询时收到"授权被拒绝"的提示
```

---

### 故事 3：CLI 工具轮询获取令牌 [US-DC-003]

**优先级**: P0

**【用户故事】**
**作为**：第三方应用（CLI 工具）（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：按照协议规定间隔轮询，直到用户完成授权
**从而**：获得 access token 用于后续 API 调用

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：用户完成授权后获取令牌**
```gherkin
Given 用户已在验证页面完成授权
And CLI 以规定间隔轮询令牌
When 轮询请求携带有效的设备码
Then 系统返回 access token
```

**场景 2：用户尚未完成授权（等待中）**
```gherkin
Given 用户还未在验证页面完成授权
When CLI 轮询令牌
Then 系统返回"授权待确认"状态，CLI 应继续轮询
```

**场景 3：轮询过快需要降速**
```gherkin
Given CLI 轮询频率高于规定间隔
When CLI 在间隔时间内再次请求
Then 系统返回"请求过快"提示，CLI 应将轮询间隔增加 5 秒后继续
```

**场景 4：设备码过期**
```gherkin
Given 设备码已超过有效期
When CLI 轮询令牌
Then 系统提示"设备码已过期"，CLI 应引导用户重新发起设备授权
```

**场景 5：用户拒绝授权**
```gherkin
Given 用户在验证页面拒绝了授权
When CLI 轮询令牌
Then 系统提示"授权被拒绝"，CLI 应提示用户授权被拒绝
```

---

### 故事 4：Realm Admin 配置 Device Code Grant [US-DC-004]

**优先级**: P1

**【用户故事】**
**作为**：Realm Admin（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：为 Client App 启用或禁用 Device Code Grant
**从而**：按需控制哪些应用支持 CLI 设备码登录

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：为 Client App 启用 Device Code Grant**
```gherkin
Given Realm Admin 在 Client App 设置页面
When 管理员将 Device Code Grant 设置为"启用"
Then 该 Client App 可以发起设备授权请求
```

**场景 2：为 Client App 禁用 Device Code Grant**
```gherkin
Given Realm Admin 在 Client App 设置页面
When 管理员将 Device Code Grant 设置为"禁用"
Then 该 Client App 发起设备授权请求时，系统提示"该应用未授权使用设备码登录"
```

**场景 3：默认状态**
```gherkin
Given 新创建的 Client App
When 管理员查看 Device Code Grant 配置
Then 默认为"禁用"状态，需手动启用
```

---

### 故事 5：自定义设备码验证体验 [US-DC-005]

**优先级**: P1

**【用户故事】**
**作为**：第三方应用开发者（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：在自己的应用中构建自定义的设备码验证和授权确认流程
**从而**：提供与自有品牌一致的用户体验，而非依赖 Herald 提供的验证页面

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：验证设备码**
```gherkin
Given 拥有有效的用户码（user_code）
And 用户已登录 Herald
When 第三方应用验证该用户码
Then 系统验证用户码有效后，返回需要授权的 Client App 信息
```

**场景 2：确认授权**
```gherkin
Given 用户码已验证通过
When 第三方应用提交确认授权请求
Then 系统完成授权绑定，CLI 工具下次轮询时获得 access token
```

**场景 3：无效设备码**
```gherkin
Given 用户码不存在或已过期
When 第三方应用尝试验证
Then 系统返回验证失败提示
```

---

## 业务规则与边界说明

1. **Device Code Grant 是 OAuth 2.0 的扩展授权类型**（RFC 8628），适用于无浏览器或输入受限的设备
2. 用户码（user_code）格式为 8 字符（`XXXX-XXXX`），使用大写辅音字母（BCDFGHJKMNPQRSTVWXYZ）排除元音和易混淆字符
3. 设备码（device_code）对用户不可见，仅用于后端轮询
4. 设备码和用户码的默认有效期为 900 秒（15 分钟）
5. 默认轮询间隔为 5 秒
6. Device Code Grant 需在 Client App 配置中显式启用
7. Device Code Grant 不需要跳转地址（redirect_uri），适用于无浏览器环境

### 安全注意事项
1. 设备码应使用高强度随机值，不可猜测
2. 设备授权请求按来源 IP 限流（每 60 秒 10 次）；不另设单个 Client App 的 pending 设备码并发计数
3. 用户应只输入自己发起的用户码，防范钓鱼攻击
4. 验证页面应展示请求授权的 Client App 名称，帮助用户确认
5. 已使用或已过期的用户码应立即失效
6. 授权确认后设备码应标记为已使用，防止重放

---

## 相关文档

- **Device Code**: [docs/prd/auth/device-code.md](/docs/prd/auth/device-code.md)
- **OAuth 第三方集成**: [docs/prd/auth/oauth.md](/docs/prd/auth/oauth.md)
- **Client App 管理**: [docs/prd/integration/client-app.md](/docs/prd/integration/client-app.md)
