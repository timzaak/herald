# 第三方应用 用户故事

> 角色定义见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)

**故事范围**: US-TP-001 ~ US-TP-007, US-TP-015 ~ US-TP-016

---

## 故事 1：OAuth 授权码登录（Authorization Code + PKCE） [US-TP-001]

**优先级**: P0

**【用户故事】**
**作为**：第三方应用开发者（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：能够使用 Authorization Code + PKCE 流程验证用户身份
**从而**：安全地获取用户访问令牌，无需将 client_secret 暴露给前端

**【验收标准】**

**场景 1：发起授权请求（含 PKCE 参数）**
```gherkin
Given 第三方应用 "my-app" 已接入 realm-1
And 第三方前端生成了 code_verifier 和对应的 code_challenge（S256 方法）
When 第三方前端将用户重定向到 Herald 授权端点，携带 client_id、redirect_uri、state、response_type=code、code_challenge 和 code_challenge_method=S256
Then Herald 校验 Client App 存在且启用、redirect_uri 在白名单中
And Herald 将用户重定向到登录页面，携带 OAuth 上下文参数
```

**场景 2：用户登录成功，获取授权码**
```gherkin
Given 用户在 Herald 登录页面输入正确的邮箱和密码
And 登录请求中包含完整的 OAuth 参数（oauthClientId、redirectUri、state）
When Herald 校验临时存储的 state 匹配
Then Herald 生成一次性授权码（关联 code_challenge、client_id、redirect_uri）
And Herald 返回登录成功响应，包含指向第三方 callback 地址的重定向信息
```

**场景 3：使用授权码 + PKCE code_verifier 换取令牌**
```gherkin
Given 第三方后端获得了 authorization_code 和用户原始的 code_verifier
When 第三方后端向 Herald 令牌端点提交 authorization_code、redirect_uri、client_id 和 code_verifier
Then Herald 校验 code 有效且未使用
And Herald 验证 code_verifier 的 SHA256 值与存储的 code_challenge 匹配
Then Herald 创建会话并返回 access_token
```

**场景 4：授权码重放被拒绝（失败场景）**
```gherkin
Given 第三方后端已使用 authorization_code 成功换取令牌
When 第三方后端再次使用相同的 authorization_code 请求令牌
Then Herald 返回错误，提示授权码无效或已使用
```

**场景 5：授权码过期（失败场景）**
```gherkin
Given authorization_code 已超过有效期
When 第三方后端使用该授权码请求令牌
Then Herald 返回错误，提示授权码已过期
```

**场景 6：PKCE 校验失败（失败场景）**
```gherkin
Given 第三方后端提交的 code_verifier 与授权时提交的 code_challenge 不匹配
When 第三方后端请求令牌
Then Herald 返回错误，提示 PKCE 验证失败
```

**场景 7：State 不存在或不匹配（失败场景）**
```gherkin
Given 用户登录时提交的 state 不存在或参数不匹配
When Herald 校验 state
Then Herald 返回错误，登录失败
```

**场景 8：回调地址不在白名单（失败场景）**
```gherkin
Given Client App 配置的白名单为 "https://myapp.com/callback"
When 授权请求中的 redirect_uri 为 "https://evil.com/callback"
Then Herald 拒绝授权请求并返回错误
```

**场景 9：redirect_uri 前缀绕过被拒绝（失败场景）**
```gherkin
Given Client App 配置的白名单为 "https://myapp.com/callback"
When 授权请求中的 redirect_uri 为 "https://myapp.com.evil.com/callback"
Then Herald 拒绝授权请求（白名单精确匹配，不允许前缀绕过）
```

---

## 故事 2：验证用户登录状态 [US-TP-002]

**优先级**: P0

**【用户故事】**
**作为**：第三方应用开发者（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：能够验证用户的登录状态和身份
**从而**：保护应用资源，只允许已登录用户访问

**【验收标准】**

**场景 1：验证用户已登录**
```gherkin
Given 用户已通过 Herald 登录并获得访问令牌
When 第三方应用使用该令牌请求验证用户登录状态
Then 系统确认用户已登录，返回用户标识
```

**场景 2：验证用户未登录（失败场景）**
```gherkin
Given 用户未登录或令牌已过期
When 第三方应用使用无效的令牌请求验证
Then 系统返回未授权，不允许访问
```

**场景 3：令牌格式错误（失败场景）**
```gherkin
Given 第三方应用使用格式错误的令牌
When 请求验证用户登录状态
Then 系统返回错误，提示令牌格式无效
```

**场景 4：令牌过期（失败场景）**
```gherkin
Given 用户的访问令牌已超过有效期
When 第三方应用使用该令牌请求验证
Then 系统返回未授权，提示令牌已过期
```

---

## 故事 3：检查用户权限 [US-TP-003]

**优先级**: P0

**【用户故事】**
**作为**：第三方应用开发者（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：能够检查用户是否有权限访问特定资源
**从而**：实现细粒度的访问控制

**【验收标准】**

**场景 1：用户有权限访问资源**
```gherkin
Given 用户拥有某资源的读取权限
When 第三方应用请求检查该用户的资源权限
Then 系统返回允许，附带用户标识
```

**场景 2：用户无权限访问资源（失败场景）**
```gherkin
Given 用户没有某管理资源的访问权限
When 第三方应用请求检查该用户对该资源的权限
Then 系统返回不允许
```

**场景 3：批量检查多个权限**
```gherkin
Given 用户拥有多个资源的不同权限
When 第三方应用一次请求检查多个资源的权限
Then 系统返回权限检查结果（首次拒绝即停止，不继续检查后续规则）
```

**场景 4：所有权限检查通过**
```gherkin
Given 用户拥有请求的所有权限
When 第三方应用请求检查所有权限
Then 系统返回全部允许，附带用户标识
```

---

## 故事 4：获取用户信息 [US-TP-004]

**优先级**: P0

**【用户故事】**
**作为**：第三方应用开发者（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：能够获取已登录用户的基本信息
**从而**：个性化应用体验

**【验收标准】**

**场景 1：获取用户信息成功**
```gherkin
Given 用户已登录并获得访问令牌
When 第三方应用使用该令牌请求用户信息
Then 系统返回用户的基本信息（包括用户标识、邮箱、昵称和状态）
```

**场景 2：令牌无效（失败场景）**
```gherkin
Given 第三方应用使用无效的访问令牌
When 请求用户信息
Then 系统返回未授权错误
```

**场景 3：令牌过期（失败场景）**
```gherkin
Given 访问令牌已过期
When 请求用户信息
Then 系统返回未授权错误，提示令牌已过期
```

---

## 故事 5：Client App 配置管理 [US-TP-005]

**优先级**: P0

**【用户故事】**
**作为**：Realm 管理员（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：能够正确配置 Client App 信息
**从而**：第三方应用能正常接入 Herald 系统

**【验收标准】**

**场景 1：管理员创建 Client App**
```gherkin
Given 管理员在 realm 中创建 Client App
When 管理员输入 client_id、名称和描述
Then Client App 创建成功，系统自动生成 client_secret
```

**场景 2：Client App 配置正确**
```gherkin
Given Client App 已创建
When 第三方应用使用正确的 client_id 和 client_secret
Then 成功完成 OAuth 流程
```

**场景 3：回调 URL 验证**
```gherkin
Given Client App 配置的回调 URL 为 "https://myapp.com/callback"
When OAuth 授权成功后重定向
Then 系统重定向到配置的回调 URL
```

**场景 4：回调 URL 不匹配（失败场景）**
```gherkin
Given OAuth 请求中的 redirect_uri 为 "https://evil.com/callback"
When Herald 系统验证回调 URL
Then 拒绝授权请求并返回错误
```

**场景 5：Client App 被禁用（失败场景）**
```gherkin
Given Client App 的状态为禁用
When 第三方应用尝试发起 OAuth 授权
Then 系统返回错误，提示 Client App 已禁用
```

---

## 故事 6：处理异常情况 [US-TP-006]

**优先级**: P1

**【用户故事】**
**作为**：第三方应用开发者（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：能够正确处理各种异常情况
**从而**：提供友好的用户体验

**【验收标准】**

**场景 1：用户拒绝授权**
```gherkin
Given 用户在 Herald 授权页面点击"拒绝"
When Herald 系统重定向回第三方应用
Then 重定向 URL 参数包含错误标识和 state 信息
```

**场景 2：网络请求超时**
```gherkin
Given 第三方应用调用 Herald 服务时网络超时
When 超过预设的超时时间
Then 第三方应用显示"服务暂时不可用，请稍后重试"
```

**场景 3：Herald 服务不可用**
```gherkin
Given Herald 服务宕机或网络中断
When 第三方应用尝试调用 Herald 服务
Then 第三方应用显示友好错误页面
```

**场景 4：重复提交授权码（失败场景）**
```gherkin
Given 第三方应用意外重复使用相同授权码
When 第二次使用该授权码请求令牌
Then 系统返回错误，提示授权码无效
```

---

## 故事 7：会话管理 [US-TP-007]

**优先级**: P1

**【用户故事】**
**作为**：第三方应用开发者（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：能够管理用户的登录会话
**从而**：实现单点登录（SSO）和登出

**【验收标准】**

**场景 1：会话保持**
```gherkin
Given 用户已登录并获得访问令牌
When 令牌在有效期内
Then 用户可以持续访问第三方应用，无需重新登录
```

**场景 2：会话过期**
```gherkin
Given 用户的访问令牌已超过有效期
When 用户访问第三方应用
Then 第三方应用重定向到 Herald 登录页面
```

**场景 3：单点登出**
```gherkin
Given 用户在 Herald 系统中登出
When 用户访问第三方应用
Then 第三方应用检测到会话失效并重定向到登录页面
```

**场景 4：令牌刷新策略**
```gherkin
Given 访问令牌即将过期
When 第三方应用检查令牌有效期
Then 提示用户重新登录以获取新令牌
```

---

## 故事 8：第三方 Web SPA 发起 SSO 登录 [US-TP-015]

**优先级**: P0

**【用户故事】**
**作为**：第三方应用开发者（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：从 Web SPA 直接发起 Herald SSO 登录（Authorization Code + PKCE）
**从而**：前端用户无需额外后端参与即可完成认证流程的发起

**【验收标准】**

**场景 1：SPA 生成 PKCE 参数并跳转 Herald 登录**
```gherkin
Given 第三方 SPA 已在 Herald 注册 Client App "my-app"
When SPA 在浏览器中生成随机 code_verifier
And 计算 code_challenge（SHA256 哈希 + Base64url 编码）
And 将用户重定向到 Herald 授权端点，携带 client_id、redirect_uri、state、response_type=code、code_challenge、code_challenge_method=S256
Then Herald 校验参数通过，将用户重定向到登录页面
```

**场景 2：SPA 回调页面接收授权码**
```gherkin
Given 用户在 Herald 完成登录
When Herald 将用户重定向回 SPA 的 callback 地址
Then 回调 URL 的查询参数中包含授权码和 state
And SPA 校验返回的 state 与发起时一致
```

**场景 3：SPA 将授权码发送给后端**
```gherkin
Given SPA 在回调页面获得了授权码和 state
When SPA 将授权码发送给自己的后端服务
And 后端使用授权码和之前保存的 code_verifier 向 Herald 请求令牌
Then 后端获得 access_token 并完成认证
```

**场景 4：OAuth 参数不完整时 SPA 显示错误（失败场景）**
```gherkin
Given SPA 生成的 PKCE 参数中缺少 code_challenge
When SPA 尝试跳转到 Herald 授权端点
Then 授权请求被拒绝，提示参数不完整
```

**场景 5：state 不匹配时 SPA 拒绝授权码（失败场景）**
```gherkin
Given SPA 发起授权时使用了 state="abc123"
When Herald 回调时返回的 state="xyz789"
Then SPA 拒绝接受该授权码，提示可能的安全风险
```

---

## 故事 9：第三方后端用授权码换取令牌 [US-TP-016]

**优先级**: P0

**【用户故事】**
**作为**：第三方应用开发者（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：后端使用授权码和 PKCE code_verifier 向 Herald 换取 access_token
**从而**：安全地完成用户认证，token 不经过用户浏览器

**【验收标准】**

**场景 1：成功换取令牌**
```gherkin
Given 第三方后端拥有有效的 authorization_code 和原始 code_verifier
When 后端向 Herald 提交 authorization_code、redirect_uri、client_id 和 code_verifier
Then Herald 校验全部通过，返回 access_token、token_type 和 expires_in
```

**场景 2：code_verifier 不匹配（失败场景）**
```gherkin
Given 第三方后端提交的 code_verifier 与授权时存储的 code_challenge 不对应
When 后端请求换取令牌
Then Herald 返回错误，提示 PKCE 验证失败
```

**场景 3：client_id 不匹配（失败场景）**
```gherkin
Given 授权码是为 client_id="my-app" 生成的
When 后端使用 client_id="other-app" 请求令牌
Then Herald 返回错误，提示客户端不匹配
```

**场景 4：redirect_uri 不匹配（失败场景）**
```gherkin
Given 授权时使用的 redirect_uri 为 "https://myapp.com/callback"
When 后端提交不同的 redirect_uri 请求令牌
Then Herald 返回错误，提示回调地址不匹配
```

**场景 5：授权码已使用（失败场景）**
```gherkin
Given 同一个授权码已被成功使用一次
When 后端再次使用该授权码请求令牌
Then Herald 返回错误，提示授权码无效
```

> **与自建用户 UI 的关系**：本故事描述的是"后端用授权码换令牌、token 不经过用户浏览器"的经典 OAuth 后端换码场景。另存在一条**集成方自建 UI 的浏览器 token 路线**——集成方前端经 `/login` 直接获得进入用户浏览器的 `CustomUserUi` token，用于跨域自建个人中心；该路线不经过授权码换令牌链路。两条路线并存，互不取代。详见 [自建用户 UI PRD](/docs/prd/integration/custom-user-ui.md) 与 [自建用户 UI 用户故事](/docs/user-stories/integration/custom-user-ui.md)。

---

## 备注

### 业务规则

> 业务规则的唯一定义点是 PRD §4（OAuth 授权流程与凭证安全、redirect_uris 白名单、第三方 API 接入与 API Key、Client App 会话 Cookie 配置、Realm 隔离边界）：[docs/prd/auth/oauth.md](/docs/prd/auth/oauth.md)（OAuth 授权与第三方 API）、[docs/prd/integration/client-app.md](/docs/prd/integration/client-app.md)（Client App 配置与会话）、[docs/prd/integration/api-key-roles.md](/docs/prd/integration/api-key-roles.md)（API Key 角色绑定与作用域）。本文场景中的数值口径以 PRD 为准。

---

## 相关文档

- **OAuth Provider**: [docs/prd/auth/oauth.md](/docs/prd/auth/oauth.md)
- **Client Apps 管理**: [docs/prd/integration/client-app.md](/docs/prd/integration/client-app.md)
- **权限验证**: [docs/prd/auth/permissions.md](/docs/prd/auth/permissions.md)
- **计费系统**: [docs/prd/billing/subscription.md](/docs/prd/billing/subscription.md)
