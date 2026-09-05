# SaaS 自助注册开通 Realm 产品需求文档 (PRD)

**创建时间**: 2026-08-09
**优先级**: P0
**所属域**: core

---

## 1. 相关用户故事

> 详细故事与验收标准请查看 `docs/user-stories/core/realm-create.md`。

### 1.1 相关故事

- `[US-SR-001]` 自助注册开通新 Realm，优先级 P0，来源 `docs/user-stories/core/realm-create.md`
  - 角色：SaaS 自助注册访客
  - 摘要：在公开注册页面填写信息即可开通一个新 realm，成为该 realm 的 realm-admin
- `[US-SR-002]` 开通后立即管理新 Realm，优先级 P0，来源 `docs/user-stories/core/realm-create.md`
  - 角色：新 realm-admin（注册成功后的访客）
  - 摘要：注册成功后立即登录并进入新 realm 的管理界面，无需等待或人工审核
- `[US-SR-003]` Admin Realm 管理员查看自助开通的 Realm，优先级 P1，来源 `docs/user-stories/core/realm-create.md`
  - 角色：Admin Realm 管理员
  - 摘要：自助开通的 realm 出现在既有 Realm 管理列表中，与手动创建的一致
- `[US-SR-004]` 平台自助开通开关控制，优先级 P0，来源 `docs/user-stories/core/realm-create.md`
  - 角色：Admin Realm 管理员
  - 摘要：开启或关闭平台的公共自助开通入口

### 1.2 优先级汇总

| 优先级 | 数量 | 关键故事 |
|--------|------|----------|
| P0 | 3 | 自助注册开通新 Realm、开通后立即管理新 Realm、平台自助开通开关控制 |
| P1 | 1 | Admin Realm 管理员查看自助开通的 Realm |
| P2 | 0 | - |

---

## 2. 范围界定

### 2.1 包含功能

- 由 admin realm 托管、对未登录访客公开访问的**自助注册开通页面**
- 访客提交注册信息后，系统**立即开通一个新 realm**（注册即开通）
- 注册者自动成为所开通 realm 的 **realm-admin**，并立即获得该 realm 的会话进入其管理界面
- 复用既有 realm 初始化机制（默认 RBAC、`admin-web-console`、`admin-api-client`、`registration.enabled=false`、Normal 状态管理员）
- 自助开通的 realm 在 Admin Realm 的 Realm 列表中与手动创建的 realm **一致呈现**
- **平台自助开通开关**（Admin Realm 管理员开启 / 关闭；默认值属运营决策，见 Q-realm-create-003）
- **基础防滥用**：同一 IP 每 24 小时最多自助注册 2 个 realm（DEC-realm-create-007）；Cloudflare Turnstile 人机验证，按绑定 Client App 的 Turnstile 配置强制（DEC-realm-create-008）

### 2.2 不包含功能 (Out of Scope)

- 注册环节的套餐选择、计费或支付（付费由既有 billing 系统后续承载，见 DEC-realm-create-002；免费层配额与试用模型见 Q-realm-create-002）
- 平台级“单账号拥有多个 realm”能力（本 PRD 限定一次注册对应一个新 realm，见 DEC-realm-create-004）
- Realm 删除（沿用既有约束：不支持 realm 删除）
- 是否在 IP 限额 / Turnstile 之外**额外**强制“邮箱验证后才允许访问新 realm”（增强防滥用，延期至技术设计，见 Q-realm-create-004）
- 自助开通后新 realm 内部的业务配置（由该 realm 的 realm-admin 在 realm-settings 中完成）

### 2.3 依赖项

- **Realm 创建与初始化系统** — 提供 realm 生命周期与自动初始化（见 `docs/prd/core/realm.md`）
- **用户认证系统** — 提供注册后即时的会话建立与 realm 隔离登录
- **权限管理系统** — 提供注册者成为 realm-admin 所需的角色分配与 realm 隔离校验
- **Admin Realm** — 承载平台级自助开通入口（DEC-realm-create-001）

---

## 3. 需求概述

### 3.1 功能描述

Herald 当前支持已登录的 Admin Realm 管理员在管理后台手动创建 realm（见 `docs/prd/core/realm.md`、`US-AR-001`）。本 PRD 描述面向 SaaS 场景的**自助注册开通**能力：未登录的访客可通过 admin realm 托管的公共注册页面提交信息，系统立即为其开通一个新的、与之绑定的 realm，访客成为该 realm 的 realm-admin 并可立即开始管理。该能力使第三方组织无需联系平台管理员即可获得独立的认证与授权租户，是 admin realm 独有的平台级入口（DEC-realm-create-001）。

本 PRD 与 `docs/prd/core/realm.md` **互补**：后者描述已登录管理员的“手动内部开通”，本 PRD 描述未登录访客的“自助公开开通”。两者各自独立交付，不覆盖、不替换对方的开通能力；新 realm 的初始化规则统一沿用 `docs/prd/core/realm.md`（DEC-realm-create-003）。

### 3.2 关键特性

- 公共、未登录可达的注册开通页面（仅 admin realm 托管）
- 注册即开通：提交通过校验后立即开通新 realm，无人工审核或等待
- 注册者自动成为新 realm 的 realm-admin 并立即获得会话进入管理界面
- 复用既有 realm 初始化机制，不引入并行开通路径
- 新 realm 与其他 realm 严格隔离，注册者仅可访问其开通的 realm
- 平台自助开通开关由 Admin Realm 管理员控制
- 基础防滥用：同一 IP 24 小时限额与 Cloudflare Turnstile 人机验证（按绑定 Client App 配置）

---

## 4. 业务规则与状态

### 4.1 业务规则

**入口与隔离**

- 自助开通注册页面由 **admin realm 托管**，对未登录访客公开访问；该能力为 **admin realm 独有**，其他 realm 不承载平台级开通入口（DEC-realm-create-001）。
- 一次注册对应**一个新 realm**；注册者即该 realm 的 realm-admin。平台级“单账号拥有多个 realm”不在本 PRD 范围（DEC-realm-create-004）。

**开通模型**

- **注册即开通**：访客提交通过校验的注册信息后，系统立即开通新 realm；注册环节不引入套餐选择或支付（DEC-realm-create-002）。
- 开通时**复用既有 realm 初始化机制**（默认 RBAC 角色/权限/策略、`admin-web-console`、`admin-api-client`、`registration.enabled=false`、Normal 状态管理员），不新建并行路径（DEC-realm-create-003）。

**Realm 标识规则**

- 沿用既有规则（见 `docs/prd/core/realm.md` §4.1）：Realm ID 仅字母数字、连字符、下划线，3–36 字符，全局唯一，禁止保留词；创建后不可修改；可不指定（由系统生成）。

**权限要求**

- **自助注册**：无需任何平台权限，公开端点在开关开启时可访问；后端不根据请求是否同时携带既有会话改变该端点行为，前端只向访客展示入口。
- **访问新 realm**：注册者开通后仅可访问其新 realm，访问其他 realm 资源被拒绝（沿用 `docs/prd/core/realm.md` 的 realm 隔离原则）。
- **平台开关控制**：需要 Admin Realm 管理员对本 Realm 的设置管理权限（`settings.manage`）。平台开关作为 admin realm 的 `realm_config` 行承载，开关的查询与更新复用既有 Realm Settings 配置管理端点（见 `.ai/design/realm-create.md` §4.2.1/§4.5）；该端点按 `settings.manage` 授权。仅 admin realm 持有该开关配置行，因此实际可操作者仍限于 Admin Realm 管理员。

**平台开关**

- Admin Realm 管理员可开启或关闭平台自助开通入口；关闭后访客无法完成自助注册（US-SR-004）。该开关为本 PRD 必备能力（DEC-realm-create-009）。开关默认值属运营决策（Q-realm-create-003），读取时若配置缺失按关闭处理（fail-closed，避免误开放）。

**防滥用**

- **IP 注册限额**：同一 IP 每 24 小时最多自助注册开通 2 个 realm；超出后注册被拒绝并提示限额（DEC-realm-create-007）。IP 识别方式、计数窗口实现与失败计数行为下沉技术设计。
- **人机验证（Turnstile）**：自助注册页面启用 Cloudflare Turnstile，按绑定自助注册页面的 Client App 的 Turnstile 配置强制——配置为启用时必须通过人机验证，未启用时不强制（DEC-realm-create-008）。Turnstile 配置归属 Client App 级（见 `docs/prd/integration/client-app.md` §4.1 D-PROTECT-01），非 realm 级独立开关。
- **管理员账号验证行为**：新 realm 管理员账号沿用既有“创建即 Normal（已验证）”行为（DEC-realm-create-006）；是否额外强制“邮箱验证后才允许访问新 realm”属增强防滥用，不在本 PRD 强制范围（Q-realm-create-004）。

### 4.2 关键状态与异常

- **开关关闭时访问注册页面**：注册入口不可用或被明确拒绝，并向访客提示自助开通当前不可用。
- **校验失败**：注册信息不满足校验（邮箱格式、密码强度、realm 名称缺失等）时显示明确校验错误，不创建任何 realm。
- **标识冲突**：realm 标识被占用或为保留词时显示冲突提示，引导更换标识。
- **初始化失败**：沿用既有约束（见 `docs/prd/core/realm.md` §4.1）——若初始化任一步骤失败，开通失败并返回错误，已创建的部分数据可能残留（realm 不支持删除）。
- **数据隔离**：新 realm 与其他 realm 严格隔离；注册者访问其他 realm 资源时被拒绝。
- **防滥用触发**：同一 IP 24 小时内已开通 2 个 realm 时，再次注册被拒绝并提示限额（DEC-realm-create-007）；绑定 Client App 的 Turnstile 为启用时，未通过人机验证的注册被拒绝（DEC-realm-create-008）。
- **防滥用实现细节**：IP 识别方式（如可信代理头处理）、计数窗口实现、失败计数与限额计数器归属（仅计成功开通 or 计所有尝试）下沉技术设计；新 realm 管理员账号沿用“创建即 Normal（已验证）”行为（DEC-realm-create-006）。

---

## 5. 功能需求

### 5.1 核心需求

- **公共注册开通页面**：admin realm 托管、未登录访客可达的注册页面，收集开通所需信息（realm 名称、访客邮箱、访客密码；realm 标识可选，留空由系统生成）。
- **注册即开通**：提交通过校验后立即开通新 realm，完成既有初始化，并将注册者设为该 realm 的 realm-admin。
- **开通后即进入**：开通成功后注册者立即获得新 realm 的会话并进入其管理控制台。
- **Realm 一致呈现**：自助开通的 realm 在 Admin Realm 的 Realm 列表中与手动创建的 realm 在可见字段上一致呈现（US-SR-003）。
- **平台开关**：Admin Realm 管理员可开启 / 关闭平台自助开通入口（US-SR-004）。
- **IP 注册限额**：同一 IP 每 24 小时最多自助注册 2 个 realm，超出后拒绝注册并提示限额（DEC-realm-create-007）。
- **人机验证**：自助注册页面按绑定 Client App 的 Turnstile 配置强制 Cloudflare 人机验证（DEC-realm-create-008）。

### 5.2 验收目标

- 未登录访客可在 admin realm 托管的公开注册页面完成注册并开通一个新 realm，注册者成为该 realm 的 realm-admin（US-SR-001）。
- 注册成功后注册者立即进入新 realm 的管理控制台，无需额外审核或等待；注册者访问其他 realm 资源时被拒绝（US-SR-002）。
- 自助开通的 realm 出现在 Admin Realm 的 Realm 列表中，与手动创建的 realm 在可见字段上一致（US-SR-003）。
- Admin Realm 管理员关闭平台开关后访客无法完成自助注册；重新开启后可正常注册（US-SR-004）。
- 同一 IP 24 小时内第 3 次自助注册被拒绝并提示限额；前 2 次正常开通（DEC-realm-create-007）。
- 当绑定自助注册页面的 Client App 的 Turnstile 已启用时，未通过人机验证的注册被拒绝；未启用时不强制（DEC-realm-create-008）。
- 校验失败（邮箱、密码强度、名称缺失）与标识冲突（占用、保留词）时显示明确提示，且不创建任何 realm。
- 开通失败时按既有约束返回错误（部分数据可能残留，realm 不支持删除）。

---

## 6. API 相关约束

**适用性**: 适用

- **接口能力范围**：自助开通注册接口供未登录访客调用，完成注册信息校验与 realm 开通；平台开关的查询与更新接口供 Admin Realm 管理员操作；另有公开只读状态端点 `GET /api/auth/admin/signup/status`，未登录访客可查询自助开通开关是否开放，供注册页判断入口可用性，仅返回布尔值。新 realm 的初始化沿用既有 realm 创建能力，不在本 PRD 中重复定义接口契约。
- **访问控制原则**：自助注册接口是无需凭证的公开端点且平台开关必须开启；若请求额外携带既有会话，后端不据此拒绝或复用该身份。平台开关接口复用既有 Realm Settings 配置管理端点，按 `settings.manage` 授权（见 §4.1）；signup/status 状态端点为公开只读，无需登录，仅 admin realm 承载（其余 realm 请求返回未找到）。
- **租户 / realm 数据边界**：开通的新 realm 与其他 realm 严格隔离；注册者仅被授权其开通的 realm，访问其他 realm 资源被拒绝。
- **安全性**：注册信息（含密码）的传输与存储遵循既有安全要求；实施同一 IP 24 小时 2 个的注册限额（DEC-realm-create-007），并按绑定 Client App 的 Turnstile 配置强制人机验证（DEC-realm-create-008）；IP 识别、计数实现等细节下沉技术设计。
- 详细接口契约、校验规则与错误模型应在技术设计文档中维护。

---

## 7. 前端/交互约束

**适用性**: 适用

- **页面入口**：admin realm 托管的公共注册开通页面，未登录访客通过平台对外入口访问；非 admin realm 不承载此入口。
- **注册表单交互**：填写 realm 名称（必填）、访客邮箱（必填）、访客密码（必填）、realm 标识（可选，留空由系统生成）；提交前进行前端校验；当绑定自助注册页面的 Client App 的 Turnstile 为启用时，表单内嵌 Cloudflare Turnstile 人机验证组件，未通过验证不可提交（DEC-realm-create-008）。
- **限额提示**：同一 IP 24 小时内达到 2 个开通上限后，再次提交注册被拒绝并显示明确的限额提示（DEC-realm-create-007）。
- **成功反馈**：开通成功后自动将注册者带入新 realm 的管理控制台首页，无需额外登录步骤。
- **失败反馈**：校验失败与标识冲突时显示明确错误提示，并保留已填信息以便修正。
- **开关关闭时的反馈**：平台开关关闭时，注册入口不可用或被明确拒绝，并向访客提示自助开通当前不可用。
- **平台开关入口**：Admin Realm 管理员在管理后台可见平台自助开通开关控制（具体页面位置在技术设计中确定）。
- **关键状态反馈**：开通失败时显示明确错误，并说明部分数据可能残留的已知限制。

---

## 8. 已确认决策

| Decision ID | 状态 | 决策项 | 结论 | PRD 落点 | 来源 |
|---|---|---|---|---|---|
| `DEC-realm-create-001` | Applied | 入口归属与排他性 | 自助开通注册页面由 admin realm 托管并对未登录访客公开访问；该能力为 admin realm 独有 | §2.1、§3.1、§4.1、§7 | `.ai/decision-log/realm-create.md` |
| `DEC-realm-create-002` | Applied | 开通模型 | 注册即开通，注册环节不引入套餐选择或支付；付费由既有 billing 系统后续承载 | §2.1、§2.2、§3.2、§4.1 | `.ai/decision-log/realm-create.md` |
| `DEC-realm-create-003` | Applied | 初始化复用 | 复用既有 realm 初始化机制，不新建并行开通路径 | §2.1、§3.2、§4.1 | `.ai/decision-log/realm-create.md` |
| `DEC-realm-create-004` | Applied | 单次注册范围 | 一次注册对应一个新 realm；平台级单账号多 realm 不在本 PRD 范围 | §2.2、§4.1 | `.ai/decision-log/realm-create.md` |
| `DEC-realm-create-005` | Applied | 用户故事新建 | 新建独立用户故事（actor：自助注册访客），不复用 `US-AR-001` | §1.1 | `.ai/decision-log/realm-create.md` |
| `DEC-realm-create-006` | Applied | 邮箱验证行为 | 新 realm 管理员账号沿用既有“创建即 Normal（已验证）”行为；是否额外强制邮箱验证后访问延期（Q-realm-create-004） | §4.1、§4.2 | `.ai/decision-log/realm-create.md` |
| `DEC-realm-create-007` | Applied | IP 注册限额 | 同一 IP 每 24 小时最多自助注册 2 个 realm，超出拒绝 | §2.1、§4.1、§4.2、§5.1、§5.2、§6、§7 | `.ai/decision-log/realm-create.md` |
| `DEC-realm-create-008` | Applied | Turnstile 人机验证 | 按绑定自助注册页面的 Client App 的 Turnstile 配置强制，未启用不强制 | §2.1、§4.1、§4.2、§5.1、§5.2、§6、§7 | `.ai/decision-log/realm-create.md` |
| `DEC-realm-create-009` | Applied | 平台开关必备 | 自助开通为平台开关，从 P1 提升为本 PRD 必备能力 | §1.2、§4.1、§5.1、§5.2 | `.ai/decision-log/realm-create.md` |

> 这里只记录带稳定 DEC ID 的已确认结论。本 PRD 不存在待用户回答的未决问题（`needs_user_answer=0`）；延期问题见决策账本 Deferred Questions。

---

## 9. 参考资料

- 用户故事：`docs/user-stories/core/realm-create.md`
- 既有 Realm 管理 PRD：`docs/prd/core/realm.md`（手动内部开通基线，本 PRD 与之互补，不覆盖其开通能力）
- 既有 Realm Settings PRD：`docs/prd/core/realm-settings.md`（平台开关以 realm_config 形式管理，遵循 realm-settings 的配置管理能力边界）
- Client App PRD：`docs/prd/integration/client-app.md`（Turnstile 配置归属 Client App 级，见 §4.1 D-PROTECT-01）
- 角色定义：`docs/user-stories/_roles.md`
- 决策账本：`.ai/decision-log/realm-create.md`
- 技术设计：`.ai/design/realm-create.md`
