# Herald 用户故事

本目录包含 Herald 系统的用户故事，遵循用户故事编写规范。

---

## 目录结构

```
docs/user-stories/
├── index.md              # 用户故事索引（含全部 US-ID 对照表）
├── _README.md            # 本文档
├── _roles.md             # 统一角色定义参考
├── core/                 # 核心功能
├── auth/                 # 认证授权
├── billing/              # 计费相关
└── integration/          # 集成扩展
```

---

## 编写新用户故事

1. 参考 [`_roles.md`](_roles.md) 确认角色定义
2. 遵循 INVEST 原则
3. 使用 GWT 格式编写验收标准
4. 在用户故事文件开头引用 `_roles.md` 中的对应角色

**引用格式示例**：

```markdown
### 故事 1：用户管理

**【用户故事】**
**作为**：主管理员（详见 [docs/user-stories/_roles.md](_roles.md)）
**我希望**：能够查看所有Realm的用户
**从而**：统一管控系统用户访问权限
```

---

## 维护规范

### 新增/修改角色

更新角色定义时的步骤：

1. **更新** [`_roles.md`](_roles.md) 中的角色定义
2. **同步代码实现**：
   - 前端：frontend/src/lib/auth-service.ts 的角色获取逻辑
   - 后端：backend/api-admin/src/admin/middleware.rs 的权限校验逻辑
3. **在用户故事中引用**：使用新的角色定义

### 新增用户故事

- 先在 [`index.md`](index.md) 选择对应能力包；优先追加到同一用户旅程的现有文件，避免为单个小需求新建文件
- 仅当业务对象、生命周期或交付边界可以独立成立时新建文件
- 确保引用 [`_roles.md`](_roles.md) 中的角色定义

### 禁止事项

- ❌ 在单个用户故事文件中重复定义角色
- ❌ 与 [`_roles.md`](_roles.md) 中的定义不一致
- ❌ 直接在用户故事中硬编码角色权限详情

---

## 相关文档


- [`_roles.md`](_roles.md) - 统一角色定义参考
- [`index.md`](index.md) - 用户故事索引
- [`docs/prd/index.md`](/docs/prd/index.md) - PRD 文档索引
