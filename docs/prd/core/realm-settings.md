# Realm Settings 产品需求文档 (PRD)

**创建时间**: 2025-01-05
**优先级**: P0

---

## 1. 相关用户故事

> 详细故事与验收标准请查看 `docs/user-stories/` 中对应文档。

| US-ID | 标题 | 优先级 | 来源 |
|-------|------|--------|------|
| US-RA-013 | 配置 Realm 邮件服务 | P0 | `docs/user-stories/core/realm-admin.md` |
| US-RA-014 | 发送测试邮件 | P1 | `docs/user-stories/core/realm-admin.md` |
| US-RA-015 | 邮件依赖功能开关前置验证 | P0 | `docs/user-stories/core/realm-admin.md` |

---

## 2. 范围界定

### 2.1 包含功能

- Realm Config 管理（Registration、Email、TOTP、TotpKey、Creem、Stripe；Turnstile 仅遗留兼容，见 §7）
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

## 4. 业务规则与状态

### 4.1 业务规则

- **Realm 隔离**：所有配置项属于 Realm 级别，不同 Realm 的配置相互独立
- **权限要求**：按权限判定而非角色硬编码——查看需 `settings.view`、修改需 `settings.manage`（Realm Admin 角色默认持有这些权限，被显式授予相应权限的委托管理员亦可操作）
- **敏感信息脱敏**：密码、密钥类字段（Resend API Key、SMTP 密码等）在展示时必须脱敏（读取时 is_secret=true 的 config_value 返回 null，仅在写入时接受明文），编辑时才暴露为输入框
- **邮件配置完整性定义**：provider + from_address + 对应 provider 的必填字段均已填写（`is_email_configured` 不检查 enabled 标志，仅检查字段非空——即使用户禁用了某个邮件配置项，只要字段非空仍视为已配置）
- **功能开关前置验证**：`require_email_verification` 开关仅在邮件配置完整时可开启；未配置邮件时，该开关显示为禁用状态，提示 "Email verification requires email configuration"
- **OAuth 配置独立**：OAuth Provider 有独立配置系统，不在 Realm Config 中管理
- **Registration 配置**：管理用户注册策略，包括是否开放注册（enabled）、是否需要邮箱验证（require_email_verification）、允许的邮箱域名（allowed_domains）、是否拦截一次性邮箱域名（block_disposable_domains）。`allowed_domains` 为空时不限制；非空时按标准化后的邮箱域名做不区分大小写的精确匹配，不把子域自动视为父域命中。`block_disposable_domains` 默认关闭（缺省行或非 true 值均视为关），开启后按与 `allowed_domains` 相同的精确匹配语义比对后端内置的一次性邮箱域名列表（CC0 上游列表不入库，构建期由 api-base 的 build.rs 从钉定上游 commit 拉取并做 sha256 校验后编译进二进制，见 `backend/api-base/build.rs`）；该检查在 `allowed_domains` 之后独立执行，deny 优先——即使域名被 allowlist 显式允许，开启拦截时仍拒绝。两者均覆盖密码注册、Email OTP 自动注册及 OAuth/One Tap 等首次自动建号（各入口复用其现有域名拒绝响应，不引入新错误码），已存在账号的登录不受影响，且不经 public config 端点暴露
- **开启注册的默认协议拦截**：将 Registration 的 `enabled` 从关闭切换为开启时，若本 realm 的用户协议或隐私政策仍使用平台默认模板（未自定义），该次保存被拦截，须管理员确认后方可开启；仅拦截关闭→开启的转变，协议已自定义、或注册本就处于开启状态的保存不受影响
- **config_type 枚举强校验**：HTTP 写路径（单个 upsert、批量 batch_upsert、删除 delete）对未知 config_type 一律返回 400
- **审计要求**：关键配置变更应记录审计日志
- **测试邮件速率限制**：同一 realm + 用户组合限制 3 次 / 60 秒，超限返回 429

### 4.2 关键状态与异常

- **未配置邮件 + 尝试开启邮箱验证**：开关禁用，显示提示信息，阻止开启
- **Provider 切换**：切换邮件 Provider 时，隐藏/显示对应字段（Resend 显示 API Key；SMTP 显示 Host/Port/Username/Password）
- **测试邮件**：保存配置后可通过 "Send Test Email" 验证配置正确性，速率限制 3 次/60 秒
- **Registration 键名统一**：注册开关使用 `config_key = "enabled"`，创建 Realm、查询注册状态和 public config 均使用同一键名。

> **计划中功能**：以下功能在 PRD 中曾提及但当前代码无实现，移至未来扩展：
> - 密码策略配置（最小长度、大小写、数字、特殊字符要求）：`password_min_length`、`require_uppercase`、`require_lowercase`、`require_numbers`、`require_special_chars` 在 ConfigType::Registration 的代码注释中明确声明为 NOT consumed by any reader——密码规则由用户服务执行，不经该配置类型配置（前端密码强度规则为硬编码常量，不从 Realm Config 读取）；实际被读取的 Registration 键仅为 `enabled`、`require_email_verification`、`allowed_domains`、`block_disposable_domains`
> - `default_user_status`（Registration 配置中新用户默认状态，取值范围 0-3）：当前代码中无此字段，未来可能作为 Registration 类型的 config_key 新增

---

## 5. 验收目标

- Realm Admin 能通过 Settings 页面成功配置 Registration、Email 各项参数
- 邮件服务配置保存后，可通过测试邮件功能验证配置正确性
- 未配置邮件时，邮箱验证开关处于禁用状态并有明确提示
- 敏感字段在页面展示时脱敏，仅在编辑时可见
- 不同 Realm 的配置相互隔离
- 支持批量更新配置（batch_upsert）和删除单个配置项
- 测试邮件受速率限制保护

---

## 6. 边界与约束

**适用性**: 适用（API 与前端/交互边界合并陈述）

**API / 集成边界:**

- 接口能力范围：Realm Config 的查询、单个 Upsert、批量 Upsert（batch_upsert）、删除（delete），涵盖 registration、email、totp、totp_key、passkey、white_label、custom_domain、ldap、email_otp、platform_signup、stripe、creem、apple、google、wechat、invoice_policy、turnstile 配置类型（以 ConfigType 枚举为准），以及 OAuth Provider 的独立配置管理。`turnstile` 配置类型仅保留遗留兼容，不再承载有效配置（见 §7）。**例外类型两种：`custom_domain` 与 `white_label`——通用 configs API 对这两类仅放行查询，全部写路径（单个/批量 Upsert、删除）一律 400 拒绝**：`custom_domain` 的写会绕过专用 custom-domain 端点对 `custom_domain_mapping` 耦合表的同步维护（见 realm-custom-domain.md §2.3）；`white_label` 的写会绕过专用白标端点对品牌值（CSS 注入、URL 加载等）的校验直接把未校验值发布给第三方登录 UI，且通用 DELETE 可清除已发布品牌配置或 `previous_settings` 恢复快照、绕过草稿/发布/恢复生命周期（见 ui-custom.md §6）。因此这两类行的写与删必须走各自的专用端点。**第三类写拒绝：`platform_signup` 仅对 admin realm 有效**——自助开通开关只从 admin realm 读取（见 realm-create PRD），非 admin realm 经通用 configs API 对该类型的写与删一律 400 拒绝，避免产生无人读取的脏数据。此外，TOTP、Passkey、Email OTP 三类认证方式配置除通用 configs API 外，另有各自的专用配置端点承载读取与更新（更新操作记录审计日志）
- 详细接口契约、验证规则和错误模型在技术设计文档中维护

**前端 / 交互边界:**

- **页面入口**：管理后台左侧导航栏 Settings 菜单项，realmId 从 UI 上下文获取
- **页面布局**：多 Tab 布局，每个配置类型对应一个 Tab（Registration、OAuth、Email 等；Turnstile 不在此页面，见 §7）
- **每个 Tab 包含**：配置标题、启用/禁用开关、配置项表单、保存/重置按钮
- **敏感字段交互**：密码/密钥类字段展示脱敏占位符，点击编辑后变为输入框
- **功能开关联动**：未配置邮件时，Registration Tab 中邮箱验证开关显示为禁用状态，并提示原因
- **开启注册的默认协议提醒**：触发 §4.1「开启注册的默认协议拦截」时，弹出可确认的提醒对话框——「前往协议设置」跳转协议配置 Tab 并放弃本次未保存的开关状态；「仍然开启」确认后继续执行保存；直接关闭对话框同样放弃本次保存
- **操作反馈**：保存成功/失败有明确反馈，测试邮件发送有结果反馈

---

## 7. 已确认决策

- OAuth 配置使用独立系统管理，不纳入 Realm Config 存储结构
- 邮件服务配置纳入 Realm Config 管理，使用 `email` 配置类型
- Settings 页面使用多 Tab 布局而非分组卡片布局
- 支付提供商配置（Creem/Stripe）已迁移到独立的 Payment Providers 页面管理，不再通过 Realm Config 管理。Realm Config 中的 creem/stripe 配置类型仅用于遗留兼容，新功能应在 Payment Providers 页面操作
- 人机验证（Turnstile）配置已下放到 Client App 级别（每个 Client App 配置自己的 Turnstile site_key/secret_key 与启用开关），不再作为 Realm 级配置管理。Realm Config 中的 `turnstile` 配置类型仅保留遗留兼容，新的人机验证配置入口在 Client App（见 [docs/prd/integration/client-app.md](../integration/client-app.md)）

---

## 8. 参考资料

- 相关 PRD：`docs/prd/core/realm.md`
- 相关 PRD：`docs/prd/core/users.md`
- 相关 PRD：`docs/prd/integration/client-app.md`
- 用户故事来源见 §1 表格
