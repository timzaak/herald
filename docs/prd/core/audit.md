# Audit 审计日志产品需求文档 (PRD)

**创建时间**: 2026-05-13
**优先级**: P0

---

## 1. 相关用户故事

> 详细故事与验收标准请查看 `docs/user-stories/` 中对应文档。

### 1.1 相关故事

- `[US-AU-001]` 查看 Realm 审计日志，优先级 P0，来源 `docs/user-stories/core/audit.md`
  - 角色：Realm Admin
  - 摘要：Realm Admin 查看当前 Realm 下所有核心操作的审计日志列表，支持 Realm 隔离

- `[US-AU-002]` 按条件筛选审计日志，优先级 P0，来源 `docs/user-stories/core/audit.md`
  - 角色：Realm Admin
  - 摘要：按事件类型、操作者和时间范围筛选审计日志

- `[US-AU-003]` 查看审计日志详情，优先级 P1，来源 `docs/user-stories/core/audit.md`
  - 角色：Realm Admin
  - 摘要：查看某条审计日志的完整变更详情和上下文

- `[US-AU-004]` 查看 Admin Realm 审计日志，优先级 P0，来源 `docs/user-stories/core/audit.md`
  - 角色：Admin Realm 管理员
  - 摘要：查看 Admin Realm 的平台级审计日志

- `[US-AU-005]` 系统自动记录核心操作，优先级 P0，来源 `docs/user-stories/core/audit.md`
  - 角色：Herald 系统
  - 摘要：核心操作（用户管理、RBAC 变更、Realm 管理、认证事件）发生时自动记录审计事件

### 1.2 优先级汇总

| 优先级 | 数量 | 关键故事 |
|--------|------|----------|
| P0 | 4 | 查看 Realm 审计日志、按条件筛选、查看 Admin Realm 审计日志、系统自动记录核心操作 |
| P1 | 1 | 查看审计日志详情 |
| P2 | 0 | - |

---

## 2. 范围界定

### 2.1 包含功能

- 核心操作审计事件的自动采集与持久化存储
- 审计日志查询能力（列表、筛选、详情）
- Realm 级别的审计数据隔离
- 以下操作类别的审计覆盖：
  - **用户管理**：用户创建、更新、删除
  - **RBAC 变更**：角色创建/更新/删除、权限定义创建/更新/删除（`permission.create` / `permission.update` / `permission.delete`）、权限授予/撤销、角色分配/取消、权限拒绝（`rbac.permission_denied`）
  - **Realm 管理**：Realm 创建、RBAC 初始化
  - **认证事件**：用户登录、登出、登录失败、Passkey 注册/删除、Client App 切换（`auth.client_switch`）
  - **合规事件**：用户协议/隐私政策同意（`agreement.consent`）、协议发布（`agreement.published`）、协议回退（`agreement.reverted`）
  - **关键配置变更**（边界定义）：经通用 realm_config API 的所有配置行写入/删除均记审计——支付 Provider（Stripe/Creem/Apple/Google/WeChat）记 `payment_config.update` / `payment_config.delete`，其余配置类型（SMTP/Resend、LDAP、Turnstile、注册策略、`totp_key` 等）记 `realm_config.update` / `realm_config.delete`；白标草稿保存/丢弃、发布和恢复同样记 `realm_config.update/delete` 并在 details 标注生命周期操作；认证策略配置经专用端点写入时记专用事件（`passkey_config.update` / `totp_config.update` / `email_otp_config.update`）；OAuth Provider 配置记 `oauth_config.*`。自定义域名与 Realm 档案（名称/描述）编辑当前不在审计范围

### 2.2 不包含功能 (Out of Scope)

- 审计日志的导出功能（后续扩展）
- 审计日志的保留策略和自动清理（后续扩展）
- 实时审计告警和通知
- 审计日志的不可篡改保证（如区块链存证）
- 计费相关操作的审计（分层事实标准，经 wechat-support.md §4.1 局部修订）：支付 Provider 的配置变更（Stripe/Creem/WeChat/IAP）与 WeChat 支付回调（含重放拒绝 `payment.replay`）进入统一审计；Stripe/IAP/Creem 的支付操作事件维持独立追踪表（`payment_event` 等），可从现有数据推导操作者，不纳入统一审计；IAP 的凭证提交与平台通知兑付记入统一审计（`iap.receipt_submit` / `iap.notification`）
- 发票操作的审计：发票状态变更历史由发票模块自行记录，含操作者信息

### 2.3 依赖项

- Realm 系统 — 审计日志按 Realm 隔离
- 用户认证系统 — 提供操作者身份信息
- RBAC 权限系统 — 审计日志访问需要权限控制
- 现有 RBAC Audit Logger — 当前仅输出到 tracing 日志，需扩展为持久化存储

---

## 3. 需求概述

### 3.1 功能描述

Herald 系统需要为所有核心操作提供可追溯的审计能力。当前系统仅有 RBAC 模块的部分审计日志（`RbacAuditLogger`），且仅通过 `tracing` 输出到标准输出，未持久化存储，无法供管理员查询和追溯。

本功能的目标是：
1. 建立统一的审计事件模型，覆盖用户管理、RBAC 变更、Realm 管理和认证事件四大类别
2. 将审计事件持久化存储到数据库
3. 提供管理后台的审计日志查看和筛选能力

### 3.2 关键特性

- **统一审计模型**：所有核心操作使用统一的审计事件结构（操作者、操作类型、目标对象、操作结果、时间戳）
- **Realm 隔离**：审计日志严格遵循 Realm 隔离原则，管理员只能查看所属 Realm 的审计记录
- **操作全记录**：成功的操作和失败的操作尝试都记录审计事件
- **可查询**：管理员可通过事件类型、时间范围、操作者等条件筛选审计日志

---

## 4. 业务规则与状态

### 4.1 业务规则

- 审计事件包含操作者标识、操作类型、目标对象（类型+ID）、Realm ID、操作结果、时间戳和变更详情
- 成功操作和失败操作尝试均须记录，失败事件标记操作结果为失败
- 审计日志为只读资源，不提供修改或删除接口
- 审计日志查询由 `audit.view` 权限控制；内置 Realm Admin 与 Admin Realm 管理员默认持有该权限，自定义角色也可被显式授予
- 审计日志严格按 Realm 隔离，不同 Realm 的管理员无法看到彼此的审计记录

### 4.2 关键状态与异常

- **新 Realm 空态**：新创建的 Realm 无审计记录时，查询返回空列表
- **操作失败记录**：权限不足、参数错误等失败操作同样产生审计事件，结果标记为失败
- **服务重启**：审计事件持久化到数据库，服务重启后数据不丢失

---

## 5. 功能需求

### 5.1 核心需求

1. **统一审计事件模型**：设计通用的审计事件结构，覆盖所有核心操作类别，每个事件包含操作者标识、操作类型、目标对象（类型+ID）、Realm ID、操作结果、时间戳和变更详情
2. **审计事件持久化**：所有审计事件写入数据库，保证服务重启后数据不丢失
3. **自动采集集成**：在现有核心操作流程中集成审计事件采集点，覆盖用户管理、RBAC 变更、Realm 管理和认证事件
4. **审计日志查询**：提供按 Realm 隔离的审计日志查询能力，支持按事件类型、时间范围和操作者筛选
5. **失败操作记录**：操作失败（权限不足、参数错误等）也应记录审计事件，标记操作结果为失败

### 5.2 验收目标

- 所有 P0 用户故事的验收标准全部通过
- 核心操作（用户管理、RBAC 变更、Realm 管理、认证事件）在执行后均可在审计日志中查到对应记录
- 审计日志严格遵循 Realm 隔离，不同 Realm 的管理员无法看到彼此的审计记录
- 操作失败同样被记录并可查询

---

## 6. API 相关约束

**适用性**: 适用

- 审计日志查询接口仅返回当前用户所属 Realm 的审计记录，遵循 Realm 隔离原则
- 查询接口支持分页，按操作时间倒序返回
- HTTP 与 MCP 审计查询均要求 `audit.view`；内置管理员角色默认拥有该权限，授权给自定义角色或 API Key 时沿用同一权限语义
- category/action 非法筛选值返回 400，不静默忽略；认证方式只保存在事件 `details.method` 中，当前无独立 method 筛选参数
- 审计日志为只读资源，不提供修改或删除接口
- 事件写入接口仅限系统内部调用，不对外暴露

---

## 7. 前端/交互约束

**适用性**: 适用

- 页面入口：管理后台新增审计日志页面，对持 `audit.view` 的身份可见
- 关键交互：页面以列表形式展示审计日志，默认按操作时间倒序排列
- 筛选控件：提供事件类型、时间范围和操作者的筛选
- 详情查看：支持点击单条记录查看详情（P1）
- 状态反馈：无数据时显示空状态提示

---

## 8. 已确认决策

### 8.1 已确认决策

- 审计事件采用统一模型，覆盖四大操作类别（用户管理、RBAC 变更、Realm 管理、认证事件）
- 计费和发票操作的审计由各自模块独立记录，不纳入统一审计模型
- 审计日志为只读，不提供修改和删除能力

---

## 9. 参考资料

- 用户故事：`docs/user-stories/core/audit.md`
- 权限管理 PRD：`docs/prd/auth/permissions.md`
- 角色定义：`docs/user-stories/_roles.md`
