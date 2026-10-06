# 权限与角色管理产品需求文档 (PRD)

**创建时间**: 2025-01-10
**优先级**: P0

---

## 1. 相关用户故事

> 详细故事与验收标准请查看 `docs/user-stories/` 中对应文档。

| US-ID | 标题 | 优先级 | 来源 |
|-------|------|--------|------|
| US-RA-001 | Realm 隔离访问 | P0 | `docs/user-stories/core/realm-admin.md` |
| US-RA-002 | 角色定义管理 | P0 | `docs/user-stories/core/realm-admin.md` |
| US-RA-003 | 权限定义管理 | P0 | `docs/user-stories/core/realm-admin.md` |
| US-RA-004 | 为角色分配权限 | P0 | `docs/user-stories/core/realm-admin.md` |
| US-RA-005 | 查看角色权限 | P0 | `docs/user-stories/core/realm-admin.md` |
| US-RA-006 | 用户角色分配 | P0 | `docs/user-stories/core/realm-admin.md` |
| US-RA-007 | 权限策略管理 | P0 | `docs/user-stories/core/realm-admin.md` |
| US-RA-009 | 权限层级验证 | P0 | `docs/user-stories/core/realm-admin.md` |
| US-RA-010 | 查看 Dashboard 用户活跃概览 | P1 | `docs/user-stories/core/realm-admin.md` |
| US-RA-011 | 查看 Dashboard 认证趋势图 | P1 | `docs/user-stories/core/realm-admin.md` |
| US-RA-012 | 通过 Dashboard 快捷导航跳转 | P1 | `docs/user-stories/core/realm-admin.md` |
| US-BP-001 | 默认角色和权限保护 | P0 | `docs/user-stories/core/builtin-protection.md` |
| US-AU-001 | 查看 Realm 审计日志 | P0 | `docs/user-stories/core/audit.md` |
| US-AU-002 | 按条件筛选审计日志 | P0 | `docs/user-stories/core/audit.md` |
| US-AU-003 | 查看审计日志详情 | P1 | `docs/user-stories/core/audit.md` |
| US-AU-004 | 查看 Admin Realm 审计日志 | P0 | `docs/user-stories/core/audit.md` |
| US-AU-005 | 系统自动记录核心操作 | P0 | `docs/user-stories/core/audit.md` |

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
| settings.view | settings | view | 查看设置（含支付 Provider 凭证配置） |
| settings.manage | settings | manage | 设置管理（含支付 Provider 凭证配置的写入与删除，见 `/api/configs/*` 端点） |
| api_keys.view | api_keys | view | 查看 API Key 列表和详情 |
| api_keys.manage | api_keys | manage | API Key 创建、更新、删除、轮换 |
| billing.view | billing | view | 查看账单、订阅历史 |
| billing.manage | billing | manage | 账单业务管理（账单、订阅、发票等业务对象；不含支付 Provider 凭证配置——Provider 凭证由 `settings.view`/`settings.manage` 门控） |
| points.view | points | view | 查看积分、积分规则 |
| points.manage | points | manage | 积分管理、Provider 映射管理 |
| audit.view | audit | view | 查看审计日志列表和详情 |

**Admin Realm 额外权限**:

| 权限项 | 资源 | 动作 | 说明 |
|--------|------|------|------|
| realm.view | realm | view | 查看 Realm 列表（前端 Realms 菜单可见性） |
| realm.manage | realm | manage | Realm 创建（仅 admin realm） |

> **敏感权限定义的创建与改名约束**：`realm.manage` 属敏感权限——其**权限定义本身**仅可在 admin realm 创建或改名（其他 Realm 对该名称的权限定义创建/改名请求返回 403），防止租户自行造出跨租户语义的 Realm 管理权限。

**user 权限清单**:

| 权限项 | 资源 | 动作 | 说明 |
|--------|------|------|------|
| points.view | points | view | 查看自己的积分余额 |

**扩展权限（不在内置角色清单内，需显式授予）**:

| 权限项 | 资源 | 动作 | 说明 |
|--------|------|------|------|
| users.create | users | create | ext API（API Key）创建用户专用门禁；不随任何内置角色默认授予（见 `docs/prd/core/users.md` §4.1） |

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

1. 为角色添加策略/权限（`add_policy_to_role`、`assign_permission_to_role`）与为用户分配直接权限时，调用者在端点所需的管理权限（角色策略路径为 `policies.manage`；用户的角色分配为 `roles.manage`；用户直接权限分配为 `policies.manage`，见 §4.2 API 架构说明）之外，必须自身持有被授予的 `resource.action` 权限
2. 修改既有用户的角色集合必须持有 `roles.manage`；创建用户时仅附带普通 `user` 角色可由 `users.manage` 完成，其他角色仍要求角色管理权限与授予方自持检查
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

- 默认角色（`realm-admin`、`user`）和默认权限受内置保护：内置角色不能被删除、不能摘除内置权限、名称不可修改（描述 description 可修改，`US-BP-001`）；内置权限定义完全不可修改（含 description）
- 已被引用的角色/权限不可删除：仍有用户持有的角色返回 409，仍分配给角色的权限返回 409
- 权限定义的 resource/action（即 `name`）在权限仍被引用（分配给角色，或被角色策略匹配）期间不可修改（返回 409）；description 可随时修改。授权运行时按 `role_policies` 中的 resource/action 快照判定，放行 rename 会导致展示与运行时授权漂移
- 权限属于 Realm 级别，跨 Realm 访问必须拒绝
- 权限检查遵循 `resource.action` 精确匹配和层级规则，不做前端特例判断

**API 架构说明**:

权限管理 API 统一为下列端点。管理端点路径不含 `{realmId}` 段，realm 由 admin 会话 token 钉定（跨 realm 资源返回 404）。早期与 `PermissionData`（PoliceWrap/RoleWrap）格式并存的过渡期旧 API（`/api/permission/{realmId}/permissions` 及其 `/delete`、按 client_id 查询等端点）已在上线前整体移除，仅保留以下能力：

| 路径前缀 | 说明 |
|----------|------|
| `/api/permission/define` | 权限定义（permission_definitions）的 CRUD |
| `/api/roles/define` | 角色定义（role_definitions）的 CRUD 及角色权限关联 |
| `/api/permission/roles/{roleId}/policies` | 角色策略关联（GET/POST/DELETE）：查询需 `policies.view`；添加需 `policies.manage` 且授予方自持被授予的 `resource.action`；删除需 `policies.manage`（移除不受自持约束，与 §4.1 防提权规则 1 只约束添加一致） |
| `/api/permission/users/{userId}/roles` | 用户的角色分配（GET/POST/DELETE）：查询需 `users.view`；分配需 `roles.manage` 且授予方自持对应策略权限；移除需 `roles.manage` |
| `POST /api/permission/check` | 管理端批量权限检查（路径无 realm 段，realm 取自登录身份）：**任一**规则命中即 `allowed=true`。仅限自省（RFC 7662 式）：调用者须为已认证用户身份（API Key 被 403；CustomUserUi 凭证需持 `ProfileRead` scope，与 `GET /api/user/permissions` 同规则），且被探测 token 必须属于调用者本人——不可探测他人令牌；被探测 token 的主体与 ext 内省同规则复查（Client App 禁用/删除后的存活令牌回答 allowed=false，不回显 userId） |
| `POST /api/ext/permission/check` | SDK/ext 批量权限检查（API Key 认证）：**全部**规则命中才 `allowed=true`（与 admin 侧 check 的任一命中语义相反，混用易误判） |

**Principal 角色与权限管理**:

- **API Key 角色分配**: API Key 可作为 Principal 分配角色。通过 `GET/PUT /api/api-keys/{apiKeyId}/roles` 管理 API Key 的角色列表（路径无 realm 段，realm 由 admin 会话钉定；查询需要 `api_keys.view`，更新需要 `roles.manage`）。内置角色不可分配给 API Key。更新时同样受 §4.1 授予方自持约束：分配的自定义角色所含权限须为调用者完整持有（防借 API Key 提权）。
- **用户直接权限管理**: 支持绕过角色，直接为用户分配权限。通过以下端点管理（路径无 realm 段，realm 由 admin 会话钉定）：
  - `GET /api/users/{userId}/permissions` — 查询用户直接权限（需要 `users.view`）
  - `POST /api/users/{userId}/permissions` — 分配直接权限（需要 `policies.manage`）
  - `DELETE /api/users/{userId}/permissions` — 移除直接权限（需要 `policies.manage`）
  - `GET /api/users/{userId}/effective-permissions` — 查询用户有效权限（含角色继承 + 直接分配），每条权限标注来源（角色名或 "direct"）
  - 安全约束：不可创建 `All` 或通配符权限策略

---

## 5. 验收目标

- Realm Admin 可在管理端完成角色定义、权限定义、角色权限关联、用户角色分配、API Key 角色分配、用户直接权限分配/移除的完整操作
- 无权限用户访问受保护资源时被拒绝，前端隐藏无权限的菜单和操作按钮
- 权限层级规则正确生效：`manage` 隐含 `view` 和 `create`，`create` 不隐含 `view`
- 跨 Realm 访问被拒绝
- 默认角色和权限不可被删除；内置角色名称不可修改，描述可修改

---

## 6. 边界与约束

**适用性**: 适用（API 与前端/交互边界合并陈述）

**API / 集成边界:**
- 每个 API 端点检查具体的 `resource.action` 权限，不使用宽泛的 `realm.admin` 或特殊策略
- 只读操作（list、get）检查 `view` 权限；写操作（create、update、delete）检查 `manage` 权限
- 必须遵守 realm 隔离、权限边界、凭证脱敏和幂等要求

**前端 / 交互边界:**
- 管理端侧边栏菜单根据用户权限动态显示/隐藏，每个菜单项对应明确的 `resource.view` 权限
- Dashboard 快捷导航根据权限过滤，避免导向无权限页面
- 按钮级权限控制新增、编辑、删除操作；仅有 `view` 权限时管理按钮不可用
- Settings 页面：无 `settings.view` 时不可访问；有 `settings.view` 但无 `settings.manage` 时表单只读
- API Keys 页面：有 `api_keys.view` 但无 `api_keys.manage` 时能查看列表，管理按钮不可用
- 前端不做 `*` 或其他前端特例判断，权限检查结果以后端为准

**菜单权限映射**（与 `frontend/src/components/admin/sidebar.tsx` 的分组一致；billing/points 子菜单除权限外还受 feature 可用性门控，任一门不通过即不显示）:

| 菜单 | 权限 |
|-------|------|
| Dashboard | `dashboard.view` |
| Realms | `realm.view`（仅 admin realm） |
| Clients | `clients.view` |
| Users | `users.view` |
| Authorization › Permissions | `permissions.view` |
| Authorization › Roles | `roles.view` |
| Authorization › API Keys | `api_keys.view` |
| Products & Payments › Payment Providers | `billing.view` ＋ feature `billingConfigVisible` |
| Products & Payments › Entitlement Mappings | `billing.view` ＋ feature `entitlementMappingsVisible` |
| Products & Payments › Registration Rules | `points.view` ＋ feature `pointsVisible` |
| Products & Payments › Credit Buckets | `points.view` ＋ feature `pointsVisible` |
| Transactions › Invoices | `billing.view` ＋ feature `invoicesVisible` |
| Transactions › Subscription History | `billing.view` ＋ feature `subscriptionHistoryVisible` |
| Transactions › Points Wallets | `points.view` ＋ feature `pointsVisible` |
| Transactions › Statistics | `billing.view` / `points.view` 任一 |
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
| Role policy assignment | 角色策略（`/api/permission/roles/{roleId}/policies`）`policies.view`；角色权限（`/api/roles/define`）`roles.view` | 角色策略 `policies.manage`；角色权限（`/define`）`roles.manage`；均需自持被授予权限 |
| User role assignment | `users.view` | 用户角色分配（`/api/permission/users/{userId}/roles` 与用户管理服务）检查 `roles.manage`（创建用户仅附带普通 `user` 角色为受限例外） |
| API Keys | `api_keys.view` | `api_keys.manage` |
| API Key role assignment | `api_keys.view` | `roles.manage`；与用户角色分配同受授予方自持约束（分配的自定义角色所含权限须为调用者完整持有，防借 API Key 提权） |
| Products / Plans / Invoices | `billing.view` | `billing.manage` |
| Points Rules / Wallets | `points.view`（Points Rules 读取与本人积分数据；管理端跨用户 wallets/transactions 查询需 `points.manage`，与 `docs/prd/billing/points.md` §6 访问控制一致——仅持 `points.view` 的自定义角色菜单可见但跨用户数据接口 403） | `points.manage` |
| Settings（含支付 Provider 凭证配置） | `settings.view` | `settings.manage` |

---

## 7. 已确认决策

- 权限模型采用 `resource.action` 格式，不使用通配符或隐式全局权限
- `manage` 是唯一具有向下隐含能力的 action，简化权限授予策略
- 不使用 `admin` action，不引入 `realm.admin:{realm_id}` 等特殊策略
- 用户修改 profile 和 password 不走权限检查，在业务逻辑层直接处理

---

## 8. 参考资料

- 相关 PRD：`docs/prd/core/realm-settings.md`
- 相关 PRD：`docs/prd/auth/oauth.md`
- 相关 PRD：`docs/prd/core/dashboard.md`
- 相关 PRD：`docs/prd/core/audit.md`
- 用户故事来源见 §1 表格
