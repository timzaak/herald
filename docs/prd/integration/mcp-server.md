# MCP Server 产品需求文档 (PRD)

**创建时间**: 2026-08-31
**优先级**: P1

---

## 1. 相关用户故事

> 详细故事与验收标准请查看 `docs/user-stories/` 中对应文档。

| US-ID | 标题 | 优先级 | 来源 |
|-------|------|--------|------|
| US-MCP-001 | 把 Herald 接入 AI agent 客户端 | P0 | `docs/user-stories/integration/mcp-server.md` |
| US-MCP-002 | 通过 agent 查询用户 | P1 | `docs/user-stories/integration/mcp-server.md` |
| US-MCP-003 | 通过 agent 查询积分余额 | P1 | `docs/user-stories/integration/mcp-server.md` |
| US-MCP-004 | 通过 agent 查询积分交易流水 | P1 | `docs/user-stories/integration/mcp-server.md` |
| US-MCP-005 | 通过 agent 查询审计日志 | P1 | `docs/user-stories/integration/mcp-server.md` |
| US-MCP-006 | 通过 agent 查询 Realm 配置状态 | P1 | `docs/user-stories/integration/mcp-server.md` |
| US-RA-016 | API Key 角色管理（MCP 接入凭证完全复用该体系，本 PRD 不重复定义） | — | `docs/user-stories/core/realm-admin.md` |
| US-RA-017 | 创建 API Key 时绑定角色 | — | `docs/user-stories/core/realm-admin.md` |
| US-RA-018 | API Key 按 Client App 隔离 | — | `docs/user-stories/core/realm-admin.md` |

---

## 2. 范围界定

### 2.1 包含功能

- 面向 AI agent 客户端的 Herald MCP 服务：一个公网可接入的 MCP 协议端点，支持 agent 客户端完成连接、获取工具清单、调用工具的完整链路
- 首发五项只读查询能力（对应 `DEC-mcp-server-001`）：
  1. 查询用户（列表与详情）
  2. 查询指定用户的积分余额
  3. 查询指定用户的积分交易流水
  4. 查询本租户审计日志
  5. 查询本租户配置状态概览
- 基于 Client API Key 的接入鉴权，复用既有的 API Key 启用/过期校验、Client App 禁用级联失效、角色绑定与权限检查
- 严格的租户（realm）数据隔离：所有工具仅返回调用凭证所属租户的数据
- 主流 agent 客户端（Claude Code、Cursor、VS Code 等）的接入文档（含一行式配置示例）与连通性自检说明
- 官方文档站发布 MCP 集成指南（随文档站既有机制自动进入 agent 可发现的文档索引）

### 2.2 不包含功能 (Out of Scope)

- 写操作工具（创建用户、发放积分、修改配置等）：验证期通过、出现 agent 使用信号后，按逐工具安全评审准入（`DEC-mcp-server-001` 的既定扩展路径），不在首发范围
- OAuth 2.1 授权流（浏览器授权、受保护资源元数据、动态客户端注册）：不实现，列为演进路径（`DEC-mcp-server-002`）
- MCP Auth 等价物（客户的 MCP server 用 Herald 做鉴权）：属另一产品方向，依赖已 Park 的 OIDC 决策重开（延期问题 `Q-mcp-server-002`，不影响本 PRD）
- 管理后台任何页面改动或新增管理界面
- 数据库表结构与数据模型变更
- 新增权限定义或内置角色（全部复用既有 `resource.action` 权限体系）
- 专用独立部署形态（MCP 端点与现有服务同进程暴露）

### 2.3 依赖项

以下均为既有能力，本功能零新增：

- Client API Key 体系：创建、角色绑定、启用/禁用、Client App 作用域与禁用级联失效
- 既有 RBAC 权限模型与租户隔离校验
- 既有 API Key 用量统计链路（只读阶段的可观测手段）
- 既有服务限流与安全中间件栈
- 官方文档站与面向 agent 的文档索引

---

## 4. 业务规则与状态

### 4.1 业务规则

- **工具边界**：首发仅五项只读查询能力；不提供任何修改、删除或创建类工具
- **鉴权规则**：仅接受有效的 Client API Key；密钥无效、已禁用、已过期或其绑定 Client App 被禁用的请求一律拒绝，不进入工具执行。传输形态有两种：优先 `X-API-Key` 头；亦接受 `Authorization: Bearer <Client API Key>`（部分 MCP 客户端仅支持 Bearer 传输）——Bearer 形态一律按 Client API Key 解释，不作为用户 Bearer token 校验
- **权限规则**：每项工具映射到既有的对应查看权限（用户查看、积分查看、审计查看、设置查看）；凭证未持有该权限时工具返回权限不足错误，不返回任何数据
- **租户边界**：所有工具仅能访问凭证所属租户的数据；查询目标不在凭证所属租户时表现为资源不存在，不返回任何数据
- **校验顺序**：鉴权、权限、取数三道校验按序执行——先鉴权、再权限、后取数
- **积分余额作用域**：绑定普通 Client App 的 API Key 只汇总该 App 覆盖的积分账户；绑定内置 `admin-api-client` 或历史无绑定 Key 时返回 Realm 范围余额，与 ext API 口径一致
- **MCP 专用限流**：每个 API Key 的 MCP 请求量受独立的固定窗口限流约束，超限返回 429；限流与具体工具的业务权限检查互相独立（阈值等实现细节由技术设计与部署配置承载，不入 PRD）
- **时间筛选格式**：积分交易与审计工具统一接受 RFC 3339 时间戳，或按 UTC 零点解释的 `YYYY-MM-DD`
- **数据最小化**：工具返回字段以回答业务问题所需为限，不透出多余字段，降低查询结果被 agent 带出至第三方模型的数据面
- **错误语义**：业务错误（权限不足、未找到、参数错误）以工具级、agent 可读的错误返回；不向 agent 暴露内部错误细节
- **用量与观测**：复用既有 API Key 用量统计；只读阶段不新增审计事件
- **协议兼容**：实现 MCP 现行稳定规范（2026-07-28），兼容主流 agent 客户端；不实现 OAuth 授权流符合该规范（授权为可选能力）
- **演进约束**：写操作工具进入前必须逐工具通过安全评审，评审要素含最小权限映射、审计覆盖、排除不可逆/破坏性操作、批量上限、限流约束与返回字段最小化

### 4.2 关键状态与异常

- **接入异常**：无效凭证、禁用凭证、禁用 Client App 三种情况均在连接/调用入口拒绝，用户与 agent 收到明确的鉴权失败信息
- **调用异常**：权限不足、资源不存在（含跨租户目标）、参数校验失败均返回对应的 agent 可读错误，不返回部分数据
- **验证期状态（Wedge）**：首发发布后进入 4-6 周观察期；若无 agent 使用信号、无社区反馈增长，则停止扩展写操作，降级为实验特性或下线（既定 Kill Criteria）

---

## 5. 验收目标

- 主流 agent 客户端按官方文档配置后，能成功连接并列出全部五项查询工具
- 具备对应权限的凭证调用每项工具均返回正确的租户数据
- 无效/禁用凭证、缺权限、跨租户三类调用分别被明确拒绝，且错误信息对 agent 可读
- Live Demo 环境端到端可用：真实 agent 客户端经 API Key 完成接入并成功执行查询
- 发布后进入观察期，按 §4.2 的验证期状态评估去留信号

---

## 6. 边界与约束

**适用性**: API 边界适用；前端/交互约束不适用（无管理后台或终端用户界面改动）

**API / 集成边界:**
- **能力范围**：仅查询类工具能力（五项，见 §2.1）；无写入、回调或推送类能力
- **访问控制**：Client API Key 鉴权 + 既有 `resource.action` 权限检查 + 租户隔离（校验顺序见 §4.1）
- **协议边界**：MCP 端点是独立于既有 REST 管理 API 的协议面，不进入 REST 接口文档（OpenAPI）体系；协议版本兼容性与具体端点形态、工具命名、入参结构不在本 PRD 定义，归技术设计阶段
- **接口说明位置**：工具清单与调用语义在技术设计阶段定义后，随 MCP 协议的工具发现机制向 agent 客户端自描述，接入文档面向人类开发者

**前端 / 交互边界:** 不适用（无管理后台或终端用户界面改动）。本功能的用户可见面为面向开发者的接入文档与连通性自检说明（官方文档站），以及 agent 客户端内的工具交互（由 agent 客户端自身呈现，Herald 不控制其 UI）。

---

## 7. 已确认决策

> 本节只收录当前有效的决策与未决问题，记取舍、理由、决策人与重开条件；规则正文只在 §4 定义，DEC/Q 编号保持稳定，供代码注释、测试与跨 PRD 引用追溯。

- **DEC-mcp-server-001 · mcp.tool-boundary**（agent）：首发只读工具集（五项查询能力：用户、积分余额、积分流水、审计、Realm 配置状态），每个工具映射既有 `resource.action` 权限；写操作在 Wedge 验证后按逐工具安全评审加入（Minimal 阶段）。理由：最小安全面；1-2 天可交付楔子；对齐 `.ai/future/next.md` §4.4 Wedge→Minimal 两步走路径；AskUserQuestion 提问未获答复，按 next.md 既有推荐采纳，用户可在 /t-prd 推翻。落点：§2.1、§2.2、§4.1、§5。重开条件：Wedge 验证通过（4-6 周出现 agent 使用信号）进入 Minimal；或用户在 /t-prd 明确改边界。
- **DEC-mcp-server-002 · mcp.auth-model**（agent）：MCP 端点复用 Client API Key 鉴权（api-ext 中间件模式）+ 既有 RBAC/realm 隔离；不实现 OAuth 2.1 授权流（RFC 9728/WWW-Authenticate/DCR 均不引入），OAuth 列为演进路径（规则正文见 §4.1）。理由：MCP 2026-07-28 规范明确 Authorization OPTIONAL，API Key 不违规；rmcp OAuth 能力仅客户端侧，服务端 OAuth 需自建成本高；主流客户端支持自定义 header 携带 API Key（Claude Code `--header` 官方文档化）；架构级集成选择，影响 prd/design/task。落点：§2.2、§4.1、§6。重开条件：客户端生态对静态 header 支持恶化（Claude Code #14976 类问题扩大），或用户要求浏览器授权 UX（Logto 同款体验）。

**问题记录**（原账本 Resolved / Deferred Questions）：

- `Q-mcp-server-002`（延期）：MCP Auth 等价物（客户用 Herald 给自己的 MCP server 做 OAuth 2.1 鉴权）属另一产品方向，依赖已 Park 的 OIDC 决策（`.ai/decision/openid-connect.md`）重开，且无需求证据，不影响本 feature 的 MCP server 路线。须在用户选择追求该方向时的 /t-decision 决议。

---

## 8. 参考资料

- 技术预研：`.ai/tech-research/mcp-server.md`
- 技术设计：`.ai/design/mcp-server.md`
- 官方 MCP 集成指南：https://www.fornetcode.com/en/docs/integration/mcp （中文：https://www.fornetcode.com/zh/docs/integration/mcp ）
- 相关已发布 PRD：`docs/prd/integration/api-key-roles.md`（API Key 角色绑定）、`docs/prd/integration/sdk.md`（SDK 接入面）、`docs/prd/core/audit.md`（审计日志）、`docs/prd/billing/points.md`（积分系统）
- MCP 授权规范（2026-07-28）：https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization
- 用户故事来源见 §1 表格
