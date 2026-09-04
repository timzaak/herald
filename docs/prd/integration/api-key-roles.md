# API Key 角色绑定产品需求文档 (PRD)

**创建时间**: 2026-05-23
**优先级**: P0

---

## 1. 相关用户故事

> 详细故事与验收标准请查看 `docs/user-stories/` 中对应文档。

### 1.1 相关故事

- **[US-RA-016]** API Key 角色管理，优先级 P0，来源 `docs/user-stories/core/realm-admin.md`
  - 角色：Realm Admin
  - 摘要：在管理后台查看和分配 API Key 角色，控制 API Key 通过 ext API 可执行的操作范围

- **[US-RA-017]** 创建 API Key 时绑定角色，优先级 P0，来源 `docs/user-stories/core/realm-admin.md`
  - 角色：Realm Admin
  - 摘要：创建 API Key 时可选地分配角色，使 Key 创建后即可使用

### 1.2 已有关联故事

- **[US-RA-006]** 用户角色分配（P0）-- API Key 角色分配复用同一权限模型和交互模式
- **[US-TP-012]** 通过 SDK 管理 Realm（P1）-- API Key 需具备 `realm.manage` 权限
- **[US-TP-013]** 通过 SDK 管理用户（P0）-- API Key 需具备 `users.manage`/`users.view` 权限
- **[US-TP-014]** 通过 SDK 管理 Client App（P1）-- API Key 需具备 `clients.manage`/`clients.view` 权限

### 1.3 优先级汇总

| 优先级 | 数量 | 关键故事 |
|--------|------|----------|
| P0 | 2 | API Key 角色管理、创建 API Key 时绑定角色 |
| P1 | 3 | 通过 SDK 管理 Realm/用户/Client App（依赖本功能提供的角色分配能力） |

---

## 2. 范围界定

### 2.1 包含功能

- API Key 列表页显示角色 badge（每行显示已分配角色，超过 2 个折叠为「+N more」）
- API Key 列表页增加「Roles」操作按钮（打开角色管理对话框）
- API Key 角色管理对话框（查看、分配、清除角色，即时保存）
- 创建 API Key 表单增加可选角色选择器（创建成功后自动绑定）
- 创建 API Key 表单增加 Client App 选择器；未选择时默认绑定内置 `admin-api-client`
- API Key 列表展示绑定的 Client App 名称
- 每个 Realm 拥有一个内置 API Key Client App（`client_id = 'admin-api-client'`，默认 `enabled=true`）
- API Key 认证路径受关联 Client App 的 `enabled` 状态影响（Client App 禁用时其下所有 API Key 均不可用）
- ext API 对非 `admin-api-client` 的 API Key 执行 Client App 作用域隔离
- API Key 更新（名称、启用状态、过期时间）、删除、密钥轮换（轮换生成新密钥，旧密钥立即失效，新明文仅返回一次）

### 2.2 不包含功能 (Out of Scope)

- API Key 粒度的 scope/permission 直接绑定（走 RBAC 角色中转）
- SDK 变更
- 新的数据存储结构

### 2.3 依赖项

- 角色选择器组件 -- 前端 `RoleSelector` 组件可复用

---

## 3. 需求概述

### 3.1 功能描述

当前 API Key 创建后没有 UI 方式为其分配角色和权限。API Key 的 RBAC 权限绑定只能通过直接操作数据库完成，导致第三方集成方无法自助完成 API Key 的权限配置。

本方案为 API Key 管理页面增加角色分配能力，使 Realm Admin 能在管理后台中为 API Key 分配角色，从而控制 API Key 通过 ext API 可执行的操作范围。

### 3.2 关键特性

- **复用 RBAC 基础设施**：API Key 角色绑定视为与用户角色绑定平行的操作，共享权限模型
- **与用户角色分配交互一致**：复用 RoleSelector 组件，即时保存模式
- **内置角色保护**：API Key 不允许绑定内置角色（`is_builtin=true`），仅可绑定自定义角色
- **Client App 全局开关**：内置 API Key Client App 的 `enabled` 状态作为其下所有 API Key 的全局开关
- **Client App 作用域隔离**：绑定非内置 Client App 的 API Key 只能访问该 Client App 的资源
- **创建时可选角色**：创建 API Key 时可选角色，角色绑定失败不回滚 Key 创建

---

## 4. 业务规则与状态

### 4.1 业务规则

- API Key 角色管理需 `roles.manage` 权限（与用户角色分配一致）
- API Key 角色查看需 `api_keys.view` 权限
- API Key 不允许绑定内置角色（`is_builtin=true`），仅可绑定自定义角色
- API Key 必须绑定到本 Realm 下存在的 Client App；未显式选择时绑定内置 `admin-api-client`
- 绑定 `admin-api-client` 的 API Key 保持 Realm 级 ext API 访问范围
- 绑定普通 Client App 的 API Key 只能访问该 Client App 的订阅、套餐分配、积分消费和权限检查上下文
- 禁用单个 API Key 只更新该 Key 自身的 `enabled` 状态，不影响内置 Client App
- 角色替换采用 last-write-wins 语义（与用户角色分配一致）
- 并发策略不使用乐观锁，前端通过 invalidate 缓存保证 UI 最终一致

### 4.2 关键状态与异常

- 角色绑定失败时不回滚 API Key 创建，仍展示明文 Key，toast 提示用户稍后手动管理角色
- Client App 被禁用时，其下所有 API Key 认证均不可用
- 普通 Client App API Key 访问其他 Client App 资源时返回权限不足
- 无 `roles.manage` 权限时「Roles」按钮隐藏，角色 badge 仍可见（由 `api_keys.view` 控制）

---

## 5. 功能需求

### 5.1 核心需求

1. **API Key 角色查看与分配** -- US-RA-016
   - API Key 列表每行显示已分配角色 badge
   - 点击「Roles」按钮打开角色管理对话框
   - 角色变更即时保存，无需确认按钮
   - 角色变更后权限缓存立即失效

2. **创建 API Key 时绑定角色** -- US-RA-017
   - 创建表单增加可选角色选择区
   - 创建成功后自动绑定所选角色
   - 角色绑定失败不回滚 Key 创建，仍展示明文 Key

   > 创建 API Key 时支持通过 `role_ids` 字段绑定角色，角色绑定失败不回滚 Key 创建。

3. **内置 API Key Client App**
   - 每个 Realm 拥有一个内置 API Key Client App（`client_id = 'admin-api-client'`）
   - 默认 `enabled=true`
   - API Key 创建时未选择 Client App 则复用该 Client App
   - Client App 被禁用时，其下所有 API Key 均不可用

4. **API Key Client App 隔离** -- US-RA-018
   - 创建 API Key 时可选择本 Realm 下的 Client App
   - 列表和详情展示绑定的 Client App
   - 绑定普通 Client App 的 API Key 只能访问该 Client App 的 ext API 资源
   - 绑定 `admin-api-client` 的 API Key 可访问本 Realm 内全部 ext API 资源

### 5.2 验收目标

- US-RA-016 全部验收场景通过
- US-RA-017 全部验收场景通过
- API Key 角色变更后 ext API 权限立即生效
- API Key 绑定普通 Client App 后，不能访问其他 Client App 的订阅、套餐分配、积分消费和权限检查上下文
- 现有用户角色分配功能不受影响
- 现有 API Key 列表的分页和筛选行为不受影响

---

## 6. API 相关约束

**适用性**: 适用

- API Key 角色查询和替换使用管理后台的 FirstParty 浏览器 token 凭证（`admin-web-console`），非 API Key 认证
- 读取操作检查 `api_keys.view` 权限，写操作检查 `roles.manage` 权限
- Realm 隔离：只能操作当前 Realm 下的 API Key
- Client App 隔离：创建 API Key 时提供的 Client App 必须属于当前 Realm；ext API 对普通 Client App API Key 强制目标 Client App 等于绑定 Client App
- 角色替换采用 last-write-wins 语义（与用户角色分配一致）
- 并发策略不使用乐观锁，前端通过 invalidate 缓存保证 UI 最终一致

---

## 7. 前端/交互约束

**适用性**: 适用

- API Key 列表页每行新增角色 badge 列和「Roles」操作按钮
- API Key 列表页显示 Client App 列；创建页在名称后显示 Client App 选择器
- 角色对话框复用 RoleSelector 组件，交互模式与用户角色分配一致（即时保存，无确认按钮）
- 创建 API Key 表单中角色选择区仅对具备 `roles.manage` 权限的用户显示
- 角色绑定失败时 toast 提示，不阻断明文 Key 展示
- 无 `roles.manage` 权限时「Roles」按钮隐藏，角色 badge 仍可见（由 `api_keys.view` 控制）

---

## 8. 已确认决策

### 8.1 已确认决策

- **Repo 扩展方式**：新增独立的 `replace_api_key_roles` / `get_api_key_roles` 方法，不修改现有 `replace_user_roles` 签名
- **内置角色判断**：按角色 `is_builtin` 字段判断，不按角色名称判断
- **内置 Client App 默认状态**：`enabled=true`
- **创建时绑定失败策略**：不回滚 API Key 创建，保留明文 Key 展示，toast 提示用户稍后手动管理角色
- **创建表单权限门控**：无 `roles.manage` 权限时不显示角色选择区，创建 API Key 仍由 `api_keys.manage` 控制
- **Client App 默认值**：创建 API Key 未选择 Client App 时默认绑定 `admin-api-client`
- **超级范围判断**：仅 `client_id = 'admin-api-client'` 的绑定视为 Realm 级 API Key；其他 Client App 均按自身作用域隔离

---

## 9. 参考资料

- 用户故事：`docs/user-stories/core/realm-admin.md`（US-RA-016、US-RA-017）
- 用户故事：`docs/user-stories/integration/sdk.md`（US-TP-012 ~ US-TP-014）
- 相关 PRD：`docs/prd/auth/permissions.md`（权限管理）
- 相关 PRD：`docs/prd/integration/sdk.md`（SDK 资源管理）
- 相关 PRD：`docs/prd/integration/client-app.md`（Client App 管理）
