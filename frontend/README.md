# Frontend

React（Vite）+ TanStack Router（文件式路由）+ TanStack Query + paraglide JS（i18n）。

## 路由树结构：主域名租户路径 + 自定义域名镜像树

`src/routes/` 下有三组顶层入口，realm 的解析规则集中在 `src/lib/realm-routing.ts`：

1. **`$realmId/**`— 真实路由树。** 带租户前缀的 URL（如`/acme/manage/users`）全部在此实现：
管理页的 `beforeLoad` 门控（`initializeAuth`+`requireFeature`）、`validateSearch`
   和页面组件（或页面包装组件）都从这里定义并导出。
2. **`manage/**`、`user/**`、`subscription/**`— 自定义域名镜像树。** URL 不携带 realm
（如`/manage/users`），realm 由后端根据请求 Host 查询已绑定的域名映射。这些文件是
薄别名：直接复用 `$realmId/\*\*` 树导出的组件。主域名的无前缀受保护地址会提示缺少
   租户信息，不根据登录 store 猜测 realm。
3. **`auth/**`、`legal/**`、`device\*` — 自定义域名公共页面。** 主域名使用
   `/{realmId}/auth/...` 等路径；旧主域名 `/auth/...` 等公共入口规范化到 admin 前缀。

主域名示例：`https://auth.example.com/acme/manage/settings`；自定义域名示例：
`https://login.acme.com/manage/settings`。两种入口均通过 `realmPath` 生成站内链接。

约定：

- 新增管理页**先在 `$realmId/manage/**`实现**（含门控与 search 校验），再到`manage/\*\*`
  建薄别名文件复用同一组件。
- 两棵树的守卫语义保持同构：`$realmId` 树上的 `requireFeature` 门控与 `validateSearch`
  必须与镜像树同步——镜像树通过 `manageFeatureGuard`（见 `src/lib/manage-feature-guard.ts`）
  复用同一门控，避免同一页面因 URL 形式不同而守卫不一致。
- 无前缀产品根段（`manage`/`user`/`subscription`）不得被 root guard 当作 realm root
  处理，否则会把已认证管理员重定向回 `/manage` 形成自重定向循环
  （见 `isSessionScopedPath`）。
- root `beforeLoad` 先解析 Host/URL、恢复会话并校验 realm，再把 `realmContext` 传给
  子路由。镜像守卫只能消费此 context；组件使用 `useResolvedRealmContext`，store
  中的认证身份不得覆盖 URL/Host。自定义域名下其他 realm 的前缀会被拒绝，本 realm
  的前缀会规范化为无前缀路径。域名解析只有 404 可表示主域名模式，网络/服务错误必须失败关闭。
- 主域名裸根 `/` 明确使用 admin；自定义域名裸根使用绑定 realm。两者不读取全局
  「上次登录 realm」。主域名旧 `/manage/...` 书签必须换成含 realm 的完整链接。
- SDK refresh token 继续按 realm 存在 localStorage。认证 UI 状态仅在当前标签内存中，
  `auth-storage` 现在只在 sessionStorage 保存有 realm 和有效期的 pending PKCE 事务；
  旧 localStorage 认证快照不再恢复，并在初始化时移除。跨 origin 的凭据不自动共享。
- 同一 QueryClient 在 realm、账号或产品身份变化时先取消旧查询、清空缓存，再加载页面；
  迟到的认证响应和 API 重试必须绑定原 SDK client，不能借用新 realm 的凭据。
- 平台自助创建新租户只在主域名提供；已绑定的自定义域名固定服务其当前 realm，
  不提供创建另一 realm 后切换到其控制台的入口。本 realm 的账号注册不受此约束。

## 其他入口

- 视觉规范单一事实源：仓库根 `DESIGN.md`
- 测试与脚本入口：仓库根 `scripts/index.md`
