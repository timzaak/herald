# 权限与角色管理产品需求文档 (PRD)

**创建时间**: 2025-01-10
**优先级**: P0

---

## 1. 相关用户故事

> 详细故事与验收标准请查看 `docs/user-stories/` 中对应文档。

### 1.1 故事引用

**Realm Admin 用户故事** — `docs/user-stories/core/realm-admin.md`
- `[US-RA-001]` Realm 隔离访问 (P0): 作为 Realm Admin，我只能访问自己 Realm 的资源
- `[US-RA-002]` 角色定义管理 (P0): 作为 Realm Admin，我想要管理角色定义
- `[US-RA-003]` 权限定义管理 (P0): 作为 Realm Admin，我想要管理权限定义
- `[US-RA-004]` 为角色分配权限 (P0): 作为 Realm Admin，我想要为角色分配权限
- `[US-RA-005]` 查看角色权限 (P0): 作为 Realm Admin，我想要查看角色的权限
- `[US-RA-006]` 用户角色分配 (P0): 作为 Realm Admin，我想要为用户分配角色
- `[US-RA-007]` 权限策略管理 (P0): 作为 Realm Admin，我想要管理权限策略
- `[US-RA-009]` 权限层级验证 (P0): 作为 Realm Admin，系统应自动应用权限层级规则
- `[US-RA-010]` 查看 Dashboard 用户活跃概览 (P1)
- `[US-RA-011]` 查看 Dashboard 认证趋势图 (P1)
- `[US-RA-012]` 通过 Dashboard 快捷导航跳转 (P1)

**内置保护用户故事** — `docs/user-stories/core/builtin-protection.md`
- `[US-BP-001]` 默认角色和权限保护 (P0): 默认的角色和权限不能被删除，内置角色名称不可修改

**审计日志用户故事** — `docs/user-stories/core/audit.md`
- `[US-AU-001]` 查看 Realm 审计日志 (P0)
- `[US-AU-002]` 按条件筛选审计日志 (P0)
- `[US-AU-003]` 查看审计日志详情 (P1)
- `[US-AU-004]` 查看 Admin Realm 审计日志 (P0)
- `[US-AU-005]` 系统自动记录核心操作 (P0)

### 1.2 优先级汇总

| 优先级 | 数量 | 关键故事 |
|--------|------|----------|
| P0 | 13 | Realm 隔离访问、角色定义管理、权限定义管理、为角色分配权限、查看角色权限、用户角色分配、权限策略管理、权限层级验证、默认角色和权限不可删除/内置角色名称不可修改、审计日志查看/筛选/Admin Realm/自动记录 |
| P1 | 4 | Dashboard 活跃概览、认证趋势图、快捷导航、审计详情 |
| P2 | 0 | - |

---

## 2. 范围界定

### 2.1 包含功能

- RBAC 元数据层管理（角色定义、权限定义、角色权限关联）
- 自研权限运行时层（用户角色分配、资源访问策略）
- 两层架构（PostgreSQL + Redis 缓存）
- 前端角色管理页面
- 前端权限管理页面
- 权限检查集成（Service 层集成）
- 默认角色（`realm-admin`、`user`）
- 默认权限定义（见下方权限清单）
- 菜单级和按钮级前端权限控制（对齐后端权限）

### 2.2 不包含功能 (Out of Scope)

- 权限策略可视化 — 前端没有权限策略可视化工具
- 权限冲突检测 — 没有自动检测权限冲突的功能
- 通配符或全局隐式权限 — 所有权限必须精确匹配，不引入 `*` 或 `admin` 动作
- 历史数据迁移 — 项目尚未上线

### 2.3 依赖项

- 用户认证系统 — 提供登录和会话管理
- Realm 系统 — 权限属于 Realm 级别
- Client App 系统 — 权限与 Client App 关联
- Redis 缓存 — 提升权限检查性能（P95 < 50ms）

---

## 3. 需求概述

### 3.1 功能描述

Herald 系统实现完整的 RBAC（基于角色的访问控制）权限管理体系，采用两层权限控制：

1. **RBAC 元数据层** — 定义角色、权限及其关联关系，用于管理操作（如创建角色定义）
2. **自研权限运行时层** — 实际权限检查和用户角色分配（Redis 缓存 + PostgreSQL），用于运行时权限检查（如用户是否有权限访问某个资源）

### 3.2 关键特性

- `resource.action` 格式的细粒度权限模型
- `manage` / `create` / `view` 三级 action 层级，`manage` 向下隐含 `view` 和 `create`
- Realm 级别权限隔离，跨 Realm 访问拒绝
- 内置 `realm-admin` 和 `user` 默认角色，不可删除，名称不可修改，描述可修改
- 菜单级和按钮级前端权限控制，对齐后端权限模型

---

## 4. 业务规则与状态

### 4.1 业务规则

**权限格式规则**:
- 权限格式为 `resource.action`，`resource` 必须精确匹配（不支持通配符）
- `manage` 是唯一具有向下隐含能力的 action，覆盖同一 resource 下的 `view`、`create` 和 `manage`
- `create` 仅匹配自身，不隐含 `view`
- `view` 仅匹配自身
- 所有层级规则仅在**同一 resource 内**生效
- 不使用 `admin` action，不引入特殊 `resource:action` 组合（如 `realm.admin:{realm_id}`）
- 不引入隐式全局权限

**Principal Types**:

| Principal Type | 标识 | 说明 |
|---------------|------|------|
| User | `user` | 已登录用户 |
| API Key | `api_key` | API Key 凭证 |
| Client | `client` | OAuth 客户端应用 |

**内置角色**:

| 角色 | 技术标识 | 说明 |
|------|----------|------|
| Realm Admin | `realm-admin` | Realm 管理员，拥有该 Realm 的完整管理权限 |
| User | `user` | 普通用户，仅拥有基本权限 |

**realm-admin 权限清单（所有 Realm）**:

| 权限项 | 资源 | 动作 | 说明 |
|--------|------|------|------|
| dashboard.view | dashboard | view | 查看 Dashboard 统计 |
| users.view | users | view | 查看用户 |
| users.manage | users | manage | 用户管理 |
| clients.view | clients | view | 查看客户端应用 |
| clients.manage | clients | manage | 客户端应用管理 |
| roles.view | roles | view | 查看角色 |
| roles.manage | roles | manage | 角色管理 |
| permissions.view | permissions | view | 查看权限 |
| permissions.manage | permissions | manage | 权限管理 |
| policies.view | policies | view | 查看策略 |
| policies.manage | policies | manage | 策略管理 |
| settings.view | settings | view | 查看设置 |
| settings.manage | settings | manage | 设置管理 |
| api_keys.view | api_keys | view | 查看 API Key 列表和详情 |
| api_keys.manage | api_keys | manage | API Key 创建、更新、删除、轮换 |
| billing.view | billing | view | 查看账单、订阅历史、支付配置 |
| billing.manage | billing | manage | 账单管理、支付 Provider 配置管理 |
| points.view | points | view | 查看积分、积分规则 |
| points.manage | points | manage | 积分管理、Provider 映射管理 |
| audit.view | audit | view | 查看审计日志列表和详情 |

**Admin Realm 额外权限**:

| 权限项 | 资源 | 动作 | 说明 |
|--------|------|------|------|
| realm.view | realm | view | 查看 Realm 列表（前端 Realms 菜单可见性） |
| realm.manage | realm | manage | Realm 创建（仅 admin realm） |

**user 权限清单**:

| 权限项 | 资源 | 动作 | 说明 |
|--------|------|------|------|
| points.view | points | view | 查看自己的积分余额 |

> 用户修改自己的 profile 和 password 在业务逻辑层处理，不需要权限检查。

**权限层级规则**:

| 已授予的 action | 可通过的请求 action | 说明 |
|---|---|---|
| `manage` | `view`、`create`、`manage` | 唯一的层级 action，向下覆盖 |
| `create` | `create` | 仅自身 |
| `view` | `view` | 仅自身 |

1. `manage` 是唯一具有向下隐含能力的 action。授予某资源 `manage` 后，无需再单独授予该资源的 `view` 或 `create`。
2. `create` 不隐含 `view`。如需同时创建和查看，必须分别授予 `create` 和 `view`，或直接授予 `manage`。
3. 所有层级规则仅在**同一 resource 内**生效。`users.manage` 不会授予 `clients.view`。
4. 不使用 `admin` action，不引入特殊 `resource:action` 组合。

**已废弃权限**（不再初始化和使用）:

| 权限项 | 原用途 | 替代方案 |
|--------|--------|---------|
| `realm.admin` | 宽泛的管理端权限 | 各模块具体的 `resource.view` / `resource.manage` |
| `realm.create` | Realm 创建 | `realm.manage`（仅限创建 Realm，不含编辑其他 Realm 元数据；Realm 删除当前不支持） |
| `realm.admin:{realm_id}` 特殊策略 | 判断是否能进入管理端 | 具体权限检查 + `Identity::has_access_to_realm` |

**防提权约束（授予方权限自持）**:

1. 为角色添加策略/权限（`add_policy_to_role`、`assign_permission_to_role`）与为用户分配直接权限时，调用者在 `policies.manage`/`roles.manage` 之外，必须自身持有被授予的 `resource.action` 权限
2. 为用户指派 "user" 以外的内置角色时，调用者必须持有该角色授予的全部权限（普通 "user" 内置角色豁免，作为默认终端用户角色）
3. 该规则防止仅持部分管理权限的 delegated-admin 通过授权操作自我提权（例如把高权限内置角色或自身不具备的权限授予自己）；权限不满足时返回权限不足错误

**Realm 操作权限**:

| 操作 | 权限 |
|------|------|
| List realms | `realm.view` in admin realm（Super Admin only） |
| View own realm detail | 无需权限（登录即可查看） |
| View other realm detail | `realm.manage` in admin realm |
| Create realm | `realm.manage` in admin realm |
| Update realm metadata | `settings.manage` for own realm only（cross-realm editing not allowed） |

### 4.2 关键状态与异常

- 默认角色（`realm-admin`、`user`）和默认权限受内置保护，不能被删除；内置角色的名称不可修改，描述(description)可修改（`US-BP-001`）
- 权限属于 Realm 级别，跨 Realm 访问必须拒绝
- 权限检查遵循 `resource.action` 精确匹配和层级规则，不做前端特例判断

**API 架构说明**:

当前系统中存在两套权限相关 API 共存：

| API 风格 | 路径前缀 | 说明 |
|----------|---------|------|
| 旧 API | `/api/permission/{realmId}/permissions` | 使用 `PermissionData` 格式，按 client 维度管理权限分配 |
| 新 API | `/api/permission/{realmId}/define` | 权限定义（permission_definitions）的 CRUD |
| 新 API | `/api/roles/{realmId}/define` | 角色定义（role_definitions）的 CRUD 及角色权限关联 |

旧 API 为过渡期保留，新 API 为主要演进方向。前端应优先使用新 API。

**Principal 角色与权限管理**:

- **API Key 角色分配**: API Key 可作为 Principal 分配角色。通过 `GET/PUT /api/api-keys/{realmId}/{apiKeyId}/roles` 管理 API Key 的角色列表（查询需要 `api_keys.view`，更新需要 `roles.manage`）。内置角色不可分配给 API Key。
- **用户直接权限管理**: 支持绕过角色，直接为用户分配权限。通过以下端点管理：
  - `GET /api/users/{realmId}/{userId}/permissions` — 查询用户直接权限（需要 `users.view`）
  - `POST /api/users/{realmId}/{userId}/permissions` — 分配直接权限（需要 `policies.manage`）
  - `DELETE /api/users/{realmId}/{userId}/permissions` — 移除直接权限（需要 `policies.manage`）
  - `GET /api/users/{realmId}/{userId}/effective-permissions` — 查询用户有效权限（含角色继承 + 直接分配），每条权限标注来源（角色名或 "direct"）
  - 安全约束：不可创建 `All` 或通配符权限策略

---

## 5. 功能需求

### 5.1 核心需求

- RBAC 元数据管理：支持角色定义的创建、查询、更新、删除；权限定义的创建、查询、更新、删除；角色权限关联的管理
- 权限运行时：支持用户角色分配、API Key 角色分配、用户直接权限分配/移除、资源访问策略管理
- 权限检查：Service 层集成 `resource.action` 权限检查，`manage` 隐含 `view` 和 `create`
- 前端权限控制：侧边栏菜单根据 `resource.view` 权限动态显示/隐藏；按钮级权限控制新增、编辑、删除操作
- 默认角色与权限：系统提供 `realm-admin` 和 `user` 内置角色及对应权限，受内置保护（不可删除，内置角色名称不可修改）

### 5.2 验收目标

- Realm Admin 可在管理端完成角色定义、权限定义、角色权限关联、用户角色分配、API Key 角色分配、用户直接权限分配/移除的完整操作
- 无权限用户访问受保护资源时被拒绝，前端隐藏无权限的菜单和操作按钮
- 权限层级规则正确生效：`manage` 隐含 `view` 和 `create`，`create` 不隐含 `view`
- 跨 Realm 访问被拒绝
- 默认角色和权限不可被删除；内置角色名称不可修改，描述可修改

---

## 6. API 相关约束

**适用性**: 适用

- 每个 API 端点检查具体的 `resource.action` 权限，不使用宽泛的 `realm.admin` 或特殊策略
- Realm 隔离：权限属于 Realm 级别，跨 Realm 访问必须拒绝
- 权限层级遵循 4.1 节规则，`manage` 隐含 `view` 和 `create`
- 只读操作（list、get）检查 `view` 权限；写操作（create、update、delete）检查 `manage` 权限
- Realm 创建在 admin realm 内检查 `realm.manage`
- 必须遵守 realm 隔离、权限边界、凭证脱敏和幂等要求

---

## 7. 前端/交互约束

**适用性**: 适用

- 管理端侧边栏菜单根据用户权限动态显示/隐藏，每个菜单项对应明确的 `resource.view` 权限
- Dashboard 快捷导航根据权限过滤，避免导向无权限页面
- 按钮级权限控制新增、编辑、删除操作；仅有 `view` 权限时管理按钮不可用
- Settings 页面：无 `settings.view` 时不可访问；有 `settings.view` 但无 `settings.manage` 时表单只读
- API Keys 页面：有 `api_keys.view` 但无 `api_keys.manage` 时能查看列表，管理按钮不可用
- 前端不做 `*` 或其他前端特例判断，权限检查结果以后端为准

**菜单权限映射**:

| 菜单 | 权限 |
|-------|------|
| Dashboard | `dashboard.view` |
| Realms | `realm.view`（仅 admin realm） |
| Clients | `clients.view` |
| Users | `users.view` |
| Permissions | `permissions.view` |
| Roles | `roles.view` |
| API Keys | `api_keys.view` |
| Products | `billing.view` |
| Payment Providers | `billing.view` |
| Subscription Plans | `billing.view` |
| Points Rules | `points.view` |
| Invoices | `billing.view` |
| Subscription History | `billing.view` |
| Points Wallets | `points.view` |
| Audit Log | `audit.view` |
| Settings | `settings.view` |

**按钮级权限**:

| 页面 | 查看 | 新增/编辑/删除 |
|------|------|---------------|
| Realms | `realm.view` | `realm.manage` |
| Clients | `clients.view` | `clients.manage` |
| Users | `users.view` | `users.manage` |
| Permissions | `permissions.view` | `permissions.manage` |
| Roles | `roles.view` | `roles.manage` |
| Role policy assignment | `roles.view` | 角色策略（旧 API）`policies.manage`；角色权限（新 API `/define`）`roles.manage`；均需自持被授予权限 |
| User role assignment | `users.view` | `roles.manage`（指派 "user" 以外内置角色需自持其全部权限） |
| API Keys | `api_keys.view` | `api_keys.manage` |
| API Key role assignment | `api_keys.view` | `roles.manage` |
| Products / Plans / Invoices / Providers | `billing.view` | `billing.manage` |
| Points Rules / Wallets | `points.view` | `points.manage` |
| Settings | `settings.view` | `settings.manage` |

---

## 8. 已确认决策

### 8.1 已确认决策

- 权限模型采用 `resource.action` 格式，不使用通配符或隐式全局权限
- `manage` 是唯一具有向下隐含能力的 action，简化权限授予策略
- `create` 不隐含 `view`，需要查看和创建的必须分别授予或直接授予 `manage`
- 所有层级规则仅在同一 resource 内生效，避免跨资源隐含
- 不使用 `admin` action，不引入 `realm.admin:{realm_id}` 等特殊策略
- 用户修改 profile 和 password 不走权限检查，在业务逻辑层直接处理

---

## 9. 参考资料

- 用户故事：`docs/user-stories/core/realm-admin.md`
- 用户故事：`docs/user-stories/core/builtin-protection.md`
- 用户故事：`docs/user-stories/core/audit.md`
- 相关 PRD：`docs/prd/core/realm-settings.md`
- 相关 PRD：`docs/prd/auth/oauth.md`
- 相关 PRD：`docs/prd/core/dashboard.md`
- 相关 PRD：`docs/prd/core/audit.md`
