# Herald 用户角色定义参考

> 本文档定义 Herald 系统中的所有用户角色，供编写用户故事时参考。
> 各用户故事文件中的"角色定义"章节应引用本文档，而非重复定义。

---

## 角色层级关系

```
Admin Realm 管理员 (realm-admin 角色 + realm.manage in admin realm)
    ├── 平台级别权限（仅限 Realm 创建和管理）
    └── ❌ 不能访问其他 Realm 内部资源

Realm Admin (realm-admin 角色)
    ├── 单 Realm 权限
    └── 仅管理本 Realm 资源

Regular User (user 角色)
    ├── 基本登录权限
    └── 仅拥有 points.view

Third-party App (Principal Type: CLIENT)
    ├── 客户端凭证认证
    └── 服务间调用

API Key (Principal Type: API_KEY)
    ├── API 密钥认证
    └── 第三方集成

Herald 系统（System Actor，非角色）
    ├── 系统级定时任务
    ├── Webhook 回调处理
    └── 状态同步
```

---

## 角色总览

| 角色 | 技术标识 | 权限范围 | 初始化/创建方式 |
|------|----------|----------|----------------|
| Admin Realm 管理员 | `realm-admin` | 平台级别（Realm管理 via `realm.manage`）+ Admin Realm内部 | 系统初始化创建 `admin@cas.com` |
| Realm Admin | `realm-admin` | 仅本 Realm | 由 Admin Realm 管理员创建 Realm 时指定 |
| Regular User | `user` | 个人资源 | 用户注册或由管理员创建 |
| Third-party App | — | 客户端凭证认证（Principal Type: `CLIENT`） | 在管理后台注册客户端应用 |
| API Key | — | API 密钥访问（Principal Type: `API_KEY`） | 在管理后台创建 API Key |
| Herald 系统 | — | 系统级任务和回调处理（非角色，非 Principal） | N/A |

---

## 角色详细定义

### Admin Realm 管理员（Admin Realm Admin）

**技术标识**：`realm-admin`
**初始化账号**：`admin@cas.com`（在 admin realm 中）
**权限范围**：平台级别（Realm管理）+ Admin Realm内部资源访问

**权限清单**：

| 权限项 | 说明 |
|--------|------|
| dashboard.view | 查看 Dashboard 统计 |
| realm.view | 查看 Realm 信息 |
| realm.manage | Realm 创建、更新（仅 admin realm；Realm 删除不受支持） |
| users.view | 查看用户 |
| users.manage | 用户管理（CRUD） |
| clients.view | 查看客户端应用 |
| clients.manage | 客户端应用管理 |
| roles.view | 查看角色 |
| roles.manage | 角色管理 |
| permissions.view | 查看权限 |
| permissions.manage | 权限管理 |
| policies.view | 查看策略 |
| policies.manage | 策略管理 |
| settings.view | 查看设置 |
| settings.manage | 设置管理 |
| api_keys.view | 查看 API Key 列表和详情 |
| api_keys.manage | API Key 创建、更新、删除、轮换 |
| billing.view | 查看账单、订阅历史 |
| billing.manage | 账单业务管理（账单、订阅、发票等业务对象；支付 Provider 凭证配置由 settings.view/settings.manage 门控，不含于本权限） |
| points.view | 查看积分、积分规则 |
| points.manage | 积分管理、Provider 映射管理 |
| audit.view | 查看审计日志列表和详情 |

**边界约束**：
- ❌ 不能访问其他 Realm 内部资源（用户、角色、配置、客户端应用）
- ❌ 不能创建跨 Realm 策略
- ❌ 不能管理其他 Realm 的 RBAC 元数据
- ✅ 仅负责平台级别的 Realm 生命周期管理（通过 `realm.manage` in admin realm）
- ✅ Realm 创建后，内部管理由该 Realm 的 Realm Admin 负责
- ✅ 可以访问 admin realm 的所有内部资源

---

### Realm Admin（Realm 管理员）

**技术标识**：`realm-admin`
**权限范围**：仅限所属 Realm

**权限清单**：

| 权限项 | 说明 |
|--------|------|
| dashboard.view | 查看 Dashboard 统计 |
| users.view | 查看用户 |
| users.manage | 用户管理（CRUD） |
| clients.view | 查看客户端应用 |
| clients.manage | 客户端应用管理 |
| roles.view | 查看角色 |
| roles.manage | 角色管理 |
| permissions.view | 查看权限 |
| permissions.manage | 权限管理 |
| policies.view | 查看策略 |
| policies.manage | 策略管理 |
| settings.view | 查看设置 |
| settings.manage | 设置管理 |
| api_keys.view | 查看 API Key 列表和详情 |
| api_keys.manage | API Key 创建、更新、删除、轮换 |
| billing.view | 查看账单、订阅历史 |
| billing.manage | 账单业务管理（账单、订阅、发票等业务对象；支付 Provider 凭证配置由 settings.view/settings.manage 门控，不含于本权限） |
| points.view | 查看积分、积分规则 |
| points.manage | 积分管理、Provider 映射管理 |
| audit.view | 查看审计日志列表和详情 |

> **与 Admin Realm 管理员的差异**：Realm Admin 缺少 `realm.view` 和 `realm.manage` 权限（这些仅在 admin realm 中存在），其余权限相同。

**边界约束**：
- 无法创建 `resource: "All"` 策略
- 策略必须通过 `validate_policy_for_realm_admin` 校验
- 无法跨 Realm 查看或操作
- 仅能管理本 Realm 的资源和用户

---

### Regular User（普通用户）

**技术标识**：`user`
**认证方式**：邮箱密码 / OAuth/OIDC

**权限清单**：

| 权限项 | 说明 |
|--------|------|
| points.view | 查看自己的积分余额 |

**能力清单**（业务逻辑层处理，无需权限检查）：

| 能力项 | 说明 |
|--------|------|
| 登录 | 密码登录或第三方登录（Google/GitHub等） |
| 个人信息管理 | 查看昵称、更新邮箱 |
| 密码管理 | 修改密码、重置密码（通过邮箱链接） |
| 资源访问 | 访问被授权的应用资源（基于 OAuth/OIDC） |

**边界约束**：
- 无法访问管理后台（`/admin/*` 路径）
- 无法管理其他用户
- 资源访问需基于授权策略
- 初始状态为 `WaitVerified`，需验证邮箱后变为 `Normal`

---

### Third-party App（第三方应用）

**Principal Type**：`CLIENT`
**认证方式**：Client Credentials（客户端凭证）

> 第三方应用不是内置角色，而是通过 Principal Type `CLIENT` 进行身份识别。其权限通过角色分配和策略配置实现。

**能力清单**：

| 能力项 | 说明 |
|--------|------|
| 服务间调用 | 使用 `client_id` + `client_secret` 获取 token |
| API 访问 | 携带 token 调用受保护的 API |
| 多 Realm 支持 | 可配置多个 Realm 的访问权限 |

**边界约束**：
- 无法访问管理后台
- 无用户会话概念（无 UI 登录）
- 需预先在管理后台注册客户端应用（生成 `client_id`/`client_secret`）
- Token 有效期限制

---

### Herald 系统（System Actor）

**说明**：系统级 Actor，代表 Herald 后台自动执行的定时任务、回调处理、状态同步等非用户触发的行为。不对应真实用户，不是内置角色也不是 Principal Type，仅在用户故事中作为角色使用。

**典型场景**：
- Webhook 回调处理（支付回调、OAuth 回调等）
- 定时任务（过期订单清理、补偿查询等）
- 系统间状态同步

---

## Persona 别名表

用户故事与索引中出现的下述 persona 是基础角色在特定能力语境下的别名，不是独立角色、没有独立权限集；权限判定一律按其基础角色执行。

| Persona | 基础角色 | 语境 |
|---------|----------|------|
| TOTP User | Regular User / Realm Admin | 启用 TOTP 二次认证的用户（auth/totp.md） |
| Passkey User | Regular User / Realm Admin | 注册 Passkey 的用户（auth/passkey.md） |
| Points Admin | Realm Admin | 行使积分策略管理职责（billing/points-admin.md） |
| Points User | Regular User | 行使积分查询/消费职责（billing/points-user.md） |
| Free User | Regular User | 仅享受免费积分策略的用户（billing/points-free-user.md） |
| Billing User | Regular User | 持有订阅/账单的用户（billing/subscription.md） |
| SaaS 自助注册访客 | （未认证访客） | 自助开通流程中的注册者（core/realm-create.md） |
| 新 realm-admin | Realm Admin | 刚自助开通 Realm 的首任管理员（core/realm-create.md） |
| 第三方应用开发者 | Third-Party App | 集成 Herald 的开发者视角（auth/openid-connect.md、auth/google-one-tap.md） |

---

## 代码实现映射

| 用户故事角色 | 后端内置角色 | 后端 Principal Type | 后端校验方式 |
|--------------|-------------|---------------------|-------------|
| Admin Realm 管理员 | `realm-admin`（+ `realm.manage` in admin realm） | `USER` | 具体权限检查 + `Identity::has_access_to_realm` |
| Realm Admin | `realm-admin` | `USER` | 具体权限检查 + `Identity::has_access_to_realm` |
| Regular User | `user` | `USER` | 业务逻辑层处理 |
| Third-party App | — | `CLIENT` | 客户端凭证流程（`/oauth/token`） |
| API Key | — | `API_KEY` | API Key 凭证流程 |

---

## Principal Types

权限系统通过 Principal Type 识别请求方身份，后端定义三种类型：

| Principal Type | 标识 | 说明 |
|---------------|------|------|
| User | `user` | 已登录的用户，通过邮箱密码或 OAuth 认证 |
| API Key | `api_key` | 通过 API Key 凭证访问的第三方集成 |
| Client | `client` | 通过 Client Credentials 认证的 OAuth 客户端应用 |

权限检查时通过 `check_principal_permission` 方法按 Principal Type 查找角色和策略。

API Key 还携带 Client App 作用域：
- 绑定 `admin-api-client` 时，API Key 的资源范围为所属 Realm。
- 绑定普通 Client App 时，API Key 只能访问该 Client App 的资源；用户等 Realm 级资源仍按 Realm 隔离和 RBAC 权限控制。
- 禁用绑定的 Client App 会使该 Client App 下的 API Key 无法通过外部 API 认证。

---

## 权限层级

运行时权限检查实现了 action 层级匹配：

```
manage > create, view
```

- 拥有 `manage` action 自动包含 `create` 和 `view`
- 拥有 `create` action 仅匹配自身，不隐含 `view`
- 拥有 `view` action 仅匹配自身
- 所有层级规则仅在**同一 resource 内**生效
- 不使用 `admin` action，不引入特殊 `resource:action` 组合
- 不引入隐式全局权限

示例：`realm-admin` 同时拥有 `users.view` 和 `users.manage`，即使层级规则使 `users.manage` 隐含 `users.view`，两者在 RBAC 元数据层都会被创建和存储。

---

## 使用指南

编写新用户故事时：

1. **在用户故事文件开头引用本文档**：
   ```markdown
   **作为**：Admin Realm 管理员（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
   ```

2. **避免重复定义**：不要在每个故事文件中重复完整的角色定义

3. **更新角色定义时**：仅修改本文档，确保全局一致性

4. **新增角色时**：
   - 在本文档添加新角色定义
   - 同步更新后端 `backend/domain/src/rbac_init/services.rs` 的初始化逻辑
   - 同步更新后端 `backend/infra/src/authorization/policies.rs` 的策略实现

---

## 架构设计约束

### 当前架构

```
Admin Realm 管理员 (realm-admin 角色 + realm.manage in admin realm)
    ├── 平台级权限（创建、更新 Realm via realm.manage；删除不受支持）
    ├── Admin Realm 内部权限（具体 resource.action 检查）
    └── ❌ 不能访问其他 Realm 内部资源

Realm-1 Admin (realm-admin 角色，具体 resource.action 检查 + Identity::has_access_to_realm)
    └── 仅能访问 realm-1

Realm-2 Admin (realm-admin 角色，具体 resource.action 检查 + Identity::has_access_to_realm)
    └── 仅能访问 realm-2

User (user 角色)
    └── 仅拥有 points.view 权限

Third-party App / API Key (Principal Type: CLIENT / API_KEY)
    └── 权限通过角色分配和策略配置实现
```

**优点**：
- ✅ 严格的 Realm 隔离
- ✅ 精确的权限匹配（无通配符）
- ✅ 清晰的审计边界
- ✅ 每个 Realm 独立管理
