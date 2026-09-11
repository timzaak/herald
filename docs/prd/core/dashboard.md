# Dashboard Redesign 产品需求文档 (PRD)

**创建时间**: 2026-05-16
**优先级**: P1

---

## 1. 相关用户故事

> 详细故事与验收标准请查看 `docs/user-stories/` 中对应文档。

### 1.1 相关故事

- `[US-RA-010]` 查看 Dashboard 用户活跃概览，优先级 P1，来源 `docs/user-stories/core/realm-admin.md`
  - 角色：Realm Admin
  - 摘要：Realm Admin 在 Dashboard 首屏看到本 Realm 的用户核心指标（总用户数、新增用户数、活跃用户数）

- `[US-RA-011]` 查看 Dashboard 认证趋势图，优先级 P1，来源 `docs/user-stories/core/realm-admin.md`
  - 角色：Realm Admin
  - 摘要：Realm Admin 在 Dashboard 上看到最近 30 天的认证趋势图，发现异常登录波动

- `[US-RA-012]` 通过 Dashboard 快捷导航跳转，优先级 P1，来源 `docs/user-stories/core/realm-admin.md`
  - 角色：Realm Admin
  - 摘要：Dashboard 下方保留原有管理功能导航入口，支持快速跳转

### 1.2 优先级汇总

| 优先级 | 数量 | 关键故事 |
|--------|------|----------|
| P0 | 0 | - |
| P1 | 3 | 用户活跃概览、认证趋势图、快捷导航 |
| P2 | 0 | - |

---

## 2. 范围界定

### 2.1 包含功能

- Admin Dashboard 首屏展示 3 张用户指标卡片：总用户数、最近 7 天新增用户数、最近 7 天活跃用户数
- 认证趋势图：最近 30 天按天聚合的登录成功/失败次数
- 保留原有 6 张导航卡片的快捷入口（收缩为紧凑网格）

### 2.2 不包含功能 (Out of Scope)

- 自定义时间范围选择器（首版固定 7 天/30 天窗口）
- 实时推送/WebSocket（首版使用页面加载时拉取）
- Roles、Permissions、Client Apps 的计数统计
- 数据导出功能
- 指标告警/阈值通知
- 用户侧 Dashboard（仅 Admin Dashboard）

### 2.3 依赖项

- Realm 系统 — Dashboard 所有指标必须 Realm 隔离
- 用户认证系统 — 审计事件（AuthLogin、AuthLoginFailed）作为趋势图数据来源
- Realm Admin 权限检查机制 — Dashboard 访问权限控制
- Realm 创建流程 — 创建 Realm 后应能访问其 Dashboard

---

## 3. 需求概述

### 3.1 功能描述

当前 Admin Dashboard 仅是纯导航入口页面，缺少运营洞察能力。本次重设计将 Dashboard 升级为以用户活跃为核心的运营概览页，聚焦 Realm Admin 最关心的指标：总用户数、新增用户数、登录活跃数。通过认证趋势图辅助发现异常登录行为。

### 3.2 关键特性

- **指标卡片首屏展示**：总用户、新增用户（7天）、活跃用户（7天）三张核心卡片
- **认证趋势可视化**：30 天登录成功/失败趋势图，按天聚合
- **导航入口保留**：原有 6 张导航卡片收缩为紧凑网格，不丢失功能入口
- **Realm 隔离**：所有指标严格按 Realm 隔离，不跨 Realm 泄露数据

---

## 4. 业务规则与状态

### 4.1 业务规则

- 所有指标严格按当前 Realm 隔离，不跨 Realm 泄露数据
- 指标卡片展示固定时间窗口：新增用户（7天）、活跃用户（7天）、认证趋势（30天）
- **用户数口径**：总用户数与新增用户数按该 Realm 全部 account 行计数，不按状态过滤——`WaitVerified`（待验证）与 `Deleted`（已注销的匿名化终态）账户同样计入；活跃用户数按最近 7 天有审计事件（登录等动作）的独立用户计数。运营侧应知悉卡片数值是「账户行数」而非「可登录用户数」
- Dashboard 数据在页面加载时一次性拉取，首版不做实时刷新
- 访问控制通过 RBAC 策略 `dashboard.view` 授权，Realm Admin 角色默认包含此权限

### 4.2 关键状态与异常

- **新 Realm 空态**：新 Realm 无数据时，指标卡片显示 0，趋势图显示"暂无数据"
- **加载状态**：数据加载期间使用 Skeleton 占位
- **错误状态**：数据加载失败使用现有错误处理模式

---

## 5. 功能需求

### 5.1 核心需求

1. **用户指标卡片**：Dashboard 首屏展示总用户数、最近 7 天新增用户数、最近 7 天活跃用户数三张指标卡片
2. **认证趋势图**：展示最近 30 天按天聚合的登录成功次数和失败次数趋势，缺失日期自动补零（0 成功、0 失败），确保返回完整 30 天数据
3. **快捷导航**：保留原有 Users、Roles、Permissions、Client Apps、Realms、Settings 导航入口
4. **空态处理**：新 Realm 无数据时，指标卡片显示 0，趋势图显示"暂无数据"
5. **加载状态**：数据加载期间使用 Skeleton 占位

### 5.2 验收目标

- Realm Admin 登录后进入 Dashboard，首屏可见 3 张指标卡片和趋势图
- 所有指标严格按当前 Realm 隔离
- 原有 6 个导航入口全部保留且可正常跳转
- 新 Realm 的 Dashboard 不报错，显示合理的空态

---

## 6. API 相关约束

**适用性**: 适用

- Dashboard Stats 接口一次性返回所有指标数据（用户指标 + 趋势数据）
- 访问控制：通过 RBAC 策略 `dashboard.view` 授权，Realm Admin 角色默认包含此权限
- 租户边界：所有查询强制 Realm 过滤

---

## 7. 前端/交互约束

**适用性**: 适用

- 页面入口：管理后台路由（内容变化，路由不变）
- 页面结构：顶部 3 张指标卡片 → 认证趋势图（全宽） → 底部快捷导航网格
- 关键交互：页面加载自动拉取数据；"Total Users" 卡片可点击跳转用户管理页
- 状态反馈：加载中 Skeleton、空态显示 0 / "暂无数据"、错误使用现有错误处理模式

---

## 8. 已确认决策

### 8.1 已确认决策

- Dashboard 首版使用页面加载时拉取模式，不引入实时推送
- 时间窗口固定为 7 天（指标卡片）和 30 天（趋势图），首版不做自定义时间范围
- 认证趋势数据复用审计模块已有事件记录

---

## 9. 参考资料

- 用户故事：`docs/user-stories/core/realm-admin.md`
- 相关 PRD：`docs/prd/core/audit.md`（审计日志，Dashboard 聚合其数据）
- IAM Dashboard 行业参考：Cloudeagle IAM Key Metrics、Reddit r/ProductManagement KPI 讨论
