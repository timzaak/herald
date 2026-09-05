# Apple native 登录 用户故事

> 角色定义见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)

## 用户故事

### 故事 1：在 iOS App 内使用 Apple 账号一键登录 [US-AL-001]

**优先级**: P0

**【用户故事】**
**作为**：普通用户（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：在接入 Herald 的 iOS App 内，通过苹果系统弹窗完成 Apple 登录，无需跳转浏览器或输入密码
**从而**：用最少的操作在 App 内完成登录，不被页面跳转打断

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：已注册用户通过 Apple native 登录成功**
```gherkin
Given 接入方已在 iOS App 中接入 Apple 原生登录
And Herald 中该 Realm 已配置并启用 Apple Provider
And 用户已在该 Realm 拥有账号（之前通过 Apple web 登录或其他方式注册）
When 用户在 iOS App 内点击「使用 Apple 登录」并在系统弹窗中确认
Then 用户直接登录成功，App 显示已登录状态
And 全程不离开当前 App
```

**场景 2：未注册用户首次通过 Apple native 登录自动创建账号**
```gherkin
Given 用户在该 Realm 没有任何 Herald 账号
And 接入方已在 iOS App 中接入 Apple 原生登录
And 该 Realm 已允许用户注册
When 用户在 iOS App 内点击「使用 Apple 登录」并在系统弹窗中确认
Then 系统自动为该 Apple 身份创建 Herald 账号
And 用户登录成功，App 显示已登录状态
```

**场景 3：用户隐藏真实邮箱（使用 Apple 中转邮箱）首次登录**
```gherkin
Given 用户首次使用 Apple 登录
And 用户在 Apple 系统弹窗中选择「隐藏邮箱」（Apple 返回中转邮箱）
When 用户确认登录
Then 系统使用 Apple 提供的中转邮箱创建 Herald 账号
And 用户登录成功
```

**场景 4：用户取消 Apple 授权弹窗**
```gherkin
Given 用户在 iOS App 内触发了 Apple 登录
When 用户在系统弹窗中点击取消或不允许授权
Then App 停留在未登录状态
And 不创建任何账号或会话
```

---

### 故事 2：接入方在 iOS App 中集成 Apple native 登录 [US-AL-002]

**优先级**: P0

**【用户故事】**
**作为**：第三方应用开发者（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：在我的 iOS App 中接入 Apple 原生登录，用户完成 Apple 授权后，由 Herald 后端校验 Apple 凭证并建立会话或签发下游授权码
**从而**：在 iOS 原生体验下提供 Apple 登录，同时保持 Herald 统一的用户管理和安全控制

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：接入方通过直接会话方式集成（第一方 Client App）**
```gherkin
Given 接入方的 iOS App 对应 Herald 中一个已配置的 Client App（第一方）
And Herald 中该 Realm 已配置并启用 Apple Provider
When 用户在 iOS App 内完成 Apple 原生授权，App 将 Apple 签发的身份凭证提交给 Herald
Then Herald 校验该凭证有效（含签名、签发者、受众），拒绝被篡改或受众不符的凭证
And Herald 确认用户身份并返回该 App 的登录会话
And App 进入已登录状态
```

**场景 2：接入方通过下游授权码方式集成（第三方 Client App，Code+PKCE）**
```gherkin
Given 接入方的 iOS App 通过 Authorization Code + PKCE 流程接入 Herald
And 用户已在 Herald 发起过该次登录的下游授权事务
When 用户在 iOS App 内完成 Apple 原生授权，App 将 Apple 身份凭证和下游授权交易标识一起提交给 Herald
Then Herald 校验 Apple 凭证通过后签发一次性授权码
And 接入方使用授权码和 PKCE 验证值换取访问令牌
```

**场景 3：Apple 凭证被篡改或伪造（失败场景）**
```gherkin
Given iOS App 提交给 Herald 的 Apple 身份凭证签名无效
When Herald 后端校验签名失败
Then Herald 拒绝该凭证，返回认证失败
And App 登录失败，不创建任何会话或用户
```

**场景 4：Apple 凭证已过期（失败场景）**
```gherkin
Given iOS App 提交给 Herald 的 Apple 身份凭证已超过有效期
When Herald 后端校验凭证有效期
Then Herald 拒绝该凭证，返回认证失败
```

**场景 5：Apple 凭证的受众不匹配（失败场景）**
```gherkin
Given iOS App 提交的 Apple 身份凭证中的受众与 Herald 该 Realm 配置的 Apple Client ID 不一致
When Herald 后端校验受众
Then Herald 拒绝该凭证，返回认证失败
```

**场景 6：Realm 未配置 Apple Provider（失败场景）**
```gherkin
Given Herald 中该 Realm 未配置或已禁用 Apple Provider
When iOS App 尝试使用 Apple 身份凭证向 Herald 发起认证
Then Herald 返回错误，提示 Apple Provider 未配置
And 不创建任何会话或用户
```

---

### 故事 3：Apple native 登录与已有账号关联 [US-AL-003]

**优先级**: P1

**【用户故事】**
**作为**：普通用户（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：在 iOS App 内用 Apple 登录时，系统能识别我已有的 Herald 账号（即使之前通过 Apple web 登录或其他方式注册）
**从而**：不会因为使用 native 登录而产生重复账号，多个入口关联到同一个我

**【验收标准】**

**场景 1：同一 Apple 用户在 web 与 native 两条路径关联到同一账号**
```gherkin
Given 用户之前已通过 Apple web 跳转登录在该 Realm 关联了 Herald 账号
When 用户在 iOS App 内用同一 Apple 账号完成 native 登录
Then 系统通过 Apple 用户唯一标识识别到已有关联
And 用户登录成功，使用同一账号，不产生重复账号
```

**场景 2：Apple native 登录邮箱与已有账号邮箱一致**
```gherkin
Given 用户之前用邮箱密码或其他方式在 Herald 注册了账号
And Apple 本次返回的邮箱与该账号邮箱一致
When 用户在 iOS App 内完成 Apple native 登录
Then 系统识别到已有账号，将 Apple 身份关联到该账号
And 用户登录成功，使用同一账号
```

**场景 3：首次 native 登录但 Apple 未返回邮箱（建号不失败）**
```gherkin
Given 用户首次用 Apple native 登录
And Apple 本次未在凭证中返回邮箱（非首次授权或被隐藏且未走中转邮箱）
When 用户完成 Apple native 登录
Then 系统仍创建 Herald 账号（使用占位邮箱，标记为未验证）
And 用户登录成功，不因邮箱缺失而被拒绝
```

若账号创建成功后 provider link 因瞬时故障未落账，使用同一已验签 Apple `sub` 重试时可重新认领由该 `sub` 确定性生成的占位邮箱并补建关联；该例外不适用于任意未验证真实邮箱。

---

## 备注

### 业务规则

1. Apple native 登录在 **iOS App 内**触发，由接入方的 iOS App 调用苹果系统原生授权弹窗；Herald 仅作为后端凭证校验与会话签发方，本仓库不包含 iOS App。
2. Herald 在此场景中接收 Apple 签发的身份凭证，在服务端校验签名、签发者、受众和有效期，不得信任 App 传来的任何明文用户信息。
3. 用户匹配策略与现有 Apple web 跳转登录一致：通过 Apple 用户唯一标识（sub）优先匹配，其次邮箱，最后创建新用户；保证同一 Apple 用户在 web 与 native 两条路径关联到同一 Herald 账号。
4. 端点支持两种会话建立方式：直接会话（第一方 Client App，Herald 直接签发该 App 的会话）和下游授权码（第三方 Client App，Code+PKCE，Herald 签发一次性授权码）。
5. 邮箱处理与 Apple web 跳转登录有意不同：Apple native 凭证在非首次授权时恒不返回邮箱，故 native 路径在首次建号且邮箱缺失时生成占位邮箱并标记未验证（不拒绝），与微信占位邮箱策略一致；Apple 中转邮箱是合法可收信地址，作真实邮箱处理。
6. Apple Provider 复用现有配置（Client ID、启用状态），不新增 native 专用配置项；Realm 启用 Apple Provider 即可使用 native 登录。
7. 本能力不代理调用 Apple 上游接口、不存 Apple 访问令牌或刷新令牌，只做身份落账。

### 与现有用户故事的关系

- 扩展 [US-RU-003](/docs/user-stories/core/regular-user.md) OAuth 第三方登录，提供 iOS 原生场景下的 Apple 登录入口
- 与 [US-TP-001](/docs/user-stories/auth/third-party-app.md) OAuth 授权码登录兼容，native 登录可嵌入下游 Code+PKCE 流程
- 用户匹配逻辑复用现有 OAuth 回调的 `find_or_create_user` 策略，账号关联语义与 Google One Tap 同构
- Apple web 跳转登录（[OAuth](/docs/prd/auth/oauth.md)）与本 native 登录共存，关联同一 Apple 用户

---

## 相关文档

- **PRD**: [docs/prd/auth/support-mobile-apple-login.md](/docs/prd/auth/support-mobile-apple-login.md)
- **OAuth 第三方登录**: [docs/user-stories/core/regular-user.md](/docs/user-stories/core/regular-user.md)
- **第三方应用接入**: [docs/user-stories/auth/third-party-app.md](/docs/user-stories/auth/third-party-app.md)
- **Google One Tap 登录（架构参照）**: [docs/prd/auth/google-one-tap.md](/docs/prd/auth/google-one-tap.md)
- **微信 OAuth（占位邮箱范式参照）**: [docs/prd/auth/wechat-oauth.md](/docs/prd/auth/wechat-oauth.md)
- **OAuth Provider 配置管理**: [docs/user-stories/auth/oauth-extension.md](/docs/user-stories/auth/oauth-extension.md)
- **角色定义**: [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)
