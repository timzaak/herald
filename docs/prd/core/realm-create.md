# SaaS 自助注册开通 Realm 产品需求文档 (PRD)

**创建时间**: 2026-08-09
**优先级**: P0
**所属域**: core

---

## 1. 相关用户故事

> 详细故事与验收标准请查看 `docs/user-stories/core/realm-create.md`。

| US-ID | 标题 | 优先级 | 来源 |
|-------|------|--------|------|
| US-SR-001 | 自助注册开通新 Realm | P0 | `docs/user-stories/core/realm-create.md` |
| US-SR-002 | 开通后立即管理新 Realm | P0 | `docs/user-stories/core/realm-create.md` |
| US-SR-003 | Admin Realm 管理员查看自助开通的 Realm | P1 | `docs/user-stories/core/realm-create.md` |
| US-SR-004 | 平台自助开通开关控制 | P0 | `docs/user-stories/core/realm-create.md` |

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

- 注册环节的套餐选择、计费或支付（付费由既有 billing 系统后续承载，见 DEC-realm-create-002；不引入免费层配额或试用期）
- 平台级“单账号拥有多个 realm”能力（本 PRD 限定一次注册对应一个新 realm，见 DEC-realm-create-004）
- Realm 删除（沿用既有约束：不支持 realm 删除）
- 是否在 IP 限额 / Turnstile 之外**额外**强制“邮箱验证后才允许访问新 realm”（增强防滥用；不做门禁，沿用创建即 Normal，见 DEC-realm-create-006）
- 自助开通后新 realm 内部的业务配置（由该 realm 的 realm-admin 在 realm-settings 中完成）

### 2.3 依赖项

- **Realm 创建与初始化系统** — 提供 realm 生命周期与自动初始化（见 `docs/prd/core/realm.md`）
- **用户认证系统** — 提供注册后即时的会话建立与 realm 隔离登录
- **权限管理系统** — 提供注册者成为 realm-admin 所需的角色分配与 realm 隔离校验
- **Admin Realm** — 承载平台级自助开通入口（DEC-realm-create-001）

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
- **平台开关控制**：需要 Admin Realm 管理员对本 Realm 的设置管理权限（`settings.manage`）。平台开关作为 admin realm 的 `realm_config` 行承载，开关的查询与更新复用既有 Realm Settings 配置管理端点（见 `.ai/design/realm-create.md` §4.2.1/§4.5）；该端点读取按 `settings.view`、写入按 `settings.manage` 授权（与 realm-settings.md §4.1 的分工一致）。仅 admin realm 持有该开关配置行，因此实际可操作者仍限于 Admin Realm 管理员。非 admin realm 写入 `platform_signup` 类型配置行的请求被显式 400 拒绝（该开关仅由 signup 流程从 admin realm 读取，其他 realm 的同类型配置行只会是无效脏数据，因此在 API 边界拒绝）。

**平台开关**

- Admin Realm 管理员可开启或关闭平台自助开通入口；关闭后访客无法完成自助注册（US-SR-004）。该开关为本 PRD 必备能力（DEC-realm-create-009）。开关默认值属运营决策（Q-realm-create-003），读取时若配置缺失按关闭处理（fail-closed，避免误开放）。

**防滥用**

- **IP 注册限额**：同一 IP 每 24 小时最多自助注册开通 2 个 realm；超出后注册被拒绝并提示限额（DEC-realm-create-007）。IP 识别方式、计数窗口实现与失败计数行为下沉技术设计。
- **人机验证（Turnstile）**：自助注册页面启用 Cloudflare Turnstile，按绑定自助注册页面的 Client App 的 Turnstile 配置强制——配置为启用时必须通过人机验证，未启用时不强制（DEC-realm-create-008）。Turnstile 配置归属 Client App 级（见 `docs/prd/integration/client-app.md` §4.1 D-PROTECT-01），非 realm 级独立开关。
- **管理员账号验证行为**：新 realm 管理员账号沿用既有“创建即 Normal（已验证）”行为（DEC-realm-create-006）；是否额外强制“邮箱验证后才允许访问新 realm”属增强防滥用，不在本 PRD 强制范围。

### 4.2 关键状态与异常

- **开关关闭时访问注册页面**：注册入口不可用或被明确拒绝，并向访客提示自助开通当前不可用。
- **校验失败**：注册信息不满足校验（邮箱格式、密码强度、realm 名称缺失等）时显示明确校验错误，不创建任何 realm。
- **标识冲突**：realm 标识被占用或为保留词时显示冲突提示，引导更换标识。
- **初始化失败**：沿用既有约束（见 `docs/prd/core/realm.md` §4.1）——若初始化任一步骤失败，开通失败并返回错误，已创建的部分数据可能残留（realm 不支持删除）。
- **数据隔离**：新 realm 与其他 realm 严格隔离；注册者访问其他 realm 资源时被拒绝。
- **防滥用触发**：同一 IP 24 小时内已开通 2 个 realm 时，再次注册被拒绝并提示限额（DEC-realm-create-007）；绑定 Client App 的 Turnstile 为启用时，未通过人机验证的注册被拒绝（DEC-realm-create-008）。
- **防滥用实现细节**：IP 识别方式（如可信代理头处理）、计数窗口实现、失败计数与限额计数器归属（仅计成功开通 or 计所有尝试）下沉技术设计；新 realm 管理员账号沿用“创建即 Normal（已验证）”行为（DEC-realm-create-006）。

---

## 5. 验收目标

- 未登录访客可在 admin realm 托管的公开注册页面完成注册并开通一个新 realm，注册者成为该 realm 的 realm-admin（US-SR-001）。
- 注册成功后注册者立即进入新 realm 的管理控制台，无需额外审核或等待；注册者访问其他 realm 资源时被拒绝（US-SR-002）。
- 自助开通的 realm 出现在 Admin Realm 的 Realm 列表中，与手动创建的 realm 在可见字段上一致（US-SR-003）。
- Admin Realm 管理员关闭平台开关后访客无法完成自助注册；重新开启后可正常注册（US-SR-004）。
- 同一 IP 24 小时内第 3 次自助注册被拒绝并提示限额；前 2 次正常开通（DEC-realm-create-007）。
- 当绑定自助注册页面的 Client App 的 Turnstile 已启用时，未通过人机验证的注册被拒绝；未启用时不强制（DEC-realm-create-008）。
- 校验失败（邮箱、密码强度、名称缺失）与标识冲突（占用、保留词）时显示明确提示，且不创建任何 realm。
- 开通失败时按既有约束返回错误（部分数据可能残留，realm 不支持删除）。

---

## 6. 边界与约束

**适用性**: 适用（API 与前端/交互边界合并陈述）

**API / 集成边界:**

- **接口能力范围**：自助开通注册接口供未登录访客调用，完成注册信息校验与 realm 开通；平台开关的查询与更新接口供 Admin Realm 管理员操作；另有公开只读状态端点 `GET /api/auth/{realmId}/signup/status`（`{realmId}` 必须为 admin realm，其余 realm 返回 404，即实际地址为 `/api/auth/admin/signup/status`），未登录访客可查询自助开通开关是否开放，供注册页判断入口可用性，仅返回布尔值。新 realm 的初始化沿用既有 realm 创建能力，不在本 PRD 中重复定义接口契约。
- 注册信息（含密码）的传输与存储遵循既有安全要求。
- 详细接口契约、校验规则与错误模型应在技术设计文档中维护。

**前端 / 交互边界:**

- **页面入口**：admin realm 托管的公共注册开通页面，未登录访客通过平台对外入口访问；非 admin realm 不承载此入口。
- **注册表单交互**：填写 realm 名称（必填）、访客邮箱（必填）、访客密码（必填）、realm 标识（可选，留空由系统生成）；提交前进行前端校验；当绑定自助注册页面的 Client App 的 Turnstile 为启用时，表单内嵌 Cloudflare Turnstile 人机验证组件，未通过验证不可提交（DEC-realm-create-008）。
- **限额提示**：同一 IP 24 小时内达到 2 个开通上限后，再次提交注册被拒绝并显示明确的限额提示（DEC-realm-create-007）。
- **成功反馈**：开通成功后自动将注册者带入新 realm 的管理控制台首页，无需额外登录步骤。
- **失败反馈**：校验失败与标识冲突时显示明确错误提示，并保留已填信息以便修正。
- **平台开关入口**：Admin Realm 管理员在管理后台可见平台自助开通开关控制（具体页面位置在技术设计中确定）。
- **关键状态反馈**：开通失败时显示明确错误，并说明部分数据可能残留的已知限制。

---

## 7. 已确认决策

> 本节只收录当前有效的决策与未决问题，记取舍、理由、决策人与重开条件；规则正文只在 §4 定义，产品级与实现级（设计层）决策均在此留痕，DEC/Q 编号保持稳定，供代码注释、测试与跨 PRD 引用追溯。

- **DEC-realm-create-001 · 入口归属与排他性**（user）：自助开通注册页面由 admin realm 托管并对未登录访客公开访问；该能力为 admin realm 独有，其他 realm 不承载平台级开通入口。理由：用户明确要求"admin realm 独有"；admin realm 是平台级 realm 生命周期的归属（`docs/prd/core/realm.md` §4.1）。落点：§2.1、§4.1、§6。重开条件：用户要求开通入口可由非 admin realm 承载。
- **DEC-realm-create-002 · 开通模型**（user + repository-fact）：注册即开通——访客提交注册信息后系统立即开通新 realm，注册者成为该 realm 的 realm-admin；注册环节不引入套餐选择或支付，付费由既有 billing 系统后续承载。理由：用户表述"注册帐号，并为其开通 realm"描述即时开通；仓库已有独立 billing 系统。落点：§2.1、§2.2、§4.1。重开条件：用户要求注册时选套餐或必须支付后才开通。
- **DEC-realm-create-003 · 初始化复用**（repository-fact + agent）：复用既有 realm 初始化机制（RBAC 默认角色/权限/策略、`admin-web-console`、`admin-api-client`、`registration.enabled=false`、Normal 状态管理员），不新建并行开通路径。理由：`docs/prd/core/realm.md` §3.2/§4.1 已定义稳定初始化规则；最小改动。落点：§2.1、§4.1。重开条件：初始化规则在正式 PRD 中变更。
- **DEC-realm-create-004 · 单次注册范围**（agent）：一次注册对应一个新 realm，注册者即该 realm 的 realm-admin；平台级"一个账号拥有多个 realm"不在本 PRD 范围。理由：最简可审阅模型；多 realm 归属需要平台账号层，超出本轮"注册即开通"意图。落点：§2.2、§4.1。重开条件：用户要求支持单账号多 realm 或跨 realm 所有者。
- **DEC-realm-create-005 · 用户故事新建**（agent）：新建独立用户故事（actor：SaaS 自助注册访客），不复用 `US-AR-001`（手动内部开通，actor 为 Admin Realm 管理员，入口与流程不同）。理由：两者 actor、入口、前置权限与验收目标不同，不合并冲突模式。落点：§1。重开条件：用户确认两者应合并为同一旅程。
- **DEC-realm-create-006 · 邮箱验证行为**（repository-fact + agent）：新 realm 管理员账号沿用既有 realm-create 行为——创建即 Normal（已验证），注册后可立即进入新 realm；是否强制"邮箱验证后才允许访问"作为防滥用细节，不做门禁。理由：与既有 `docs/prd/core/realm.md` §3.2 行为一致；防滥用阈值与策略为技术设计细节。落点：§4.1、§4.2。重开条件：用户要求邮箱前置验证。
- **DEC-realm-create-007 · IP 注册限额**（user）：同一 IP 每 24 小时最多自助注册开通 2 个 realm，超出后注册被拒绝并提示限额。理由：用户明确要求（安全防滥用）；防止刷号抢占 realm 标识。落点：§2.1、§4.1、§4.2、§5、§6。重开条件：用户调整限额阈值或窗口。
- **DEC-realm-create-008 · Turnstile 人机验证**（user + repository-fact）：自助注册页面启用 Cloudflare Turnstile 人机验证，条件是 admin realm 侧绑定的 Client App 的 Turnstile 已开启（未开启时不强制）。理由：用户明确要求"有 cloudflare 验证码如果 admin realm 开启了的话"；仓库事实——Turnstile 配置归属 Client App 级（`docs/prd/integration/client-app.md` §4.1 D-PROTECT-01），非 realm 级独立开关。落点：§2.1、§4.1、§4.2、§5、§6。重开条件：用户要求 Turnstile 与 Client App 解耦、作为独立 realm 级开关。
- **DEC-realm-create-009 · 平台开关必备**（user）：自助开通整体为一个平台开关（Admin Realm 管理员可开启/关闭），关闭后访客无法自助注册；该开关从 P1 提升为本 PRD 必备能力。理由：用户明确要求"这个功能要是一个开关"。落点：§1、§4.1、§5。重开条件：用户要求移除开关或调整其层级。
- **DEC-realm-create-010 · 后端 policy 旁路**（agent，实现层）：新增 `RealmService::create_realm_self_service(request, audit_ctx)`（不调用 `can_create_realm` policy gate），由 signup handler 自行保证前置校验（开关 + Turnstile + IP 限额）；既有 admin/ext 两条 `create_realm` 鉴权不变，共享抽出后的私有 `create_realm_inner`。理由：最小改动；不污染既有鉴权路径；显式命名"公开开通"新语义；备选方案（系统 Identity 特权、复用 admin 鉴权）扩散面大。落点：设计/实现层（`.ai/design/realm-create.md` §4.1/§5.1）。重开条件：需要在非公开端点也绕过 policy，或既有 create_realm 鉴权语义变更。
- **DEC-realm-create-011 · 限流计数口径**（agent，实现层）：同 IP 24h 限流计数口径为"进入开通阶段的尝试计数"（在通过校验与 Turnstile 后、调用 create_realm 前执行 `rate_limit_hit`，失败不回退），满足验收"同 IP 第 3 次 429"。理由：复用既有 `rate_limit_hit`（先 INCR 再判断）；避免预检+成功后记录两段式复杂度。落点：§4.2、设计层。重开条件：用户要求仅计成功开通，或线上反馈失败请求误耗配额。
- **DEC-realm-create-012 · 会话目标 realm**（repository-fact + agent，实现层）：signup 成功后签发**新 realm** 的 `admin-web-console` first-party token（`create_first_party_token_family`），而非 admin realm 的 token；响应携带 `realmId`/`realmName` 供前端切换路由上下文。理由：US-SR-002 要求"立即进入新 realm 管理控制台"；create_realm 已在新 realm 创建 first-party admin-web-console 与 Normal 管理员，签发无传播延迟。落点：§5、设计层。重开条件：用户要求签发 admin realm token 或要求额外登录步骤。
- **DEC-realm-create-013 · 开关存储**（agent，实现层）：平台开关存于 admin realm 的 `realm_config`（新增 `ConfigType::PlatformSignup`，`config_key="enabled"`，`config_value="true"/"false"`），不新建表；读取缺失按 `false`（fail-closed）。理由：`realm_config` 是 realm 级配置唯一存储；admin realm 是平台级配置自然归属。落点：§4.1、设计层。重开条件：用户要求独立平台配置表或独立开关服务。
- **DEC-realm-create-014 · 冲突状态码**（repository-fact + agent，实现层）：realmSlug 已占用时 signup 返回 400（沿用既有 realm 仓库 `CoreError::BadRequest` 与 admin `create_realm` 的 400 约定），而非设计稿理想化的 409。理由：signup 复用同一 `create_realm` 仓库路径；仅为 signup 单独改 409 会分裂两个调用方。落点：设计层。重开条件：用户要求 realmSlug 冲突统一为 409（届时需同时改仓库返回并同步 admin/ext 路径）。

**问题记录**（原账本 Resolved / Deferred Questions）：

- `Q-realm-create-003`（延期）：平台自助开通开关的默认值（默认开启/关闭）属运营决策，不改变功能存在性与验收目标。须在 t-run 前决议。

---

## 8. 参考资料

- 既有 Realm 管理 PRD：`docs/prd/core/realm.md`（手动内部开通基线，本 PRD 与之互补，不覆盖其开通能力）
- 既有 Realm Settings PRD：`docs/prd/core/realm-settings.md`（平台开关以 realm_config 形式管理，遵循 realm-settings 的配置管理能力边界）
- Client App PRD：`docs/prd/integration/client-app.md`（Turnstile 配置归属 Client App 级，见 §4.1 D-PROTECT-01）
- 角色定义：`docs/user-stories/_roles.md`
- 技术设计：`.ai/design/realm-create.md`
- 用户故事来源见 §1 表格
