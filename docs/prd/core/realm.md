# Realm 管理产品需求文档 (PRD)

**创建时间**: 2025-01-01
**优先级**: P0

---

## 1. 相关用户故事

> 详细故事与验收标准请查看 `docs/user-stories/` 中对应文档。

### 1.1 故事引用

- `[US-AR-001]` 创建 Realm，优先级 P0，来源 `docs/user-stories/core/admin-realm.md`
  - 角色：Admin Realm 管理员
  - 摘要：创建新的 Realm，为不同组织提供独立的认证服务

- `[US-AR-002]` 查看 Realm 列表，优先级 P0，来源 `docs/user-stories/core/admin-realm.md`
  - 角色：Admin Realm 管理员
  - 摘要：查看所有 Realm，管理系统中的组织

- `[US-AR-003]` 查看 Realm 详情，优先级 P1，来源 `docs/user-stories/core/admin-realm.md`
  - 角色：Admin Realm 管理员
  - 摘要：查看 Realm 详情，了解配置信息

- `[US-AR-004]` Realm 创建权限控制，优先级 P0，来源 `docs/user-stories/core/admin-realm.md`
  - 角色：Admin Realm 管理员
  - 摘要：只有拥有 realm.manage 权限的 Admin Realm 用户才能创建新 Realm

- `[US-AR-005]` 访问新创建的 Realm，优先级 P0，来源 `docs/user-stories/core/admin-realm.md`
  - 角色：Admin Realm 管理员
  - 摘要：创建 Realm 后能够访问该 Realm 的管理界面

- `[US-RA-001]` Realm 隔离访问，优先级 P0，来源 `docs/user-stories/core/realm-admin.md`
  - 角色：Realm Admin
  - 摘要：只能访问自己 Realm 的资源，保证数据隔离

### 1.2 优先级汇总

| 优先级 | 数量 | 关键故事 |
|--------|------|----------|
| P0 | 4 | 创建 Realm、查看 Realm 列表、Realm 创建权限控制、Realm 隔离访问 |
| P1 | 1 | 查看 Realm 详情 |
| P2 | 0 | - |

---

## 2. 范围界定

### 2.1 包含功能

- 多租户隔离（完全的数据和权限隔离）
- 通过 URL 中的 realm_id 参数进行上下文切换
- Realm 创建功能
- Realm 列表查看
- 基于 realm 的数据隔离和权限控制
- 前端路由设计（使用 `/$realmId/*` 路径）
- 认证集成（登录时需要指定 realm_id）
- 多 Realm 导航访问
  - 每个 Realm 拥有独立的 UI 界面
  - 通过 URL 路径 `/$realmId/*` 访问特定 Realm 的管理界面
  - Admin Realm 管理员可以切换到其他 Realm 验证配置
- Realm 名称和描述编辑（Realm ID 不可修改）
- Realm 详情查看

### 2.2 不包含功能 (Out of Scope)

- Realm 删除功能（数据库不支持级联删除，删除 realm 会导致数据孤立）
- Realm 列表独立管理页面（当前通过管理后台入口访问）
- 级联删除支持（数据库外键约束不支持级联删除）

### 2.3 依赖项

- 用户认证系统 — 提供登录和会话管理
- 权限管理系统 — 提供基于 Realm 的权限检查（resource.action 模型）
- 数据库系统 — PostgreSQL 数据存储
- RBAC 基础设施 — Realm 创建时自动初始化默认角色、权限和策略

---

## 3. 需求概述

### 3.1 功能描述

Realm（域）是 Herald 系统中的多租户隔离单位，每个用户、客户端应用、角色、配置都属于一个特定的 realm。本文档描述 Realm 的创建、查看、导航和数据隔离等功能需求。Realm 为不同组织提供独立的认证服务，是整个多租户架构的基础隔离层。

### 3.2 关键特性

- 完全的多租户数据隔离
- 通过 URL 路径中的 realm_id 参数进行上下文切换
- 基于 realm 的权限控制和资源隔离
- Realm 创建时自动初始化完整的 RBAC 基础设施
- Realm 创建时自动创建管理控制台客户端应用（admin-web-console）
- Realm 创建时自动创建 API Key 客户端应用（admin-api-client），用于 API Key 认证
- Realm 创建时自动创建个人中心客户端应用（user-account-center），供终端用户访问个人中心，浏览器 refresh 绝对上限 30 天
- Realm 创建时自动初始化注册配置（enabled: false）
- Realm 创建时自动创建管理员用户并分配角色，管理员用户状态自动设为 Normal（已验证）
- 当前不提供 Realm 删除功能

---

## 4. 业务规则与状态

### 4.1 业务规则

**权限级别与角色**

1. **主管理员（Super Admin / Admin Realm 管理员）**
   - 可以创建新的 realm（需要 `realm.manage` 权限）
   - 完全管理 admin realm
   - 不能直接管理其他 realm 的内部资源
   - 不能删除 realm（当前限制）

2. **次管理员（Realm Admin）**
   - 可以管理特定 realm 的用户和配置
   - 可以编辑 realm 的名称和描述
   - 不能删除 realm
   - 创建 realm 时指定的管理员用户自动成为该 realm 的管理员

3. **普通用户（Realm User）**
   - 只能访问被授权的 realm
   - 可以在所属 realm 中进行被授权的操作
   - 不能管理 realm 设置

**权限要求**

- **查看 Realm 列表**：仅 Super Admin（admin realm 中拥有 `realm.view` 权限的用户）
- **查看自己 Realm 详情**：无需权限（登录即可查看）
- **切换 Realm**：用户需要有访问目标 realm 的权限（通过用户-realm 关联检查）
- **创建 Realm**：需要 Admin Realm 的 `realm.manage` 权限
- **查看其他 Realm**：需要 Admin Realm 的 `realm.manage` 权限（普通 Realm Admin 只能查看自己 Realm）
- **编辑 Realm**：仅可编辑自己 Realm 的元数据，需要 `settings.manage` 权限（即使 Super Admin 也不能编辑其他 Realm 的元数据）
- **删除 Realm**：不提供该功能（当前限制）

**Realm ID 验证规则**

- 格式：仅字母数字、连字符（`-`）和下划线（`_`），必须以字母或数字开头，3-36 个字符
- 唯一性：全局唯一
- 保留词：禁止使用 `admin`、`system`、`api`、`www` 等保留词（不区分大小写）
- Realm ID 创建后不可修改

**Realm 创建初始化规则**

- 创建 realm 时必须指定管理员用户的 email 和 password
- 自动级联创建默认 web-console client app（client_id 固定为 `admin-web-console`）
- 自动创建 API Key 客户端应用（client_id 固定为 `admin-api-client`），用于 API Key 认证
- 自动创建个人中心客户端应用（client_id 固定为 `user-account-center`）
- 自动初始化注册配置（`registration.enabled: false`）
- 自动创建管理员用户并分配 `realm-admin` 角色
- 管理员用户状态自动设为 Normal（已验证），可立即登录
- 自动初始化默认 RBAC：角色定义（`realm-admin`、`user`）、权限定义、角色权限关联和 RBAC 策略
- 如果任何步骤失败（注册配置、客户端创建、RBAC 初始化、管理员用户创建等），创建失败返回错误，已创建的部分数据可能残留（已知限制，Realm 不支持删除）
- 详细默认角色和权限说明请参考 `docs/prd/core/permissions.md`

### 4.2 关键状态与异常

- **权限隔离**：Realm 级别的数据隔离，用户只能访问被授权的 realm 资源；后端验证用户是否有权访问目标 Realm 的资源，未授权时拒绝访问
- **列表按身份过滤**：Realm 列表接口按请求者身份过滤——非 admin realm 身份仅能看到自己有访问权的 realm；仅 Admin Realm 身份列出全部 realm（平台管理设计）
- **删除限制**：当前项目不支持 realm 删除，避免数据孤立和一致性问题
- **导航权限可见性**：非 Admin Realm 管理员（无 `realm.manage` 权限）看不到 "Realms" 菜单项；直接访问 URL 时返回权限不足提示
- **Admin Realm 管理员边界**：创建 Realm 后不能直接切换到新 Realm 的内部资源，需使用该 Realm 的管理员账号登录

---

## 5. 功能需求

### 5.1 核心需求

- **Realm 创建**：Admin Realm 管理员可创建新 Realm，指定 Realm ID（可选，留空自动生成 UUID v7，格式为字母数字、连字符和下划线，3-36 个字符）、名称、管理员 email 和密码；系统自动初始化 RBAC 基础设施、客户端应用和管理员用户
- **Realm 列表查看**：仅 Super Admin（admin realm 中拥有 `realm.view` 权限）可查看 Realm 列表，支持分页、排序和搜索
- **Realm 详情查看**：任何已认证用户可查看自己 Realm 的基本信息；Super Admin 可查看其他 Realm 详情
- **Realm 编辑**：可修改名称和描述，Realm ID 不可修改
- **Realm 导航与访问**：通过 URL 路径 `/$realmId/*` 访问特定 Realm 的管理界面；后端验证用户权限，实现跨 Realm 的导航隔离
- **多 Realm 数据隔离**：所有数据操作限定在当前 Realm 范围内，不同 Realm 之间严格隔离

### 5.2 验收目标

- Admin Realm 管理员能够成功创建新 Realm，创建后新 Realm 自动包含默认角色、权限、策略、客户端应用（admin-web-console、admin-api-client 和 user-account-center）和管理员用户（状态为已验证）
- 创建的 Realm 管理员能够登录并访问自己 Realm 的管理功能
- Realm Admin 只能访问自己 Realm 的资源，访问其他 Realm 资源时被拒绝
- Realm 列表支持分页、排序和搜索，正确显示所有 Realm 信息
- 非 Admin Realm 用户无法看到或访问 Realm 管理功能
- Realm 创建失败时（如 ID 冲突、格式错误、保留词冲突）显示明确的验证错误

---

## 6. API 相关约束

**适用性**: 适用

- Realm 管理能力的访问边界：Realm 创建、列表查询、编辑更新均受权限控制；查看自己 Realm 详情无需权限
- 创建 Realm 需要 Admin Realm 的 `realm.manage` 权限；列表查询需要 admin realm 的 `realm.view` 权限（仅 Super Admin）
- 编辑操作：仅可编辑自己 realm 的元数据，需要 `settings.manage` 权限（即使 Super Admin 也不能编辑其他 Realm 的元数据）
- 所有 Realm 相关接口必须遵守 realm 隔离原则，确保数据不跨 realm 泄露
- 敏感操作（如创建 Realm 时传入管理员密码）需遵循安全传输和存储要求
- 详细接口契约、验证规则和错误模型应在技术设计文档中维护

---

## 7. 前端/交互约束

**适用性**: 适用

- **页面入口**：Admin Realm 管理员通过左侧导航菜单中的 "Realms" 菜单项进入 Realm 管理页面；非 Admin Realm 用户（无 `realm.manage` 权限）看不到此菜单项
- **创建 Realm 交互**：点击页面右上角 "Create Realm" 按钮弹出对话框，填写 Realm ID（可选）、名称（必填）、管理员 email（必填）、管理员密码（必填），前端验证 ID 格式和唯一性后提交
- **创建成功反馈**：显示成功提示消息，自动刷新 Realm 列表，可选自动切换到新创建的 realm
- **列表交互**：表格展示所有 Realm（Realm ID、名称、创建时间、更新时间），支持分页、排序和搜索
- **详情与编辑**：点击 Realm 进入详情页面，可编辑名称和描述，Realm ID 只读不可修改
- **权限可见性**：不同角色看到的操作入口和数据范围保持一致；直接访问无权限的 URL 时显示权限不足提示或重定向
- **关键状态反馈**：创建失败时显示明确的验证错误（ID 格式、保留词、唯一性、密码强度等）

---

## 8. 已确认决策

### 8.1 已确认决策

- Realm 删除功能当前不提供：数据库不支持级联删除，删除会导致数据孤立
- Realm ID 创建后不可修改
- Realm 创建时自动初始化完整的 RBAC 基础设施（角色、权限、策略）
- Realm 创建时自动创建 admin-web-console client app、admin-api-client（用于 API Key 认证）和 user-account-center client app
- Realm 创建时自动初始化注册配置（registration.enabled: false）
- 创建的管理员用户状态自动设为 Normal（已验证），可立即登录
- 创建 Realm 时必须指定管理员 email 和密码，创建失败时返回错误（已创建的部分数据可能残留，Realm 不支持删除）
- Admin Realm 管理员创建 Realm 后不能直接切换到新 Realm 的内部资源
- Admin realm 管理员拥有 realm.manage 权限可创建新 realm，但不能编辑其他 realm 的元数据（仅可编辑自 realm）
- Realm 列表接口按身份过滤：非 admin realm 身份仅能看到自己有访问权的 realm；仅 Admin Realm 身份列出全部 realm（平台管理设计）

---

## 9. 参考资料

- 用户故事：`docs/user-stories/core/admin-realm.md`、`docs/user-stories/core/realm-admin.md`
- 相关 PRD：`docs/prd/core/realm-settings.md`、`docs/prd/core/users.md`、`docs/prd/core/permissions.md`
- 相关 PRD：`docs/prd/integration/client-app.md`（包含双 ID 系统详细说明）
- 相关 PRD：`docs/prd/auth/oauth.md`
