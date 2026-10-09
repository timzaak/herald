# Client App 管理产品需求文档 (PRD)

**创建时间**: 2025-01-05
**优先级**: P0

---

## 1. 相关用户故事

> 详细故事与验收标准请查看 `docs/user-stories/` 中对应文档。

| US-ID | 标题 | 优先级 | 来源 |
|-------|------|--------|------|
| US-TP-005 | Client App 配置管理（创建、client_secret、回调 URL 配置） | P0 | `docs/user-stories/auth/third-party-app.md` |
| US-TP-008 | 配置 Client App 跳转地址白名单 | P0 | `docs/user-stories/auth/client-app-settings.md` |
| US-TP-009 | 管理 Client App 图标 | P0 | `docs/user-stories/auth/client-app-settings.md` |
| US-TP-010 | 启用/禁用 Client App | P0 | `docs/user-stories/auth/client-app-settings.md` |
| US-TP-011 | 配置浏览器 token 生命周期策略 | P0 | `docs/user-stories/auth/client-app-settings.md` |

> 编号口径：US-TP-001~004 属 OAuth 授权流故事（见 [docs/prd/auth/oauth.md](../auth/oauth.md) §1）；Client App 的列表/详情/编辑/删除等管理端 CRUD 未单列用户故事，由 US-TP-005 配置管理与本 PRD §4 规则承载。

---

## 2. 范围界定

### 2.1 包含功能

- Client App 列表展示（分页；显示基本信息：图标、Client ID、名称、描述、Redirect URIs、浏览器 token 绝对上限、状态）
- 创建 Client App（含 Basic、Redirect URIs、Security、Appearance 四类配置；支持自定义 client_id，双 ID 系统：`id`: UUID（内部主键，用于数据库关联和 role_policies）+ `client_id`: string（外部标识符，必填，3-36 字符，字母数字、连字符和下划线））
- 编辑 Client App（名称、描述、启用状态）
- 删除 Client App（需要二次确认）
- OAuth 2.0 配置（redirect_uris、client_secret、enabled）
- 浏览器 token 生命周期配置（browser_refresh_absolute_ttl_seconds：refresh token 绝对有效上限，默认 2,592,000 秒/30 天，合法区间 86,400–7,776,000 秒）
- 浏览器 token 会话模型（短时效 access token + 旋转 refresh token + 复用检测吊销 + 绝对上限）
- 应用外观配置（icon_url）
- Client App 级人机验证（Turnstile）配置（启用开关、site_key、secret_key）——人机验证配置归属 Client App 级，不再由 Realm 承载（见 [docs/prd/core/realm-settings.md](../core/realm-settings.md) §7）
- Client Secret 重新生成
- Client App 快速切换（启用/禁用）
- Client App 作为 API Key 的作用域边界：绑定到该 App 的 API Key 可随 App 禁用而失效
- URL 安全验证（禁止 javascript: 协议、协议相对 URL）

### 2.2 不包含功能 (Out of Scope)

- Client App 作用域管理（无 OAuth 2.0 scope 管理功能）
- Client App 访问日志
- Client App 使用统计
- Client App 模板功能
- 批量导入/导出 Client App
- 内置 API Key Client App 管理（由系统自动创建和维护，见 [API Key Roles PRD](/docs/prd/integration/api-key-roles.md)）

### 2.3 依赖项

- 用户认证系统（提供登录和会话管理）
- 权限管理系统（Realm Admin 权限检查）
- Realm 系统（Client App 属于 Realm 级别）
- OAuth 2.0 系统（支持 OAuth 2.0 授权流程）

---

## 4. 业务规则与状态

### 4.1 业务规则

- Client App 属于 Realm 级别，所有操作受 Realm 隔离
- `client_id` 创建后不可修改，作为系统外部标识
- Client Secret 由系统自动生成（UUID），仅在创建/重新生成时返回一次，不在列表和详情查询中返回
- Redirect URIs：一旦提交列表就至少包含一个有效 URL，禁止 javascript: 和协议相对 URL；创建时可暂不提供，纯 Device Code Client 可在创建时提交空列表
- 删除 Client App 需要二次确认
- 浏览器 token 的 refresh 绝对上限策略在 token 家族签发时固化，后续配置修改只影响新签发的 token 家族
- Client App 会话 Cookie 的初始有效期可配置；续期后的有效期可配置，未设置表示不允许续期
- 禁用 Client App 会使绑定到该 App 的 API Key 在外部 API 认证中不可用；同时实时吊销该 App 名下全部浏览器 token 家族（已登录会话立即失效），删除 Client App 时同样吊销
- 删除 Client App 前必须不存在绑定 API Key；存在绑定时拒绝删除。数据库外键使用 `RESTRICT`，不得因删除把 App 级 Key 转成 Realm 级空绑定；NULL 仅兼容历史数据
- **人机验证（Turnstile）配置归属 Client App 级**：每个 Client App 配置自己的 Turnstile 启用开关、site_key 与 secret_key；未认证身份端点（注册/登录/找回密码/重置密码/邮箱验证/邮箱验证码登录）的人机验证按当前请求绑定的 Client App 的配置执行，未启用 Turnstile 的 Client App 不强制人机验证
- **Turnstile secret 不回显**：Turnstile secret_key 属敏感凭证，读取 Client App 详情时不返回；仅在创建/编辑时接受明文
- 内置第一方 Client App 不允许被删除：`admin-web-console`（管理控制台）与用户账户中心 Client App（`user-account-center`）；二者的 `client_id` 同时受保留字保护，新建 Client App 不得占用。防止内置管理/用户入口不可用
- 内置 API Key Client App（`admin-api-client`）同样不允许被删除（返回 400）：它仅在 Realm 创建时播种（默认停用，需管理员显式开启）、无自动重建路径，删除后该 Realm 的 API Key 将失去内置绑定宿主（见 [API Key Roles PRD](api-key-roles.md) §2.1）
- `device_code_grant_enabled` 字段控制是否启用 Device Code Grant 流程，默认 false；启用后允许该 Client App 参与 Device Code 授权流程

### 4.2 关键状态与异常

- **刷新与旋转**：access token 过期后，前端用 refresh token 换发新的 access token 和新的 refresh token，旧 refresh token 立即作废
- **复用检测**：已作废的旧 refresh token 被再次使用时视为泄露信号，吊销整个 token 家族，要求重新登录
- **绝对上限**：refresh token 到达 `browser_refresh_absolute_ttl_seconds` 绝对上限后拒绝刷新，要求重新登录
- **典型策略**：
  - 严格策略（如银行应用）：较短的绝对上限
  - 宽松策略（如企业工具）：较长的绝对上限（最长 90 天）

---

## 5. 验收目标

- Client App 全部 CRUD 操作可正常执行
- 浏览器 token 生命周期按旋转 refresh token 模型执行（刷新换发、复用检测吊销、绝对上限）
- URL 安全验证生效（拒绝 javascript: 协议和协议相对 URL）
- Client Secret 仅在创建/重新生成时展示一次
- 所有操作遵守 Realm 隔离原则
- 作为 API Key 作用域时，普通 Client App 只能授权其自身资源；`admin-api-client` 是系统内置的 Realm 级 API Key 作用域

---

## 6. 边界与约束

**适用性**: 适用（API 与前端/交互边界合并陈述）

**API / 集成边界:**
- 接口能力范围：Client App 的创建、查询列表、查询详情、更新、删除，以及 OAuth 2.0 和会话配置管理
- 访问控制：所有操作需 Realm Admin 权限，遵守 Realm 隔离原则
- ext API 通过 API Key 认证提供第三方集成接口（Client App 的创建、列表和详情查询）；创建时 `client_id` 由系统自动生成（UUID v7），不允许自定义
- 详细接口契约、认证方式和错误模型应下沉到技术设计文档

**前端 / 交互边界:**
- 页面入口：管理后台 Client Apps 菜单，位于左侧导航栏
- 关键用户路径：列表浏览 -> 创建/编辑 -> 配置 OAuth 与会话 -> 管理 Secret
- 创建表单采用 Tabs 布局（Basic、Redirect URIs、Security、Appearance）
- 编辑模式下 Client ID 只读，新增 Regenerate Secret 选项
- 删除操作需二次确认
- 刷新由集成方前端持有 refresh token 并主动调用刷新接口完成，后端负责旋转、复用检测与绝对上限判定

---

## 7. 已确认决策

- **client_id 格式**：仅允许 ASCII 字母数字、连字符（`-`）和下划线（`_`），3-36 字符
- **ext API 自动生成 client_id**：ext API 创建 Client App 时使用 UUID v7 自动生成 client_id，不允许自定义；admin API 允许自定义 client_id
- **人机验证配置归属 Client App 级（D-PROTECT-01）**：Turnstile 配置（启用开关、site_key、secret_key）归属 Client App 级，不再由 Realm 承载；未认证身份端点的人机验证按当前请求绑定的 Client App 的配置执行。该决策与 Realm Settings PRD、自建用户 UI PRD 的 Turnstile 表述同步（见 [docs/prd/core/realm-settings.md](../core/realm-settings.md) §8、[docs/prd/integration/custom-user-ui.md](custom-user-ui.md) D-PROTECT-01）

---

## 8. 参考资料

- 相关 PRD：`docs/prd/integration/api-key-roles.md`（API Key 角色绑定）
- 相关 PRD：`docs/prd/auth/oauth.md`（OAuth 2.0）
- 相关 PRD：`docs/prd/core/realm.md`（Realm 管理）
- 用户故事来源见 §1 表格
