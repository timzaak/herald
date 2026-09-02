# Herald

中文 | [English](README.md)

[![CI Pipeline](https://github.com/timzaak/herald/actions/workflows/ci.yml/badge.svg)](https://github.com/timzaak/herald/actions/workflows/ci.yml)
[![CD Pipeline](https://github.com/timzaak/herald/actions/workflows/cd.yml/badge.svg)](https://github.com/timzaak/herald/actions/workflows/cd.yml)
[![Release](https://img.shields.io/github/v/release/timzaak/herald)](https://github.com/timzaak/herald/releases/latest)

**面向 AI 产品的开源、自托管基础设施。**

从已经打通的多租户认证、计费、支付、积分和管理后台开始，借助 AI 辅助开发持续改造开放代码，把每一次迭代都用在客户真正愿意付费的产品逻辑上。

[官网](https://www.fornetcode.com/zh) · [在线演示](https://auth.fornetcode.com) · [快速开始](https://www.fornetcode.com/zh/docs/getting-started) · [Star on GitHub](https://github.com/timzaak/herald)

## 为什么选择 Herald

AI 创业团队需要快速验证和持续迭代，但一个能够收费的产品仍然需要账号、租户隔离、权限、订阅、用量积分和运营工具。从零开发这些系统，或者拼接多个服务，都会挤占真正用于产品差异化的时间。

Herald 不只是认证服务。它在同一套代码中把身份、支付、权益和产品用量连接起来：

- 用户购买后可以自动获得访问权和积分；
- 退款或取消订阅后可以自动收回权益；
- 每个租户都可以拥有独立的用户、角色、应用、品牌和计费配置；
- 整套基础设施都可以自托管并按业务需要修改。

## 开箱即用的能力

| 领域 | 已包含的能力 |
|------|--------------|
| **多租户身份** | 相互隔离的 Realm、邮箱密码、Google、GitHub、Apple、Facebook 和微信登录、Passkey、TOTP 双因素认证、人机验证 |
| **授权与应用** | Realm 级 RBAC、客户端应用、API Key、OAuth 2.0、设备授权、跨应用单点登录 |
| **计费与支付** | Stripe、Creem、WeChat Pay，App Store / Google Play 内购，订阅、一次性购买、发票、支付到权益的映射 |
| **积分与用量** | 预付余额、充值、退款、过期、用户账本、积分发放、多时间窗滚动配额 |
| **后台与运营** | 用户、角色、计费、积分、应用、租户设置、审计轨迹和账号生命周期管理 |
| **产品定制** | 自定义域名、白标品牌、交易邮件、版本化用户协议、API 文档和 SDK |

这套底座尤其适合提供免费额度、付费套餐、按量使用或积分计价的 AI 产品。

## 为 AI 辅助迭代而设计

Herald 为 AI 编程工具提供一套完整、可运行的产品底座，而不是一个空仓库或一组彼此割裂的 API。你可以在统一的身份、计费和用量模型之上，继续调整流程、角色、集成、品牌和业务规则。

Herald 本身也采用 Claude Code、GLM 和 Codex 混合协作的 AI 辅助开发流程，开发工具基于 [web-dev-skills](https://github.com/timzaak/web-dev-skills) 构建。

[herald-app-example](https://github.com/timzaak/herald-app-example) 是一个完整的、全 AI 开发的 Flutter App，集成了 Herald 认证，可作为这种开发方式的参考实现。

## 快速开始

你需要 Python 3.12+、[uv](https://github.com/astral-sh/uv)、Docker、Cargo 和 npm。

```bash
git clone https://github.com/timzaak/herald.git
cd herald
uv run scripts/demo-start.py
```

启动完成后：

- 前端：http://localhost:3000
- 后端 API：http://localhost:8080

手动安装和后续配置请参阅[快速开始文档](https://www.fornetcode.com/zh/docs/getting-started)。

## 体验在线演示

打开 [auth.fornetcode.com](https://auth.fornetcode.com)，使用以下账号登录：

```text
邮箱：admin@fornetcode.com
密码：Herald@2026Admin
```

无需本地运行 Herald，即可先体验管理后台。

## AI Agent 专用入口

Herald 的文档以 MDX 编写，位于 [`docs-web/content/docs/`](docs-web/content/docs/)（英文）和 [`docs-web/content/docs/zh/`](docs-web/content/docs/zh/)（中文），发布于 https://www.fornetcode.com。AI agent 请优先使用下列结构化来源，而非抓取渲染后的 HTML：

- **完整文档索引（LLM）：** https://www.fornetcode.com/llms.txt
- **完整文档全文（LLM）：** https://www.fornetcode.com/llms-full.txt
- **单页 Markdown：** 在任意文档 URL 后加 `.md`，例如 https://www.fornetcode.com/zh/docs/auth-passkey.md
- **API 参考（OpenAPI）：** [`docs-web/openapi.json`](docs-web/openapi.json)
- **MCP 服务：** 用 Client API Key 直连，通过五个只读工具查询你的租户：

  ```bash
  claude mcp add --transport http herald https://your-herald-host/mcp \
    --header "X-API-Key: sk-your-api-key"
  ```

  连接后用零入参的 `get_realm_config_status` 工具做连通性自检。接入配置与工具说明见 [MCP 集成（AI Agent 接入）](https://www.fornetcode.com/zh/docs/mcp-integration)。

## 许可证

Herald 采用 [Apache-2.0](LICENSE) 许可证。你可以在商业产品中自由使用、修改和分发，开源项目不收取按用户计算的许可证费用。
