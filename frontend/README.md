# Frontend

React（Vite）+ TanStack Router（文件式路由）+ TanStack Query + paraglide JS（i18n）。

## 路由树结构：真实树 + 会话作用域镜像树

`src/routes/` 下有三组顶层入口，realm 的解析规则集中在 `src/lib/realm-routing.ts`：

1. **`$realmId/**`— 真实路由树。** 带租户前缀的 URL（如`/acme/manage/users`）全部在此实现：
管理页的 `beforeLoad` 门控（`initializeAuth`+`requireFeature`）、`validateSearch`
   和页面组件（或页面包装组件）都从这里定义并导出。
2. **`manage/**`、`user/**`、`subscription/**`— 会话作用域镜像树。** URL 不携带 realm
（如`/manage/users`），realm 从会话存储读取（`useAuthStore`的`realmId`，缺失时回退
`admin`）。这些文件是薄别名：直接复用 `$realmId/\*\*` 树导出的组件。
3. **`auth/**`、`legal/**`、`device\*` — 公共页面。** 在主域名与自定义域名上均无 realm 前缀，
   自定义域名经 resolve 接口反查 realm（见 `resolveRealmContext`）。

约定：

- 新增管理页**先在 `$realmId/manage/**`实现**（含门控与 search 校验），再到`manage/\*\*`
  建薄别名文件复用同一组件。
- 两棵树的守卫语义保持同构：`$realmId` 树上的 `requireFeature` 门控与 `validateSearch`
  必须与镜像树同步——镜像树通过 `manageFeatureGuard`（见 `src/lib/manage-feature-guard.ts`）
  复用同一门控，避免同一页面因 URL 形式不同而守卫不一致。
- 会话作用域根段（`manage`/`user`/`subscription`）不得被 root loader 当作 realm root
  处理，否则会把已认证管理员重定向回 `/manage` 形成自重定向循环
  （见 `isSessionScopedPath`）。

## 其他入口

- 视觉规范单一事实源：仓库根 `DESIGN.md`
- 测试与脚本入口：仓库根 `scripts/index.md`
