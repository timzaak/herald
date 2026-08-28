# SDK 增强 -- 资源管理产品需求文档 (PRD)

**创建时间**: 2026-05-21
**优先级**: P0

---

## 1. 相关用户故事

> 详细故事与验收标准请查看 `docs/user-stories/integration/sdk.md`。

### 1.1 相关故事

- **[US-TP-012]** 通过 SDK 管理 Realm，优先级 P1，来源 `docs/user-stories/integration/sdk.md`
  - 角色：Third-Party App
  - 摘要：编程式创建、查询列表、查询详情 Realm

- **[US-TP-013]** 通过 SDK 管理用户，优先级 P0，来源 `docs/user-stories/integration/sdk.md`
  - 角色：Third-Party App
  - 摘要：在指定 Realm 中创建、查询列表、查询详情用户

- **[US-TP-014]** 通过 SDK 管理 Client App，优先级 P1，来源 `docs/user-stories/integration/sdk.md`
  - 角色：Third-Party App
  - 摘要：编程式创建、查询列表、查询详情 Client App

### 1.2 优先级汇总

| 优先级 | 数量 | 关键故事 |
|--------|------|----------|
| P0 | 1 | 通过 SDK 管理用户 |
| P1 | 2 | 通过 SDK 管理 Realm、通过 SDK 管理 Client App |

---

## 2. 范围界定

### 2.1 包含功能

- SDK 新增 Realm 管理方法：创建、查询列表、查询详情
- SDK 新增用户管理方法：创建、查询列表、查询详情
- SDK 新增 Client App 管理方法：创建、查询列表、查询详情
- 后端 api-ext 模块新增对应的外部 API 端点
- SDK 方法保持与现有风格一致：基于 reqwest、使用 API Key 认证、统一的错误处理
- 新增资源管理端点要求 API Key Principal 具备对应 RBAC 权限

### 2.2 不包含功能 (Out of Scope)

- 权限管理 SDK 方法（角色 CRUD、权限定义、策略管理等）-- 保持现有 `check_permission` 不变
- 用户编辑、删除操作
- Client App 编辑、删除、设置管理操作
- Realm 编辑、删除、设置操作
- 前端页面变更
- SDK 缓存策略变更

### 2.3 依赖项

- 现有 api-ext 模块的认证机制（API Key）
- 现有 domain 层的 Realm、User、Client App 领域服务
- 现有 SDK 的 Client 结构体和错误处理模式

---

## 3. 需求概述

### 3.1 功能描述

当前 Rust SDK 仅覆盖权限检查、订阅管理和积分系统三类能力。第三方应用开发者若需要通过编程方式管理 Realm、用户和 Client App 等核心资源，只能自行调用内部 API 或登录管理后台手动操作。

本次增强为 SDK 补齐核心资源的管理能力，使第三方应用能够通过 SDK 自动完成用户开通、应用注册和组织（Realm）初始化，降低集成门槛。

### 3.2 关键特性

- **Realm 管理**：创建、查询列表、查询详情
- **用户管理**：创建、查询列表、查询详情（P0）
- **Client App 管理**：创建、查询列表、查询详情
- **与现有 SDK 风格一致**：共享 Client 实例、统一错误类型、API Key 认证
- **统一 Principal 权限语义**：API Key 代表第三方服务端机器凭据；API Key 自身作为 Principal 参与授权，能力由角色/权限决定，资源边界由 Realm 隔离决定

---

## 4. 业务规则与状态

### 4.1 业务规则

- API Key 只有一种身份语义，代表第三方服务端机器凭据；API Key 自身作为 Principal 参与授权，不按 Key 类型拆分
- 使用统一 Principal + RBAC 模型，API Key 不携带 runtime/management scope，能力由 Principal 的角色和 role policy 决定
- Realm 隔离：用户和 Client App 操作仅限 API Key 所属 Realm
- Realm 创建特权：创建 Realm 需 API Key Principal 在 admin realm 具备 `realm:manage` 权限，普通 Realm 的 API Key 不可创建 Realm（RBAC 初始化仅对 admin realm 注册 `realm:manage` 权限）
- 严格的 Realm 等值边界：`require_realm_membership` 要求 Principal 所属 Realm 与目标 Realm 严格相等；admin realm 的 Principal（包括 API Key）不享有任何跨 Realm 放行，任何 Principal 只能操作自身所属 Realm 的用户、Client App 等资源（admin realm 的特殊之处仅在于持有 `realm.view`/`realm.manage` 等 Admin Realm 专属权限）
- Principal 绑定：API Key 以自身唯一标识作为 Principal ID，复用现有角色绑定机制
- 角色分配：API Key 的角色通过管理后台由 Realm Admin 分配（详见 [API Key Roles PRD](/docs/prd/integration/api-key-roles.md)），API Key 不允许绑定内置角色

### 4.2 关键状态与异常

- 跨 Realm 操作被拒绝时返回权限不足错误（对所有 Realm 一致生效，admin realm 的 Principal 无豁免，见 4.1 严格的 Realm 等值边界）
- Realm 创建时需校验 API Key Principal 属于 admin realm 且具备 `realm:manage` 权限

---

## 5. 功能需求

### 5.1 核心需求

1. **Realm 管理** -- US-TP-012
   - 创建新 Realm，返回 Realm ID 和基本信息
   - 查询可见 Realm 列表
   - 查询指定 Realm 详情

2. **用户管理** -- US-TP-013（P0）
   - 在指定 Realm 中创建用户，返回用户 ID 和状态
   - 查询指定 Realm 的用户列表（分页：page 默认 1，page_size 默认 20，最大 100）
   - 查询指定 Realm 中单个用户的详情

3. **Client App 管理** -- US-TP-014
   - 在指定 Realm 中创建 Client App，返回 Client ID 和 Secret
   - 查询指定 Realm 的 Client App 列表（返回字段：id、client_id、name、enabled、created_at）
   - 查询指定 Realm 中单个 Client App 的详情（返回字段：id、client_id、client_secret（仅创建时返回）、name、description、redirect_uris、enabled、created_at）

### 5.2 验收目标

- 3 个用户故事的全部验收场景通过
- SDK 新增方法与现有方法风格一致（方法命名、错误处理、参数模式）
- 所有新增 ext 端点遵循 Realm 隔离原则：API Key 只能操作所属 Realm 的资源（对所有 Realm 一致，admin realm 的 API Key 无跨租户例外，见 4.1）
- 所有新增资源管理端点要求 API Key Principal 具备对应权限
- Realm 创建需额外校验 API Key Principal 属于 admin realm 且具备 `realm:manage`

---

## 6. API 相关约束

**适用性**: 适用

### 访问控制原则

- 所有新增端点使用现有 API Key 认证机制
- API Key 语义：API Key 只有一种身份语义，代表第三方服务端机器凭据；API Key 自身作为 Principal 参与授权，不按 Key 类型拆分
- 权限模型：使用统一 Principal + RBAC 模型，能力由 Principal 的角色和 role policy 决定
- Realm 隔离：用户和 Client App 操作仅限 API Key 所属 Realm
- Realm 创建特权：创建 Realm 需 API Key Principal 在 admin realm 具备 `realm:manage` 权限
- Realm 等值边界：`require_realm_membership` 对所有 Principal（含 admin realm 的 Principal 与 API Key）一律要求所属 Realm 与目标 Realm 严格相等，不存在跨 Realm 超级管理员
- Principal 绑定：API Key 以自身唯一标识作为 Principal ID，复用现有角色绑定机制
- 角色分配：API Key 的角色通过管理后台由 Realm Admin 分配（详见 [API Key Roles PRD](/docs/prd/integration/api-key-roles.md)），API Key 不允许绑定内置角色

### 输入验证规则

- Realm name：3-50 字符（代码中 `req.name.len() < 3 || req.name.len() > 50` 时返回 400 ValidationError）
- Realm admin email：非空且符合邮箱格式
- Realm admin password：最少 8 字符（代码中 `req.admin_user.password.len() < 8`）
- User email（ext API 创建用户）：非空且符合邮箱格式
- User password：最少 8 字符
- Client App name：非空
- Client App redirect_uris：必填（传入 `CreateClientAppRequest`，如启用 device_code_grant 且 redirect_uris 为空会触发业务校验失败）

### 分页参数

- 用户列表：`page`（1-based，默认 1）、`page_size`（默认 20，最大 100）
- Realm 列表与 Client App 列表：当前无分页，返回全量数据

### 接口能力边界

- Realm：创建、列表、详情（需对应权限；创建还需 admin realm `realm:manage` 权限）
- User：创建、列表、详情（需对应权限，限本 Realm，无跨 Realm 例外）
- Client App：创建、列表、详情（需对应权限，限本 Realm，无跨 Realm 例外）
- 积分交易查询单笔（`get_transaction_ext`）：端点挂载于 `/api/ext/points/{realmId}/transactions/{transactionId}`，并已注册到 OpenAPI 文档

---

## 7. 前端/交互约束

**适用性**: 不适用

本次变更仅涉及 SDK 和后端 ext API，无前端页面变更。

---

## 8. 已确认决策

### 8.1 已确认决策

- **统一 Principal 模型**：API Key 不按类型拆分，统一作为 Principal 参与授权
- **权限由 RBAC 决定**：API Key 能力由角色和 role policy 决定，不引入 scope 机制
- **SDK 风格一致**：新增方法共享 Client 实例、统一错误类型

---

## 9. 参考资料

- 用户故事：`docs/user-stories/integration/sdk.md`
- 相关 PRD：`docs/prd/auth/oauth.md`（现有 ext API）
- 相关 PRD：`docs/prd/core/realm.md`（Realm 管理）
- 相关 PRD：`docs/prd/core/users.md`（用户管理）
- 相关 PRD：`docs/prd/integration/client-app.md`（Client App 管理）
- 相关 PRD：`docs/prd/integration/api-key-roles.md`（API Key 角色绑定）
