# SDK 增强 -- 资源管理产品需求文档 (PRD)

**创建时间**: 2026-05-21
**优先级**: P0

---

## 1. 相关用户故事

> 详细故事与验收标准请查看 `docs/user-stories/integration/sdk.md`。

| US-ID | 标题 | 优先级 | 来源 |
|-------|------|--------|------|
| US-TP-012 | 通过 SDK 管理 Realm | P1 | `docs/user-stories/integration/sdk.md` |
| US-TP-013 | 通过 SDK 管理用户 | P0 | `docs/user-stories/integration/sdk.md` |
| US-TP-014 | 通过 SDK 管理 Client App | P1 | `docs/user-stories/integration/sdk.md` |
| US-TP-017 | 通过 SDK 发放积分 | P0 | `docs/user-stories/integration/sdk.md` |

---

## 2. 范围界定

### 2.1 包含功能

- SDK 新增 Realm 管理方法：创建、查询列表、查询详情
- SDK 新增用户管理方法：创建、查询列表、查询详情
- SDK 新增 Client App 管理方法：创建、查询列表、查询详情
- SDK 积分发放方法：向指定用户显式发放积分，必须指定目标 Credit Bucket（`bucketId` 必填，缺失或非法返回 400 `grant_bucket_required`）；发放原因 `reason` 必填且非空（审计字段），有效期可设（缺省为永久有效）；数量为 1 ~ 1,000,000
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

## 4. 业务规则与状态

### 4.1 业务规则

- API Key 只有一种身份语义，代表第三方服务端机器凭据；API Key 自身作为 Principal 参与授权，不按 Key 类型拆分
- 使用统一 Principal + RBAC 模型，API Key 不携带 runtime/management scope，能力由 Principal 的角色和 role policy 决定
- Realm 隔离：用户和 Client App 操作仅限 API Key 所属 Realm
- Realm 创建特权：创建 Realm 需 API Key Principal 在 admin realm 具备 `realm.manage` 权限，普通 Realm 的 API Key 不可创建 Realm（RBAC 初始化仅对 admin realm 注册 `realm.manage` 权限）
- 严格的目标资源等值边界：用户、Client App 等带目标 Realm 的操作要求 Principal 所属 Realm 与目标 Realm 严格相等；唯一的平台视图例外是 Admin Realm Principal 持 `realm.view` 调用 Realm 列表时返回全平台列表，普通 Realm Principal 只返回自身 Realm；Admin Realm `realm.view` 只对 Realm 列表提供平台视图，不授权跨 Realm 修改用户或 Client App
- Principal 绑定：API Key 以自身唯一标识作为 Principal ID，复用现有角色绑定机制
- Client App 绑定的列表可见性收窄：绑定普通 Client App 的 Key 调用 ext API 的 Client App 列表（`clients.view`）时只返回其绑定的 App；未绑定或绑定内置 `admin-api-client` 的 Key 返回全量列表（详见 [API Key Roles PRD](/docs/prd/integration/api-key-roles.md) §4.1）
- 角色分配：API Key 的角色通过管理后台由 Realm Admin 分配（详见 [API Key Roles PRD](/docs/prd/integration/api-key-roles.md)），API Key 不允许绑定内置角色；分配/替换同样受授予人权限层级守卫约束（授予人须持目标角色全部权限）

**积分发放:**
- 向指定用户显式发放积分，必须指定目标 Credit Bucket（`bucketId`）：缺失或非法返回 400 `grant_bucket_required`（多钱包模型下每笔发放必须落到显式 Bucket）
- 发放原因 `reason` 必填且非空（审计字段，空白返回 400 校验错误）；有效期可设（不设置为永久有效）
- 数量校验：必须为正数且不超过 1,000,000，越界返回参数校验错误（`invalid_amount`）
- 发放需要 API Key Principal 具备 `points.manage` 权限（与 SDK 消费同一权限点）；跨 Realm 目标用户被拒绝

**ext API 输入验证:**
- Realm name：3-50 字符（代码中 `req.name.len() < 3 || req.name.len() > 50` 时返回 400 ValidationError）
- Realm admin email：非空且符合邮箱格式
- Realm admin password：8–100 字符（代码中 `req.admin_user.password.len() < 8 || > 100` 时返回 400 ValidationError）
- User email（ext API 创建用户）：非空且符合邮箱格式
- User password：8–100 字符
- Client App name：非空
- Client App redirect_uris：必填；例外——启用 `device_code_grant` 的 Client App 允许空 `redirect_uris`（device flow 无回调，校验按 `device_code_grant_enabled && redirect_uris.is_empty()` 跳过）

### 4.2 关键状态与异常

- 跨 Realm 操作被拒绝时返回权限不足错误（对所有 Realm 一致生效，admin realm 的 Principal 无豁免，见 4.1 严格的 Realm 等值边界）
- Realm 创建时需校验 API Key Principal 属于 admin realm 且具备 `realm.manage` 权限

---

## 5. 验收目标

- 4 个用户故事的全部验收场景通过
- SDK 新增方法与现有方法风格一致（方法命名、错误处理、参数模式）
- 所有新增 ext 端点遵循 Realm 隔离原则：目标用户与 Client App 操作只能作用于 API Key 所属 Realm；Realm 列表按 4.1 的平台视图例外过滤
- 所有新增资源管理端点要求 API Key Principal 具备对应权限
- Realm 创建需额外校验 API Key Principal 属于 admin realm 且具备 `realm.manage`

---

## 6. 边界与约束

**适用性**: API 边界适用；前端/交互约束不适用（本次变更仅涉及 SDK 和后端 ext API，无前端页面变更）

**API / 集成边界:**
- 所有新增端点使用现有 API Key 认证机制
- 分页参数：用户列表 `page`（1-based，默认 1）、`page_size`（默认 20，最大 100）；Realm 列表与 Client App 列表：当前无分页，返回全量数据
- 接口能力边界：
  - Realm：创建（返回 Realm ID 和基本信息）、列表、详情（需对应权限；创建还需 admin realm `realm.manage` 权限）
  - User：创建（返回用户 ID 和状态）、列表、详情（限本 Realm，无跨 Realm 例外）。权限点与管理端分属两个调用面：创建检查 `users.create`，列表/详情检查 `users.view`（见 `docs/prd/core/users.md` §4.1；`users.manage` 经 action 层级同样覆盖两者，但仅为 API Key 角色授予 `users.create` 是最小授权）
  - Client App：创建（返回 Client ID 和 Secret，client_secret 仅创建时返回）、列表（返回字段：id、client_id、name、enabled、created_at）、详情（返回字段：id、client_id、client_secret（仅创建时返回）、name、description、redirect_uris、enabled、created_at）（需对应权限，限本 Realm，无跨 Realm 例外）
  - 积分交易查询单笔（`get_transaction_ext`）：ext 端点，已注册到 OpenAPI 文档；另有按 external ref 查单笔交易的 ext 端点，能力定义归 points PRD（见 `docs/prd/billing/points.md` §4.1 查询与展示），不在此重复

**前端 / 交互边界:** 不适用——本次变更仅涉及 SDK 和后端 ext API，无前端页面变更。

---

## 7. 已确认决策

- **统一 Principal 模型**：API Key 不按类型拆分，统一作为 Principal 参与授权
- **权限由 RBAC 决定**：API Key 能力由角色和 role policy 决定，不引入 scope 机制
- **SDK 风格一致**：新增方法共享 Client 实例、统一错误类型

---

## 8. 参考资料

- 相关 PRD：`docs/prd/auth/oauth.md`（现有 ext API）
- 相关 PRD：`docs/prd/core/realm.md`（Realm 管理）
- 相关 PRD：`docs/prd/core/users.md`（用户管理）
- 相关 PRD：`docs/prd/integration/client-app.md`（Client App 管理）
- 相关 PRD：`docs/prd/integration/api-key-roles.md`（API Key 角色绑定）
- 用户故事来源见 §1 表格
