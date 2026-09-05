# Realm Settings 产品需求文档 (PRD)

**创建时间**: 2025-01-05
**优先级**: P0

---

## 1. 相关用户故事

> 详细故事与验收标准请查看 `docs/user-stories/` 中对应文档。

### 1.1 故事引用

- `[US-RA-008]` 配置 Realm 设置 (P0)，来源 `docs/user-stories/core/realm-admin.md`
  - 角色：Realm Admin
  - 摘要：作为 Realm Admin，配置 Realm 设置（注册策略、OAuth Provider、邮件服务），管理本 Realm 的安全和访问控制

- `[US-RA-013]` 配置 Realm 邮件服务 (P0)，来源 `docs/user-stories/core/realm-admin.md`
  - 角色：Realm Admin
  - 摘要：配置邮件发送方式（Resend API 或 SMTP），让本 Realm 独立发送系统邮件

- `[US-RA-014]` 发送测试邮件 (P1)，来源 `docs/user-stories/core/realm-admin.md`
  - 角色：Realm Admin
  - 摘要：发送测试邮件验证配置正确性

- `[US-RA-015]` 邮件依赖功能开关前置验证 (P0)，来源 `docs/user-stories/core/realm-admin.md`
  - 角色：Realm Admin
  - 摘要：未配置邮件时无法开启邮箱验证等邮件依赖功能

### 1.2 优先级汇总

| 优先级 | 数量 | 关键故事 |
|--------|------|----------|
| P0 | 3 | 配置 Realm 设置、配置邮件服务、功能开关前置验证 |
| P1 | 1 | 发送测试邮件 |
| P2 | 0 | - |

---

## 2. 范围界定

### 2.1 包含功能

- Realm Config 管理（Registration、Email、TOTP、TotpKey、Creem、Stripe；Turnstile 仅遗留兼容，见 §8）
- OAuth Provider 配置管理（独立系统，不在 Realm Config 中管理）
- Email 邮件服务配置（Per-Realm，支持 Resend / SMTP）
- 邮件依赖功能开关前置验证
- 前端 Settings 页面（多 Tab 布局）
- 配置项批量更新（batch_upsert）和删除（delete）
- 测试邮件发送（含速率限制）

### 2.2 不包含功能 (Out of Scope)

- 端到端测试
- 配置模板功能（无预定义配置模板）
- 会话配置、密码策略配置（计划中，当前代码中无对应 config_key）
- `default_user_status` 字段（计划中，当前代码中无此字段）

### 2.3 依赖项

- **Realm 系统** — Config 属于 Realm 级别，依赖 Realm 基础设施
- **权限管理系统** — Realm Admin 权限检查
- **OAuth Provider 系统** — OAuth Provider 配置管理

---

## 3. 需求概述

### 3.1 功能描述

在 Herald 管理后台提供 Realm Settings 功能，允许 Realm Admin 管理本 Realm 的各类配置项，包括用户注册配置、OAuth Provider 配置和邮件服务配置。Settings 页面通过多 Tab 布局组织不同配置类型，每个 Tab 包含独立的配置表单。

> **人机验证（Turnstile）配置位置**：Turnstile 配置**已下放到 Client App 级别**（每个 Client App 配置自己的 Turnstile site_key/secret_key 与启用开关），不再作为 Realm 级配置管理。Realm Config 中的 `turnstile` 配置类型仅保留遗留兼容，新的人机验证配置入口在 Client App（见 [docs/prd/integration/client-app.md](../integration/client-app.md)）。

### 3.2 关键特性

- 分 Tab 布局管理多种配置类型（Registration、OAuth、Email 等）
- 每种配置类型独立启用/禁用，支持保存和重置
- 支持单个配置项 Upsert、批量 Upsert（batch_upsert）和删除（delete）
- 邮件服务支持 Resend API 和 SMTP 两种 Provider
- 邮件依赖的功能开关（如邮箱验证）需前置校验邮件配置完整性
- OAuth 配置通过独立系统管理，不在 Realm Config 中
- 测试邮件发送带速率限制（3 次 / 60 秒）

---

## 4. 业务规则与状态

### 4.1 业务规则

- **Realm 隔离**：所有配置项属于 Realm 级别，不同 Realm 的配置相互独立
- **权限要求**：仅 Realm Admin 角色可查看和修改 Realm Settings
- **敏感信息脱敏**：密码、密钥类字段（Resend API Key、SMTP 密码等）在展示时必须脱敏，编辑时才暴露为输入框
- **邮件配置完整性定义**：provider + from_address + 对应 provider 的必填字段均已填写（不检查 enabled 标志，仅检查字段非空）
- **功能开关前置验证**：`require_email_verification` 开关仅在邮件配置完整时可开启；未配置邮件时，该开关显示为禁用状态，提示 "Email verification requires email configuration"
- **OAuth 配置独立**：OAuth Provider 有独立配置系统，不在 Realm Config 中管理
- **测试邮件速率限制**：同一 realm + 用户组合限制 3 次 / 60 秒，超限返回 429

### 4.2 关键状态与异常

- **未配置邮件 + 尝试开启邮箱验证**：开关禁用，显示提示信息，阻止开启
- **Provider 切换**：切换邮件 Provider 时，隐藏/显示对应字段（Resend 显示 API Key；SMTP 显示 Host/Port/Username/Password）
- **测试邮件**：保存配置后可通过 "Send Test Email" 验证配置正确性，速率限制 3 次/60 秒
- **Registration 键名统一**：注册开关使用 `config_key = "enabled"`，创建 Realm、查询注册状态和 public config 均使用同一键名。

> **计划中功能**：以下功能在 PRD 中曾提及但当前代码无实现，移至未来扩展：
> - `default_user_status`（Registration 配置中新用户默认状态，取值范围 0-3）
> - 密码策略配置（最小长度、大小写、数字、特殊字符要求）

---

## 5. 功能需求

### 5.1 核心需求

- **Registration 配置**：管理用户注册策略，包括是否开放注册（enabled）、是否需要邮箱验证（require_email_verification）、允许的邮箱域名（allowed_domains）。`allowed_domains` 为空时不限制；非空时按标准化后的邮箱域名做不区分大小写的精确匹配，不把子域自动视为父域命中。该限制覆盖密码注册、Email OTP 自动注册及 OAuth/One Tap 等首次自动建号，已存在账号的登录不受影响
- **Email 配置**：管理邮件服务（Resend 或 SMTP），包括发件人地址、Provider 特定参数
- **OAuth 配置**：通过独立系统管理 OAuth Provider（不在本页面详细定义）
- **TOTP 配置**：管理 TOTP 二次认证开关设置
- **TOTP 密钥配置**（TotpKey）：存储 Realm 级别的 TOTP 加密密钥
- **支付提供商配置**：Creem、Stripe 的 API Key / Webhook Secret 等配置（已迁移到独立 Payment Providers 页面，此处仅保留遗留兼容）
- **Settings 页面**：多 Tab 布局，每个配置类型对应一个 Tab，包含启用/禁用开关、配置表单、保存/重置按钮

> 人机验证（Turnstile）配置不在此页面管理，见 §3.1 Client App 级配置说明。

### 5.2 验收目标

- Realm Admin 能通过 Settings 页面成功配置 Registration、Email 各项参数
- 邮件服务配置保存后，可通过测试邮件功能验证配置正确性
- 未配置邮件时，邮箱验证开关处于禁用状态并有明确提示
- 敏感字段在页面展示时脱敏，仅在编辑时可见
- 不同 Realm 的配置相互隔离
- 支持批量更新配置（batch_upsert）和删除单个配置项
- 测试邮件受速率限制保护

---

## 6. API 相关约束

**适用性**: 适用

- 接口能力范围：Realm Config 的查询、单个 Upsert、批量 Upsert（batch_upsert）、删除（delete），涵盖 registration、email、totp、totp_key、passkey、white_label、custom_domain、ldap、email_otp、platform_signup、stripe、creem、apple、google、wechat、invoice_policy、turnstile 配置类型（以 ConfigType 枚举为准），以及 OAuth Provider 的独立配置管理。`turnstile` 配置类型仅保留遗留兼容，不再承载有效配置（见 §3.1、§8）
- 访问控制原则：所有接口要求 Realm Admin 权限，操作需通过 Realm 归属校验
- 数据边界原则：配置数据按 Realm 隔离，不同 Realm 之间不可交叉访问
- 敏感信息处理：密码、密钥等敏感字段在读取时脱敏返回（is_secret=true 时 config_value 返回 null），仅在写入时接受明文
- 审计要求：关键配置变更应记录审计日志
- 测试邮件速率限制：同一 realm + 用户 3 次 / 60 秒，超限返回 429 Too Many Requests
- 详细接口契约、验证规则和错误模型在技术设计文档中维护

---

## 7. 前端/交互约束

**适用性**: 适用

- **页面入口**：管理后台左侧导航栏 Settings 菜单项，realmId 从 UI 上下文获取
- **页面布局**：多 Tab 布局，每个配置类型对应一个 Tab（Registration、OAuth、Email 等；Turnstile 不在此页面，见 §3.1）
- **每个 Tab 包含**：配置标题、启用/禁用开关、配置项表单、保存/重置按钮
- **敏感字段交互**：密码/密钥类字段展示脱敏占位符，点击编辑后变为输入框
- **Email Provider 切换**：切换 Provider 时动态隐藏/显示对应字段
- **功能开关联动**：未配置邮件时，Registration Tab 中邮箱验证开关显示为禁用状态，并提示原因
- **操作反馈**：保存成功/失败有明确反馈，测试邮件发送有结果反馈
- **角色差异**：仅 Realm Admin 可见和操作 Settings 入口

---

## 8. 已确认决策

### 8.1 已确认决策

- OAuth 配置使用独立系统管理，不纳入 Realm Config 存储结构
- 邮件服务配置纳入 Realm Config 管理，使用 `email` 配置类型
- Settings 页面使用多 Tab 布局而非分组卡片布局
- 支付提供商配置（Creem/Stripe）已迁移到独立的 Payment Providers 页面管理，不再通过 Realm Config 管理。Realm Config 中的 creem/stripe 配置类型仅用于遗留兼容，新功能应在 Payment Providers 页面操作
- 人机验证（Turnstile）配置已下放到 Client App 级别（每个 Client App 配置自己的 Turnstile site_key/secret_key 与启用开关），不再作为 Realm 级配置管理。Realm Config 中的 `turnstile` 配置类型仅保留遗留兼容，新的人机验证配置入口在 Client App（见 [docs/prd/integration/client-app.md](../integration/client-app.md)）

### 8.2 已知限制

- **Registration 键名统一**：创建 Realm、注册状态查询和 public config 均使用 `config_key = "enabled"`。
- **config_type 枚举强校验**：HTTP 写路径（单个 upsert、批量 batch_upsert、删除 delete）对未知 config_type 一律返回 400
- **邮件完整性不检查 enabled 标志**：`is_email_configured` 仅检查字段非空，不检查各条目的 `enabled` 字段。即使用户禁用了某个邮件配置项，只要字段非空仍视为已配置

### 8.3 计划中功能

- **密码策略配置**：最小长度、大小写、数字、特殊字符要求。当前代码中 ConfigType::Registration 无对应 config_key，未来可能新增 `password_min_length`、`password_require_uppercase` 等 key
- **新用户默认状态**：`default_user_status`（取值范围 0-3）。当前代码中无此字段，未来可能作为 Registration 类型的 config_key 新增
- **会话配置**：暂不在 Realm Config 中管理

---

## 9. 参考资料

- 用户故事：`docs/user-stories/core/realm-admin.md`
- 相关 PRD：`docs/prd/core/realm.md`
- 相关 PRD：`docs/prd/core/users.md`
- 相关 PRD：`docs/prd/integration/client-app.md`
