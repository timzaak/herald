# Device Code 登录产品需求文档 (PRD)

**创建时间**: 2026-05-14
**优先级**: P0

---

## 1. 相关用户故事

> 详细故事与验收标准请查看 `docs/user-stories/` 中对应文档。

### 1.1 故事引用

- `[US-DC-001]` CLI 工具发起设备授权，优先级 P0，来源 `docs/user-stories/auth/device-code.md`
  - 角色：Third-Party App
  - 摘要：CLI 通过 Device Authorization Grant 请求 device_code 和 user_code
- `[US-DC-002]` 用户在验证页面完成授权，优先级 P0，来源 `docs/user-stories/auth/device-code.md`
  - 角色：Regular User
  - 摘要：用户在 Herald 验证页面输入 user_code 并完成登录授权
- `[US-DC-003]` CLI 工具轮询获取令牌，优先级 P0，来源 `docs/user-stories/auth/device-code.md`
  - 角色：Third-Party App
  - 摘要：CLI 按 interval 轮询令牌端点，用户授权后获得 access token
- `[US-DC-004]` Realm Admin 配置 Device Code Grant，优先级 P1，来源 `docs/user-stories/auth/device-code.md`
  - 角色：Realm Admin
  - 摘要：管理员为 Client App 启用或禁用 Device Code Grant
- `[US-DC-005]` 设备验证页面 API，优先级 P1，来源 `docs/user-stories/auth/device-code.md`
  - 角色：Third-Party App
  - 摘要：开放 API 供第三方应用构建自定义设备码验证体验

### 1.2 优先级汇总表

| 优先级 | 数量 | 关键故事 |
|--------|------|----------|
| P0 | 3 | 设备授权请求、用户验证授权、令牌轮询 |
| P1 | 2 | Client App 配置、验证页面 API |
| P2 | 0 | - |

---

## 2. 范围界定

### 2.1 包含功能

- 设备授权请求（Device Authorization Request，RFC 8628 §3.1、§3.2）
- 令牌轮询（Device Access Token Request，RFC 8628 §3.4、§3.5）
- Herald 前端设备验证页面（`/{realmId}/device` 路由）
- `verification_uri_complete` 支持（URL 中嵌入 user_code）
- Device Code Grant 在 Client App 中的启用/禁用配置
- 设备验证 API（供第三方应用自定义验证流程）
- 协议规定的全部错误码：`authorization_pending`、`slow_down`、`expired_token`、`access_denied`，以及扩展错误码 `invalid_request`（consumed 状态时返回）

### 2.2 不包含功能 (Out of Scope)

- QR 码生成（可在后续迭代中添加，CLI 工具可自行生成）
- 独立的 Device Flow 刷新协议；首次换取令牌时复用浏览器 token family，响应会包含可轮换的 `refresh_token`，其轮换与撤销规则沿用现有 OAuth 会话
- Scope 管理（当前系统无 OAuth scope 管理，与现有 OAuth 一致）
- PKCE（Device Code Flow 不适用 PKCE，RFC 8628 未要求）
- 标准授权码流程改造（本功能为独立 grant_type，不影响现有流程）

### 2.3 依赖项

- Client App 系统 — 复用 client_id 和 Client App 配置模型
- Session Token 系统 — 复用 Session Token 生成与验证
- 缓存/存储基础设施 — 用于存储 device_code 等临时状态
- 用户认证系统 — 验证页面复用登录能力
- 权限管理系统 — 复用 RBAC 权限检查

---

## 3. 需求概述

### 3.1 功能描述

为 Herald 新增 OAuth 2.0 Device Authorization Grant（RFC 8628）支持，主要服务于 CLI 工具认证场景。

在 CLI 工具等无浏览器或输入受限的环境中，用户无法通过传统的授权码流程完成 OAuth 认证。Device Code Flow 通过将认证过程分离到用户的浏览器（手机或电脑）上，使 CLI 工具能在终端环境下安全完成用户认证。

**核心价值**：为第三方 CLI 应用提供标准化、安全的认证方式，降低集成门槛，提升用户体验。

### 3.2 关键特性

- **RFC 8628 完整合规**：实现协议规定的全部端点、参数和错误码
- **复用现有架构**：复用 Client App 模型和 Session Token 机制
- **双通道验证**：Herald 提供默认验证页面，同时开放 API 供第三方自定义
- **安全防护**：短生命周期码、轮询限速、展示 Client App 名称防钓鱼

---

## 4. 业务规则与状态

### 4.1 业务规则

**设备授权流程**
1. CLI 工具通过 `client_id` 请求 `device_code` 和 `user_code`
2. 响应包含 `verification_uri`、`verification_uri_complete`、`expires_in`（默认 900 秒）、`interval`（默认 5 秒）
3. 用户在 Herald 验证页面输入 `user_code`、登录、查看 Client App 名称并确认授权
4. CLI 工具以指定间隔轮询令牌端点，系统返回 `authorization_pending`、`slow_down`、`expired_token`、`access_denied`、`invalid_request`，或包含 access token 与可轮换 refresh token 的浏览器 token family
5. Realm Admin 可为每个 Client App 独立启用或禁用 Device Code Grant

**user_code 生成规则**
- 长度：8 字符，格式 `XXXX-XXXX`（4+4，连字符分隔）
- 字符集：base-20 编码，排除易混淆字符（0、O、1、I、L）
- 有效字符：`B C D F G H J K M N P Q R S T V W X Y Z`（共 20 个字符，纯大写，无数字）
- 大小写：统一大写显示（验证前自动将输入转为大写）
- 唯一性：
  - 生成后通过 Redis `EXISTS` 显式碰撞检查，冲突时重新生成，最多重试 5 次；重试耗尽仍冲突则报错拒绝

**API 能力边界**
- 不需要 `redirect_uri` 参数（与授权码流程的关键区别）
- 不需要 `client_secret`（适用于 public client / CLI 场景）
- verify 与 confirm 各自按用户限制为每 300 秒 20 次。
- 授权请求入口按来源 IP 限制为每 60 秒 10 次；当前不维护“单 Client App 同时处于 pending 的设备码数量”这一额外状态

### 4.2 关键状态与异常

**device_code 生命周期**
- 高强度随机性（完全随机 UUID v4，不使用带时间戳前缀的 v7），不可猜测或枚举
- 有效期：默认 900 秒（15 分钟），由 Redis TTL 自然过期，过期后 Redis key 自动删除
- 状态机（所有状态转换不可逆）：
  ```
  pending → verified → authorized → consumed
                    ↘ denied
  ```
  - `pending`：初始状态，device_code 生成后等待用户验证
  - `verified`：用户在验证页面输入 user_code 后的中间状态，此时 user_id 已绑定
  - `authorized`：用户在确认页面点击"授权"后的状态，CLI 可领取 token
  - `denied`：用户在确认页面点击"拒绝"后的终态
  - `consumed`：CLI 成功领取 token 后的终态，防止重复领取
  - `expired`：非显式状态，由 Redis TTL 自然过期（key 被删除后查询返回 `expired_token`）

**Realm 隔离校验**
- 所有端点（authorize、verify、confirm、token）均校验存储的 `realm_id` 与路径参数 `realmId` 一致
- 不匹配时返回 `invalid_request` 错误（realm mismatch），且不消费已授权的 device code

**幂等性**
- 同一用户重复调用 verify 端点验证同一 user_code 时，幂等返回 Client App 信息，不会重复修改状态
- 不同用户尝试验证已被其他用户验证的 user_code 时，返回 `already_used` 错误

**轮询错误码**
- `authorization_pending`：用户尚未完成授权（状态为 pending 或 verified），CLI 应继续轮询
- `slow_down`：轮询过快，CLI 应在当前间隔基础上增加 5 秒
- `expired_token`：device_code 已过期（Redis key 不存在），需重新发起授权请求
- `access_denied`：用户拒绝授权（状态为 denied）
- `invalid_request`：device_code 已被消费（状态为 consumed），不可重复领取 token

---

## 5. 功能需求

### 5.1 核心需求

1. **设备授权请求**：支持 CLI 工具通过 `client_id` 获取 `device_code`、`user_code`、`verification_uri` 等参数
2. **用户验证授权**：提供 Herald 验证页面供用户输入 user_code、登录并确认授权
3. **令牌轮询**：支持 CLI 工具按 interval 轮询，正确返回全部协议错误码
4. **Client App 配置**：Realm Admin 可为每个 Client App 独立启用或禁用 Device Code Grant

### 5.2 验收目标

- P0 场景（US-DC-001 ~ US-DC-003）全部通过，CLI 工具可完成完整的设备码认证流程
- P1 场景（US-DC-004 ~ US-DC-005）通过，管理员可配置、第三方可自定义验证页面
- 与现有授权码流程互不干扰

---

## 6. API 相关约束

**适用性**: 适用

- 设备授权请求需验证 `client_id` 有效且 Client App 已启用 Device Code Grant
- 令牌轮询需正确实现 RFC 8628 §3.5 规定的全部错误响应
- 轮询端点需对 `slow_down` 错误正确累加间隔（每次 +5 秒）
- 验证页面 API 需要求用户已登录（session 认证）
- 所有端点遵守 realm 隔离原则

---

## 7. 前端/交互约束

**适用性**: 适用

### 验证页面（`/{realmId}/device`）

- **入口**：Herald 前端新增 `/{realmId}/device` 路由，与 realm 绑定（登录跳转、API 调用均基于路径中的 realmId）
- **输入**：用户输入 `user_code`，8 字符输入框（自动格式化为 `XXXX-XXXX`）
- **授权确认**：显示请求授权的 Client App 名称和图标（如果配置了 icon_url），用户点击"授权"或"拒绝"
- **状态反馈**：
  - 输入无效/过期码：提示"设备码无效或已过期"
  - 已登录用户直接看到授权确认页面
  - 未登录用户先跳转 `/{realmId}/auth/login`，登录后回到验证页面
  - 授权成功：提示"授权成功，请返回 CLI 工具"
  - 授权拒绝：提示"授权已拒绝"
- **URL 预填**：通过 `verification_uri_complete` 访问时，`user_code` 自动填入输入框

### Client App 设置

- 在现有 Client App 设置页面中新增 Device Code Grant 启用/禁用开关
- 默认为禁用状态

---

## 8. 已确认决策

### 8.1 已确认决策
- 复用现有 Client App 模型和 Session Token 机制 / 降低实现复杂度
- 双通道验证策略（Herald 默认页面 + 第三方自定义 API） / 兼顾标准化和灵活性

---

## 9. 参考资料
- 用户故事：`docs/user-stories/auth/device-code.md`
- RFC 8628 — OAuth 2.0 Device Authorization Grant: https://datatracker.ietf.org/doc/html/rfc8628
- 相关 PRD：`docs/prd/auth/oauth.md`
- 相关 PRD：`docs/prd/integration/client-app.md`
- Auth0 Device Authorization Flow: https://auth0.com/docs/get-started/authentication-and-authorization-flow/device-authorization-flow
- WorkOS Device Authorization Grant 实践指南: https://workos.com/blog/oauth-device-authorization-grant
