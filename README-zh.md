# Herald

中文 | [English](README.md)

[![CI Pipeline](https://github.com/timzaak/herald/actions/workflows/ci.yml/badge.svg)](https://github.com/timzaak/herald/actions/workflows/ci.yml)
[![CD Pipeline](https://github.com/timzaak/herald/actions/workflows/cd.yml/badge.svg)](https://github.com/timzaak/herald/actions/workflows/cd.yml)
[![Release](https://img.shields.io/github/v/release/timzaak/herald)](https://github.com/timzaak/herald/releases/latest)

**给 AI 产品用的认证、计费、积分和管理后台，放在一套开源代码库里。**

自己部署，所有租户的账号、支付和积分记录都存在你自己的 Postgres 里。把仓库交给 AI 编程工具，它拿到的是一套能跑的产品，不是空工程。Apache-2.0 许可证，不按用户数收费。

[官网](https://www.fornetcode.com/zh) · [在线演示](https://auth.fornetcode.com) · [快速开始](https://www.fornetcode.com/zh/docs/getting-started) · [Star on GitHub](https://github.com/timzaak/herald)

## 为什么用 Herald

AI 创业团队总在撞同一堵墙：产品本体用 AI 工具几天就能做出来，之后用户看不见的部分要耗掉几周，账号、租户隔离、角色、订阅、用量积分、管理后台。这些要么全部从零写，要么拼接独立的认证、支付、计费服务，然后祈祷接线不出事。

出事的总是接线。每家服务有自己的用户 ID、自己的 webhook、自己的重试规则。订阅续费成功但 webhook 丢了，用户付了钱却失去访问权，你从一封客服邮件里才知道这件事。

Herald 把身份、支付、权益和用量放进同一套代码，这些连接是你读得到、改得动的代码：

- 用户购买后自动获得访问权和积分；
- 退款或取消订阅后自动收回权益；
- 每个租户有独立的用户、角色、应用、品牌和计费配置。

如果你只需要登录加刷卡，托管的认证服务更省事。下面这些情况 Herald 才划算：需要租户级控制、按积分计价，或者要求用户数据放在自己的数据库里。

## 开箱即用的能力

| 领域 | 已包含的能力 |
|------|--------------|
| **多租户身份** | 相互隔离的 Realm、邮箱密码与邮箱验证码登录、Google、GitHub、Apple、Facebook、微信、Discord、LDAP 登录、Passkey、TOTP 双因素、OpenID Connect 身份层、Cloudflare Turnstile 人机验证 |
| **授权与应用** | Realm 级 RBAC、带独立角色和成员的组织、客户端应用、API Key、OAuth 2.0、设备授权、跨应用单点登录 |
| **计费与支付** | Stripe、Creem、微信支付，App Store / Google Play 内购，订阅、一次性购买、多币种定价、发票、支付到权益的映射、支付与积分消耗统计 |
| **积分与用量** | 预付余额、充值、退款、过期、用户账本、积分发放、滚动配额窗口 |
| **后台与运营** | 用户、角色、计费、积分、应用、租户设置、审计轨迹、账号生命周期管理 |
| **产品定制** | 自定义域名、白标品牌、交易邮件、版本化用户协议、API 文档、SDK |

这套底座为带付费墙、免费额度、付费套餐、按量使用或积分计价的 AI 产品而建。

## 接入长什么样

浏览器端用 `herald-auth-web` 处理注册、登录、双因素和静默刷新 token：

```ts
import { createHeraldClient } from 'herald-auth-web'

const client = createHeraldClient({
  baseUrl: 'https://auth.example.com',
  realmId: '<your-realm>',
  clientId: '<your-client-app>',
})

const result = await client.login({ email: 'user@example.com', password: '••••••••' })
// result.kind: 'success' | 'requires-second-factor' | 'consent-required' | 'oauth-redirect'
```

服务端用 `herald-sdk`（Node 和 Rust）对同一份数据查权限、查积分余额：

```ts
import { HeraldClient } from 'herald-sdk'

const herald = new HeraldClient('https://your-herald-instance.com', '<your-api-key>')

const check = await herald.checkPermission({
  accessToken: 'user-browser-token',
  rules: [{ resource: 'document', action: 'read' }],
})

const balance = await herald.getBalance('realm-id', 'user-id')
```

浏览器 SDK 能从你的页面调用 API 之前，先把页面 origin 加到 Client App 的 `allowed_origins` 里，在 Herald 控制台改。

## 先试一下

在线版，零安装：打开 [auth.fornetcode.com](https://auth.fornetcode.com)，登录页点「开通自己的 Realm」。你会成为这个租户的管理员，用户和计费配置完全隔离，随便折腾不影响别人的数据。

本地运行需要 Python 3.12+、[uv](https://github.com/astral-sh/uv)、Docker、Cargo 和 npm：

```bash
git clone https://github.com/timzaak/herald.git
cd herald
uv run scripts/demo-start.py
```

启动后前端在 http://localhost:3000，后端 API 在 http://localhost:8080。手动安装和后续配置见[快速开始文档](https://www.fornetcode.com/zh/docs/getting-started)。

部署卡住，或在评估 Herald？发邮件到 [zsy.evan@gmail.com](mailto:zsy.evan@gmail.com)，很乐意帮忙。

## 为 AI 辅助迭代而设计

这个仓库本来就打算让 AI 编程工具来改：

- LLM 文档：[llms.txt](https://www.fornetcode.com/llms.txt)（索引）· [llms-full.txt](https://www.fornetcode.com/llms-full.txt)（全文）· 任意文档 URL 加 `.md` 取 Markdown
- API 参考（OpenAPI）：[`docs-web/openapi.json`](docs-web/openapi.json)
- MCP 服务，OAuth 浏览器登录，不需要 API Key，也不需要密钥：

  ```bash
  claude mcp add --transport http herald https://your-herald-host/mcp/your-realm-id
  ```

接入配置与工具说明：[MCP 集成（AI Agent 接入）](https://www.fornetcode.com/zh/docs/integration/mcp)。

[herald-app-example](https://github.com/timzaak/herald-app-example) 是一个完整接入 Herald 认证的 Flutter App，全程用 AI 开发，可以作为这种做法的参考。Herald 本身用 Claude Code、GLM 和 Codex 混合开发，工具链基于 [web-dev-skills](https://github.com/timzaak/web-dev-skills)。

## 联系我们

正在基于 Herald 构建产品？欢迎告诉我们你在做什么。

- 🐛 提交 [GitHub Issue](https://github.com/timzaak/herald/issues) 反馈 bug 与功能需求
- ✉️ [zsy.evan@gmail.com](mailto:zsy.evan@gmail.com) 部署咨询、产品反馈，或聊聊你在做的东西

## 许可证

Herald 采用 [Apache-2.0](LICENSE) 许可证。可以在商业产品里使用、修改和分发，开源项目不收按用户数计算的许可证费用。
