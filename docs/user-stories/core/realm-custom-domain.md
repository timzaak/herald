# Realm 自定义域名（Custom Domain）用户故事

> 角色定义见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)

## 用户故事

### 故事 1：为本 Realm 配置自定义登录域名 [US-CD-001]

**优先级**: P0

**【用户故事】**
**作为**：Realm Admin（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：在管理后台为本 Realm 配置一个自定义登录域名（如 `login.acme.com`），并按系统指引将该域名 CNAME 到 Herald 指定的 hostname
**从而**：为本 Realm 建立自有品牌登录域名配置，并让 Herald 仅对已注册的该域名授权签发证书

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：成功配置并保存自定义域名**
```gherkin
Given 我是 realm-1 的 Realm Admin
And 我已进入管理后台 Settings 页面的自定义域名配置入口
When 我填入一个尚未被其他 Realm 占用的自定义域名
And 我保存配置
Then 配置保存成功并立即生效
And 配置仅关联到 realm-1，不影响其他 Realm
```

**场景 2：系统提供 CNAME 指引并展示生效状态**
```gherkin
Given realm-1 已配置自定义域名 login.acme.com
When 我查看该自定义域名配置
Then 系统向我展示需要 CNAME 到的 Herald 指定 hostname
And 系统展示当前域名生效状态（如 CNAME 是否已正确指向、TLS 是否就绪）
```

**场景 3：自定义域名全局唯一**
```gherkin
Given login.acme.com 已被 realm-1 配置占用
When 我尝试在 realm-2 配置相同的 login.acme.com
Then 系统拒绝该配置并提示该域名已被占用
```

**场景 4：仅 Realm Admin 可配置**
```gherkin
Given 我是 realm-1 的普通用户（Regular User）
When 我尝试访问管理后台的自定义域名配置入口
Then 系统拒绝访问
And 我看不到该配置入口
```

---

### 故事 2：自定义域名配置保存即生效 [US-CD-003]

**优先级**: P1

**【用户故事】**
**作为**：Realm Admin（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：自定义域名配置保存后立即生效（写入域名注册映射，授权签发证书），无需单独的发布步骤
**从而**：以最简流程完成自定义域名配置；填错域名时直接重新保存正确域名即可覆盖

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：保存后域名注册映射更新**
```gherkin
Given 我是 realm-1 的 Realm Admin
And 我已进入管理后台的自定义域名配置入口
When 我填入自定义域名 login2.acme.com 并保存
Then 系统将 login2.acme.com 置为当前生效的自定义域名
And 该域名被注册到 Herald 的域名注册映射中
And 证书授权门控据此开始为 login2.acme.com 授权
```

**场景 2：切换域名覆盖当前配置**
```gherkin
Given realm-1 当前生效自定义域名为 login.acme.com
When 我填入新的域名 login2.acme.com 并保存
Then 系统将 login2.acme.com 置为当前生效的自定义域名
And 域名注册映射更新为 login2.acme.com
```

**场景 3：清空域名移除映射**
```gherkin
Given realm-1 当前生效自定义域名为 login.acme.com
When 我清空域名并保存
Then 该 Realm 不再有任何生效的自定义域名
And 域名注册映射中不再包含该 Realm 的条目
```

---

### 故事 3：未授权域名访问的拒绝 [US-CD-005]

**优先级**: P1

**【用户故事】**
**作为**：Regular User（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：访问一个未在任何 Realm 注册的自定义域名时，Herald 不会为该域名授权签发证书
**从而**：避免 Herald 为未授权域名签发证书，防止证书滥用与钓鱼

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：未注册自定义域名不被授权签发证书**
```gherkin
Given evil.com 未在任何 Realm 注册为自定义域名
When 证书签发流程向 Herald 查询 evil.com 是否可签发
Then Herald 不授权为 evil.com 签发证书
```

**场景 2：已注册并生效的域名被授权签发证书**
```gherkin
Given login.acme.com 已在 realm-1 注册并保存生效
When 证书签发流程向 Herald 查询 login.acme.com 是否可签发
Then Herald 授权为 login.acme.com 签发证书
```

**场景 3：证书授权查询不泄露 Realm 身份**
```gherkin
Given 任意域名（无论是否注册）
When 证书签发流程向 Herald 查询该域名是否可签发
Then Herald 的响应仅表明是否授权
And 不泄露该域名关联的 Realm 身份或其他信息
```

---

## 已交付的历史 Deferred 项

以下两项原列为本 feature 的未来范围（当时 host→realm 解析机制尚未交付）。域名绑定解析与前端 SPA 托管已交付（见 [docs/prd/core/realm-custom-domain.md](/docs/prd/core/realm-custom-domain.md) §2.3 与 [docs/prd/core/frontend-realm-isolation.md](/docs/prd/core/frontend-realm-isolation.md)），两项均属当前已发布能力：

- **终端用户在自定义域名下完成 auth 流**（已交付）：终端用户可在租户的自定义域名（如 `login.acme.com`）下完成登录/注册及其他 auth 流，URL 保持该自有品牌域名，收到的邮件链接也指向该自定义域名。
- **自定义域名与 path-based canonical 域名并存**（已交付）：配置自定义域名后，原有 `herald.com/{realmId}` 路径入口仍可用，两条入口同时可用，不强制迁移；两种入口的确定规则与标签级会话隔离见 frontend-realm-isolation PRD。
