# White-label（Per-Realm 登录/注册及 Auth 流 UI 定制）产品需求文档 (PRD)

**创建时间**: 2026-07-08
**优先级**: P1

---

## 1. 相关用户故事

> 详细故事与验收标准请查看 `docs/user-stories/core/white-label.md`。

### 1.1 相关故事

- `[US-WL-001]` 配置 Realm 品牌资产 (P0)，来源 `docs/user-stories/core/white-label.md`
  - 角色：Realm Admin
  - 摘要：在管理后台配置本 Realm 的 logo、主色、背景、页脚文案、登录/注册页标题与副标题文案，配置仅作用于本 Realm

- `[US-WL-002]` 终端用户看到品牌化 auth 流页面 (P0)，来源 `docs/user-stories/core/white-label.md`
  - 角色：Regular User
  - 摘要：在 realm 的登录、注册及其他 auth 流页面及子状态间流转时，始终看到该 realm 配置的品牌呈现

- `[US-WL-003]` 主色对比度安全提示 (P1)，来源 `docs/user-stories/core/white-label.md`
  - 角色：Realm Admin
  - 摘要：配置主色时，若对比度低于 WCAG AA 标准，系统仅警告不拦截保存

- `[US-WL-004]` 资产 URL 引用与租户自备图床 (P1)，来源 `docs/user-stories/core/white-label.md`
  - 角色：Realm Admin
  - 摘要：logo 与背景通过图片 URL 引用配置，无需 Herald 提供上传存储

### 1.2 优先级汇总

| 优先级 | 数量 | 关键故事 |
|--------|------|----------|
| P0 | 2 | 配置品牌资产、终端用户看到品牌化页面 |
| P1 | 2 | 主色对比度提示、资产 URL 引用 |
| P2 | 0 | - |

---

## 2. 范围界定

### 2.1 包含功能

- Per-realm logo（图片 URL，展示在 auth 流页面头部，替代默认 "Herald" 文字）
- Per-realm 品牌名称（brand_name）
- Per-realm 站点图标（favicon_url，与 logo 同样为 http(s) URL 引用）
- Per-realm 主色 / accent color（影响主按钮、链接等品牌色）
- Per-realm 背景（图片 URL 或背景渐变，替代当前固定背景）
- Per-realm 页脚文案（替代当前固定品牌信息）
- Per-realm 登录/注册页文案（标题 / 副标题，复用已有 realmName / realmDescription 扩展）
- 品牌资产配置的管理后台入口（Settings 页面新增品牌化 Tab）
- 品牌化呈现覆盖所有经统一布局出口的 auth 流页面及其子状态（登录、注册、忘记密码、邮箱验证、OAuth 同意、TOTP、passkey 等）
- logo / 背景加载失败时的默认回退
- 主色对比度的配置端提示（仅警告不拦截）
- 草稿预览、发布生效和恢复上一版的最小闭环，避免错误品牌配置立即影响终端用户或无法快速回退

### 2.2 不包含功能 (Out of Scope)

- 自定义 HTML / CSS 模板（Full Custom，XSS / CSP 风险高、需审核体系）
- 自定义子域 / 自定义域（custom domain，属 Auth0 Branded 级，超出 Standard White-label）
- 资产审核 / 内容安全审核体系
- 多 locale 文案管理（i18n 已有独立 PRD，不在本 feature 扩 scope）
- 资产上传存储（本期 logo / 背景仅 URL 引用；上传为 Possible Expansion）
- 管理后台之外的非 auth 页面品牌化（如 legal 页、用户中心等，本期不纳入）
- 暗色模式下的品牌色适配（本期仅 light 模式）

### 2.3 依赖项

- **Realm 系统** — 品牌资产属 Realm 级别，依赖 Realm 基础设施与 realm 隔离
- **权限管理系统** — 品牌资产配置要求 `settings.manage` 权限（与现有 Realm Config 一致）
- **Realm 设置能力** — 复用现有 Realm 级配置能力，保证品牌资产按 Realm 隔离管理
- **Public Config 通道** — 复用现有按 realm 读取的 public 配置通道，扩展返回品牌资产字段
- **统一 Auth 流页面布局出口** — 所有 auth 流页面经同一布局出口，品牌资产注入集中于此

---

## 3. 需求概述

### 3.1 功能描述

让 Herald 作为多租户认证产品具备 white-label 能力：Realm Admin 可在管理后台配置本 Realm 的品牌资产（logo / 主色 / 背景 / 页脚文案 / 登录注册文案），使本 Realm 终端用户在该 Realm 的登录、注册及其他 auth 流页面看到属于该租户品牌的呈现，而非默认的 Herald 品牌。配置按 Realm 隔离，未配置项回退默认 Herald 呈现。

本能力动机为**产品完整度**（让 Herald 作为多租户认证产品具备行业基线的 white-label 能力，匹配 Auth0 / Clerk / WorkOS 基础体验），而非被 demand 驱动；因此成功标准以「能力完整」衡量，不以转化提升衡量。

### 3.2 关键特性

- Per-realm 品牌资产配置（logo / 主色 / 背景 / 页脚 / 登录注册文案）
- 品牌化覆盖所有 auth 流页面及其子状态
- 资产以 URL 引用方式配置（租户自备图床）
- logo / 背景加载失败时回退默认 Herald 呈现
- 主色对比度仅警告不拦截（保留品牌色决策权）
- 配置按 Realm 隔离，仅 Realm Admin 可配置

---

## 4. 业务规则与状态

### 4.1 业务规则

- **Realm 隔离**：品牌资产属于 Realm 级别，不同 Realm 的配置相互独立；一个 Realm 的品牌配置不会影响其他 Realm
- **权限要求**：仅 Realm Admin 可管理品牌资产配置——读取需 `settings.view`，写入（草稿/发布/恢复）需 `settings.manage`；Regular User 仅作为配置的消费方在 auth 流页面看到品牌化呈现，无配置入口
- **配置回退**：任一品牌资产字段未配置时，该字段在终端用户页面回退到默认 Herald 呈现（如无 logo 显示默认 "Herald" 文字、无背景使用默认背景、无主色使用默认主题色）；其余已配置字段仍按 realm 配置呈现
- **生效模型**：Realm Admin 在配置端保存草稿时，仅管理端预览可见，不影响终端用户 auth 流页面；点击发布后，新的品牌配置才通过 public 配置通道对终端用户生效
- **恢复上一版**：系统至少保留最近一次发布前的品牌配置；Realm Admin 发布错误配置后，可恢复上一版，使终端用户 auth 流页面回到最近一次发布前的品牌呈现
- **资产 URL 引用**：logo / 背景以图片 URL（或背景渐变描述）方式配置，租户需自备图床；Herald 本期不提供资产上传存储
- **对比度策略「仅警告不拦截」**：Realm Admin 配置主色时，若该色导致按钮文字对比度低于 WCAG 1.4.3 AA（≥4.5:1），系统在配置端显示警告但允许保存；渲染端不做二次拦截
- **本期仅 light 模式**：品牌资产字段仅作用于 light 主题，本期不提供暗色模式下的独立品牌值
- **品牌化覆盖所有 auth 流页面**：品牌资产注入经统一 auth 流页面布局出口，覆盖登录、注册、忘记密码、邮箱验证、OAuth 同意、TOTP、passkey 等所有 auth 流页面及其子状态

### 4.2 关键状态与异常

- **logo 加载失败**：logo URL 无法加载（图片被删除或链接失效）时，logo 区域不显示破损图标，回退显示默认 "Herald" 文字；其余品牌资产不受影响
- **背景加载失败**：背景 URL 无法加载时，背景回退为默认呈现，不显示破损样式；其余品牌资产不受影响
- **品牌配置数据解析失败**：后端读取品牌配置数据失败（如配置值非法）时，回退默认 Herald 呈现，不影响 auth 流页面正常可用
- **草稿未发布**：存在未发布草稿时，终端用户 auth 流页面继续显示当前已发布配置；管理端预览显示草稿或当前编辑值
- **放弃草稿**：Realm Admin 放弃草稿后，管理端编辑器恢复到当前已发布配置；终端用户页面不发生变化
- **跨子状态一致性**：终端用户在 auth 流页面及其各子状态（如登录页主表单 / consent / TOTP / passkey-2FA 等）间流转时，品牌资产不会丢失或回退为默认
- **管理后台之外页面不受影响**：本期品牌资产仅作用于 auth 流页面，legal 页、用户中心等非 auth 页面不做品牌化（即使技术上共享部分样式）

---

## 5. 功能需求

### 5.1 核心需求

- **品牌资产配置**：Realm Admin 可配置本 Realm 的品牌名称（brand_name）、logo（图片 URL）、站点图标（favicon_url，http(s) URL 引用）、主色（accent color）、背景（图片 URL 或背景渐变）、页脚文案、登录/注册页标题与副标题文案
- **草稿与发布**：Realm Admin 可保存草稿、实时预览草稿、放弃草稿，并在确认后发布配置；只有已发布配置会影响终端用户 auth 流页面
- **错误配置恢复**：Realm Admin 可将已发布配置恢复到上一版，降低 logo、背景、颜色或文案配置错误后的修复成本
- **配置入口**：管理后台 Settings 页面新增品牌化配置入口（与现有 TOTP / Passkey / Registration 等配置 Tab 同级）
- **配置下发与渲染**：品牌资产经按 realm 读取的 public 配置通道下发，终端用户在 auth 流页面看到对应品牌呈现
- **覆盖所有 auth 流页面与子状态**：品牌化覆盖登录、注册、忘记密码、邮箱验证、OAuth 同意、TOTP、passkey 等所有 auth 流页面及其子状态
- **默认回退**：logo / 背景 / 主色 / 文案等未配置或加载失败时回退默认 Herald 呈现
- **主色对比度提示**：配置端对低于 WCAG AA 标准的主色显示警告，但不拦截保存
- **配置隔离与权限**：品牌资产按 Realm 隔离，仅 Realm Admin 可配置

### 5.2 验收目标

- Realm Admin 能通过管理后台 Settings 页面成功编辑并保存本 Realm 的 logo / 主色 / 背景 / 页脚文案 / 登录注册页文案草稿
- 草稿保存后，终端用户在该 Realm 的登录、注册及其他 auth 流页面及各子状态仍看到当前已发布品牌配置，不会提前看到草稿
- 配置发布后，终端用户在该 Realm 的登录、注册及其他 auth 流页面及各子状态看到对应品牌化呈现（logo / 主色 / 背景 / 页脚 / 标题副标题文案）
- 发布错误配置后，Realm Admin 能恢复上一版，终端用户 auth 流页面回到上一版品牌呈现
- 任一品牌资产字段未配置时，该字段回退默认 Herald 呈现，其余字段仍按 realm 配置呈现
- logo / 背景加载失败时回退默认呈现，不显示破损样式，不影响其余品牌资产与页面可用性
- Realm Admin 配置主色时，对比度不达标有警告提示但仍可保存
- 不同 Realm 的品牌资产配置相互隔离
- 仅 Realm Admin 可见和操作品牌化配置入口

---

## 6. API 相关约束

**适用性**: 适用

- **接口能力范围**：品牌资产配置的管理端草稿保存、放弃草稿、发布、恢复上一版（Realm Admin）+ 公共配置读取（终端用户在 auth 流页面加载已发布品牌资产），沿用现有 Realm Config 管理端模式与现有按 realm 读取的 public 配置通道
- **访问控制原则**：管理端读取要求 `settings.view`、写入（草稿/发布/恢复）要求 `settings.manage`，均通过 Realm 归属校验；公共配置读取为无认证读取（与现有 public 配置通道一致，供未登录终端用户在 auth 流页面加载品牌资产）
- **数据边界原则**：品牌资产配置按 Realm 隔离，不同 Realm 之间不可交叉访问；公共配置读取仅返回指定 realm 的品牌资产
- **配置数据完整性**：后端读取品牌配置数据解析失败时回退默认 Herald 呈现，不影响 auth 流页面正常可用
- **不引入资产上传接口**：本期 logo / 背景仅 URL 引用，不提供资产上传接口（上传为 Possible Expansion）
- 详细接口契约与错误模型在技术设计文档中维护

---

## 7. 前端/交互约束

**适用性**: 适用

- **配置页入口**：管理后台 Settings 页面新增「品牌化」Tab（与现有 Turnstile / Registration / Email / TOTP / Passkey 等 Tab 同级），realmId 从 UI 上下文获取
- **配置表单**：包含 logo URL、主色（accent color，颜色选择）、背景（图片 URL 或背景渐变）、页脚文案、登录/注册页标题与副标题文案等字段，支持保存草稿、实时预览、放弃草稿、发布和恢复上一版
- **主色对比度反馈**：Admin 选择主色时实时计算对比度，低于 WCAG AA（≥4.5:1）显示警告文案但允许保存
- **终端用户呈现**：登录、注册、忘记密码、邮箱验证、OAuth 同意、TOTP、passkey 等所有 auth 流页面经统一布局出口呈现品牌资产（logo 在头部、主色作用于主按钮与链接、背景作用于页面、页脚文案在底部、标题副标题文案替换默认）
- **加载失败回退**：logo 加载失败回退默认 "Herald" 文字；背景加载失败回退默认背景；均不显示破损样式
- **跨子状态一致**：auth 流页面各子状态（主表单 / consent / TOTP / passkey-2FA 等）均呈现品牌资产，切换状态时品牌资产不丢失
- **角色差异**：仅 Realm Admin 可见和操作「品牌化」配置 Tab；Regular User 仅在 auth 流页面看到品牌化呈现，无配置入口
- **本期仅 light 模式**：品牌资产字段仅作用于 light 主题呈现

---

## 8. 已确认决策

### 8.1 来自 Decision Brief 的已确认决策（D0 / D1）

- **动机归类（D0）**：产品完整度（让 Herald 作为多租户认证产品具备完整 white-label 能力），非 B2B demand 驱动；成功标准以「能力完整」衡量，不以转化提升衡量
- **Scope 重量（D0）**：Standard White-label（非 Full Custom）；明确排除自定义 HTML / CSS / 自定义子域 / 资产审核体系
- **致命假设（D0）**：white-label 是 Herald 产品定位的应有之义（即 Herald 定位包含「提供可品牌化的托管前端认证页」）；若产品定位收敛为 API-only / backend-only 则本能力 Park（Kill Criteria，见 Decision Brief §5）
- **配置能力复用（D1）**：复用现有 Realm 级设置能力承载品牌资产，不为本能力引入独立的资产存储体系
- **前端驱动方式（D1）**：扩展按 realm 读取的 public 配置通道返回品牌资产字段，前端 auth 流页面经统一布局出口按字段渲染
- **资产引用方式（D1）**：logo / 背景用 URL 引用，不做上传存储；若上线后租户普遍无图床则评估上传（Possible Expansion）

### 8.2 PRD 阶段新增的已确认决策

- **品牌化页面范围扩展到所有 auth 流页面**：品牌化覆盖所有经统一 auth 流页面布局出口的页面及其子状态（登录、注册、忘记密码、邮箱验证、OAuth 同意、TOTP、passkey 等）。
  - **与 Decision Brief §6 的差异**：Decision Brief §6 In Scope 仅明确「仅登录/注册页」，将「忘记密码 / 邮箱验证 / OAuth 同意等其他 auth 流页面的品牌化」列为 Possible Expansions 留 PRD 评估。本 PRD 评估结论为扩展到所有 auth 流页面，**这是相对 Decision Brief 的明确 scope 扩张**，依据为这些页面技术上一共用统一布局出口、扩展成本近乎零，且同属面向终端用户的信任入口。
  - **记录方式（Rule 7）**：作为显式 scope 扩张记录，不与 Decision Brief 的 In Scope 静默合并；Decision Brief §6 的「Possible Expansions」中相关项应视为已被本 PRD 吸收。
- **对比度策略「仅警告不拦截」**：Realm Admin 配置主色时，对比度低于 WCAG 1.4.3 AA 显示警告但不拦截保存；渲染端不做二次拦截
- **本期仅 light 模式**：品牌资产字段仅作用于 light 主题，本期不接入暗色模式、不提供暗色模式下的独立品牌值
- **管理后台之外页面不纳入**：legal 页、用户中心等非 auth 页面本期不做品牌化（即使技术上共享部分样式），与 Decision Brief §6 一致
- **草稿/发布/恢复模型**：配置端保存草稿不对终端用户生效；发布后才更新 public 配置；至少保留上一版用于快速回退。这是对“保存配置”验收语义的细化，避免错误配置立即影响终端用户且无法恢复。

### 8.3 已知限制（沿用既有系统）

- **外部资产风险**：logo / 背景 URL 由租户自备；资产审核体系明确 Not in Scope，后续若引入更严格的资源加载策略，需保持本 PRD 定义的外部图片引用能力

---

## 9. 参考资料

- 用户故事：`docs/user-stories/core/white-label.md`
- 相关 PRD：`docs/prd/core/realm-settings.md`（Realm Config 配置模式基线）
- 相关 PRD：`docs/prd/core/realm.md`
- 角色定义：`docs/user-stories/_roles.md`
