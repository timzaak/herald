export interface HomeTexts {
  badge: string;
  heroTitle: string;
  heroDesc: string;
  getStarted: string;
  liveDemo: string;
  terminal: {
    label: string;
    lines: {
      prefix?: string;
      text: string;
      status?: "ok" | "info" | "command";
    }[];
  };
  featureSectionTitle: string;
  featureSectionDesc: string;
  features: {
    title: string;
    desc: string;
    bullets: string[];
  }[];
  stepsSectionTitle: string;
  stepsSectionDesc: string;
  steps: {
    num: string;
    title: string;
    desc: string;
  }[];
  compareSectionTitle: string;
  compareSectionDesc: string;
  compareHeaders: {
    herald: string;
    auth0: string;
    supabase: string;
    keycloak: string;
  };
  compareRows: {
    label: string;
    herald: string;
    auth0: string;
    supabase: string;
    keycloak: string;
  }[];
  faqSectionTitle: string;
  faqSectionDesc: string;
  faq: {
    question: string;
    answer: string;
  }[];
  ctaTitle: string;
  ctaDesc: string;
  ctaContactHint: string;
  starGithub: string;
  footer: {
    copyright: string;
    contact: string;
    privacy: string;
    terms: string;
  };
}

export const en: HomeTexts = {
  badge: "Open source · Self-hosted · Rust + React",
  heroTitle: "The open-source backend for AI products",
  heroDesc:
    "Multi-tenant auth, billing, payments, credits, and an admin console, already wired together. Self-host it and every tenant's records sit in your own Postgres. Hand the repo to your AI coding tool instead of a blank starter.",
  getStarted: "Get Started",
  liveDemo: "Live Demo",
  terminal: {
    label: "terminal",
    lines: [
      {
        prefix: "$",
        text: "git clone https://github.com/timzaak/herald.git",
        status: "command",
      },
      { prefix: "$", text: "cd herald", status: "command" },
      { prefix: "$", text: "uv run scripts/demo-start.py", status: "command" },
      { text: "→ Starting PostgreSQL + Redis ...", status: "info" },
      { text: "✓ Database migrated", status: "ok" },
      { text: "✓ Multi-tenant auth  (RBAC, OAuth, OIDC, TOTP)", status: "ok" },
      {
        text: "✓ Billing & payments (Stripe, Creem, WeChat Pay)",
        status: "ok",
      },
      { text: "✓ Credits, quotas & usage ledgers", status: "ok" },
      { text: "✓ Admin console @ http://localhost:3000", status: "ok" },
    ],
  },
  featureSectionTitle: "What you get",
  featureSectionDesc:
    'Identity, payments, entitlements, and usage live in one codebase. Wiring like "a purchase grants credits, a refund revokes them" is code you can read and change.',
  features: [
    {
      title: "Multi-tenant auth",
      desc: "Isolated realms, each with its own users, roles, apps, and branding.",
      bullets: [
        "Email/password and one-time-code login",
        "Google, GitHub, Apple, WeChat, Discord, LDAP",
        "Passkeys, TOTP 2FA, OpenID Connect",
      ],
    },
    {
      title: "RBAC, orgs & client apps",
      desc: "Realm-level roles, organizations with their own membership, and app registration with scoped credentials.",
      bullets: [
        "Per-realm roles and org tiers",
        "Client App registration",
        "OAuth 2.0, device flow, cross-app SSO",
      ],
    },
    {
      title: "Billing & payments",
      desc: "Subscriptions, one-time purchases, and invoices, mapped to entitlements per client app.",
      bullets: [
        "Stripe, Creem (merchant of record), WeChat Pay",
        "App Store / Google Play in-app purchases",
        "Multi-currency pricing & invoices",
      ],
    },
    {
      title: "Points & credits",
      desc: "Prepaid balances that meter product usage, per user.",
      bullets: [
        "Top-ups, refunds, expiry",
        "Per-user ledgers & grants",
        "Rolling quota windows",
      ],
    },
    {
      title: "Consent & compliance",
      desc: "Versioned terms and privacy policies per realm, hosted or external.",
      bullets: [
        "Version-bound re-consent",
        "Hosted or external pages",
        "Auditable account deletion",
      ],
    },
    {
      title: "MCP server for AI agents",
      desc: "Agents query your realm through permission-gated tools instead of hand-written REST calls.",
      bullets: [
        "Nine read-only tools",
        "Browser OAuth + per-user RBAC",
        "Streamable HTTP at /mcp/{realmId}",
      ],
    },
  ],
  stepsSectionTitle: "From repository to AI product in three steps",
  stepsSectionDesc:
    "Deploy the foundation, shape it with AI, build your product logic.",
  steps: [
    {
      num: "01",
      title: "Deploy with Docker",
      desc: "Copy the deploy files from docker/deploy/, edit three configs, and run docker compose up -d. Herald, PostgreSQL, and Redis start on one machine; add Caddy when you want HTTPS.",
    },
    {
      num: "02",
      title: "Shape it with AI",
      desc: "Point an AI coding tool at the repo. The docs ship as llms.txt, the API as OpenAPI, and an MCP server exposes admin tools, so the agent works from real interfaces instead of guesses.",
    },
    {
      num: "03",
      title: "Build your product logic",
      desc: "Your app integrates through the Node, Rust, and browser SDKs and OAuth 2.0 endpoints, then keeps iterating on the experience customers pay for.",
    },
  ],
  compareSectionTitle: "Herald vs. the alternatives",
  compareSectionDesc:
    "Auth services stop at login. The table shows what else ships in the same Herald codebase.",
  compareHeaders: {
    herald: "Herald",
    auth0: "Auth0",
    supabase: "Supabase",
    keycloak: "Keycloak",
  },
  compareRows: [
    {
      label: "Multi-tenant auth",
      herald: "Included",
      auth0: "Enterprise only",
      supabase: "Manual setup",
      keycloak: "Included",
    },
    {
      label: "Subscription billing",
      herald: "Built-in",
      auth0: "No",
      supabase: "No",
      keycloak: "No",
    },
    {
      label: "Points & credits",
      herald: "Built-in",
      auth0: "No",
      supabase: "No",
      keycloak: "No",
    },
    {
      label: "Self-hosted",
      herald: "Yes",
      auth0: "Cloud service",
      supabase: "Yes",
      keycloak: "Yes",
    },
    {
      label: "Open source",
      herald: "Apache-2.0",
      auth0: "No",
      supabase: "Partial",
      keycloak: "Apache-2.0",
    },
  ],
  faqSectionTitle: "Frequently asked questions",
  faqSectionDesc: "What people usually ask before starting.",
  faq: [
    {
      question: "What is Herald?",
      answer:
        "An open-source, self-hosted backend for AI products. One Apache-2.0 codebase ships multi-tenant auth, RBAC, billing, payments, credits, and an admin console, and you keep adapting it with AI coding tools.",
    },
    {
      question: "How does Herald help me build faster?",
      answer:
        "The product itself takes days with AI tools; the parts users never see take weeks. Instead of stitching an auth provider, a payment provider, and a billing service, you start with those systems connected. When each provider has its own user IDs and webhooks, one dropped webhook can lock out a paying customer; in Herald those connections are one codebase.",
    },
    {
      question: "How is Herald different from an auth provider?",
      answer:
        "An auth provider stops at identity. Herald also handles subscriptions, one-time payments, entitlements, credits, invoices, and tenant administration, so paid access and product usage share one foundation.",
    },
    {
      question: "When is a hosted service the better choice?",
      answer:
        "If you only need login plus card payments and don't mind per-user pricing, a hosted auth service is less work. Herald pays off when you want tenant-level control, credit-based pricing, or user data in your own database.",
    },
    {
      question: "What does multi-tenant mean in Herald?",
      answer:
        "Herald organizes your users and data into isolated realms. Each realm is a separate tenant with its own users, OAuth providers, client apps, and billing plans. Data between realms is fully isolated.",
    },
    {
      question: "How do I deploy Herald?",
      answer:
        "Docker Compose runs three containers: the Herald app, PostgreSQL, and Redis. You need a Linux server (Ubuntu 22.04+, 2GB RAM), Docker Engine 24+, and a domain. Add Caddy as a reverse proxy when you want HTTPS; it fetches and renews certificates automatically.",
    },
    {
      question: "What payment providers does Herald support?",
      answer:
        "Stripe, Creem, and WeChat Pay, plus App Store and Google Play in-app purchases. Creem acts as merchant of record and handles tax compliance. You create plans, map them to providers, and assign them to client apps.",
    },
    {
      question: "What tech stack does Herald use?",
      answer:
        "Herald uses Rust (Axum framework) for the backend API and React with TypeScript for the frontend. Data is stored in PostgreSQL with SeaORM, and Redis handles sessions and caching.",
    },
    {
      question: "Is Herald free and open source?",
      answer:
        "Yes. Herald is released under the Apache-2.0 license. You can use, modify, and distribute it freely, including for commercial projects, with no per-user license fee.",
    },
    {
      question: "Can I get help evaluating or deploying Herald?",
      answer:
        "Yes. Email zsy.evan@gmail.com and tell us about your product; we can help with deployment, payment-provider setup, and architecture questions. Bugs and feature requests can go to the GitHub issue tracker.",
    },
  ],
  ctaTitle: "Build the AI product users pay for",
  ctaDesc:
    "Create a realm on the live demo and look around first, or clone the repo and run it yourself. Apache-2.0, no per-user fee, user data in your own database.",
  ctaContactHint: "Questions, or building something with Herald? Email us:",
  starGithub: "Star on GitHub",
  footer: {
    copyright: "Herald · Apache 2.0",
    contact: "Contact",
    privacy: "Privacy",
    terms: "Terms",
  },
};

export const zh: HomeTexts = {
  badge: "开源 · 自托管 · Rust + React",
  heroTitle: "AI 产品的开源后端",
  heroDesc:
    "多租户认证、计费、支付、积分和管理后台，接线已经完成。自己部署，租户数据都在你自己的 Postgres 里；把仓库交给 AI 编程工具，从能跑的产品开始改。",
  getStarted: "快速开始",
  liveDemo: "在线演示",
  terminal: {
    label: "terminal",
    lines: [
      {
        prefix: "$",
        text: "git clone https://github.com/timzaak/herald.git",
        status: "command",
      },
      { prefix: "$", text: "cd herald", status: "command" },
      { prefix: "$", text: "uv run scripts/demo-start.py", status: "command" },
      { text: "→ 正在启动 PostgreSQL + Redis ...", status: "info" },
      { text: "✓ 数据库迁移完成", status: "ok" },
      { text: "✓ 多租户认证 (RBAC, OAuth, OIDC, TOTP)", status: "ok" },
      { text: "✓ 计费与支付 (Stripe, Creem, 微信支付)", status: "ok" },
      { text: "✓ 积分、配额与用量账本", status: "ok" },
      { text: "✓ 管理后台 @ http://localhost:3000", status: "ok" },
    ],
  },
  featureSectionTitle: "开箱即用的能力",
  featureSectionDesc:
    "身份、支付、权益和用量在同一套代码里。「购买发放积分、退款收回权益」这类接线是你读得到、改得动的代码。",
  features: [
    {
      title: "多租户认证",
      desc: "相互隔离的 Realm，每个 Realm 有独立的用户、角色、应用和品牌。",
      bullets: [
        "邮箱密码与邮箱验证码登录",
        "Google、GitHub、Apple、微信、Discord、LDAP",
        "Passkey、TOTP 双因素、OpenID Connect",
      ],
    },
    {
      title: "RBAC、组织与客户端应用",
      desc: "Realm 级角色，带独立成员的组织，以及注册后获得受限凭证的应用。",
      bullets: [
        "Realm 级角色与组织层级",
        "客户端应用注册",
        "OAuth 2.0、设备授权、跨应用单点登录",
      ],
    },
    {
      title: "计费与支付",
      desc: "订阅、一次性购买和发票，按客户端应用映射到权益。",
      bullets: [
        "Stripe、Creem（Merchant of Record）、微信支付",
        "App Store / Google Play 内购",
        "多币种定价与发票",
      ],
    },
    {
      title: "积分与信用点",
      desc: "按用户计量产品用量的预付余额。",
      bullets: ["充值、退款、过期", "用户账本与积分发放", "多时间窗滚动配额"],
    },
    {
      title: "知情同意与合规",
      desc: "按 Realm 版本化的用户协议与隐私政策，可托管也可外链。",
      bullets: ["版本绑定的重新同意", "托管或外部页面", "可审计的账户注销"],
    },
    {
      title: "面向 AI Agent 的 MCP 服务器",
      desc: "Agent 通过受权限约束的工具查询你的租户，无需手写 REST 调用。",
      bullets: [
        "九个只读工具",
        "浏览器 OAuth + 用户级 RBAC",
        "/mcp/{realmId} Streamable HTTP",
      ],
    },
  ],
  stepsSectionTitle: "三步从代码仓库到 AI 产品",
  stepsSectionDesc: "部署底座，用 AI 改造，构建产品逻辑。",
  steps: [
    {
      num: "01",
      title: "Docker 部署",
      desc: "把 docker/deploy/ 下的部署文件拷到服务器，改三个配置，运行 docker compose up -d。Herald、PostgreSQL 和 Redis 在同一台机器上启动；需要 HTTPS 时再加 Caddy。",
    },
    {
      num: "02",
      title: "用 AI 改造",
      desc: "把 AI 编程工具指向这个仓库。文档以 llms.txt 提供，API 有 OpenAPI 描述，MCP 服务暴露管理工具，agent 面对的是真实接口，不是猜测。",
    },
    {
      num: "03",
      title: "构建产品逻辑",
      desc: "你的应用通过 Node、Rust 和浏览器 SDK 以及 OAuth 2.0 端点接入，然后持续迭代用户愿意付费的体验。",
    },
  ],
  compareSectionTitle: "Herald 与常见方案对比",
  compareSectionDesc:
    "认证服务止步于登录。这张表列出同一套 Herald 代码里还带了什么。",
  compareHeaders: {
    herald: "Herald",
    auth0: "Auth0",
    supabase: "Supabase",
    keycloak: "Keycloak",
  },
  compareRows: [
    {
      label: "多租户认证",
      herald: "内置",
      auth0: "企业版",
      supabase: "需手动配置",
      keycloak: "内置",
    },
    {
      label: "订阅计费",
      herald: "内置",
      auth0: "否",
      supabase: "否",
      keycloak: "否",
    },
    {
      label: "积分与信用点",
      herald: "内置",
      auth0: "否",
      supabase: "否",
      keycloak: "否",
    },
    {
      label: "自托管",
      herald: "支持",
      auth0: "云服务",
      supabase: "支持",
      keycloak: "支持",
    },
    {
      label: "开源",
      herald: "Apache-2.0",
      auth0: "否",
      supabase: "部分",
      keycloak: "Apache-2.0",
    },
  ],
  faqSectionTitle: "常见问题",
  faqSectionDesc: "开始之前大家通常会问的事。",
  faq: [
    {
      question: "Herald 是什么？",
      answer:
        "一个开源、自托管的 AI 产品后端。一套 Apache-2.0 代码提供多租户认证、RBAC、计费、支付、积分和管理后台，可以持续用 AI 编程工具改造。",
    },
    {
      question: "Herald 怎么帮我更快构建产品？",
      answer:
        "产品本体用 AI 工具几天就能做出来，用户看不见的部分要耗几周。与其拼接认证、支付、计费三家服务，不如从已连接的系统开始。每家服务有自己的用户 ID 和 webhook，一次 webhook 丢失就可能让付费用户失去访问；在 Herald 里这些连接是同一套代码。",
    },
    {
      question: "Herald 和单纯的认证服务有什么不同？",
      answer:
        "认证服务止步于身份。Herald 还处理订阅、一次性支付、权益、积分、发票和租户管理，让付费访问和产品用量共用同一套底座。",
    },
    {
      question: "什么时候托管服务是更好的选择？",
      answer:
        "如果你只需要登录加刷卡，也不在意按用户数付费，托管认证服务更省事。需要租户级控制、按积分计价，或者要求用户数据放在自己的数据库里时，Herald 才划算。",
    },
    {
      question: "Herald 中的多租户是什么意思？",
      answer:
        "Herald 把用户和数据组织到相互隔离的 Realm 中。每个 Realm 是独立的租户，有自己的用户、OAuth 提供商、客户端应用和计费计划。Realm 之间的数据完全隔离。",
    },
    {
      question: "如何部署 Herald？",
      answer:
        "Docker Compose 跑三个容器：Herald 应用、PostgreSQL 和 Redis。你需要一台 Linux 服务器（Ubuntu 22.04+，2GB 内存）、Docker Engine 24+ 和一个域名。需要 HTTPS 时加 Caddy 做反向代理，证书自动申请和续期。",
    },
    {
      question: "Herald 支持哪些支付提供商？",
      answer:
        "Stripe、Creem 和微信支付，另有 App Store / Google Play 内购。Creem 作为 Merchant of Record 负责税务合规。你创建订阅计划，映射到支付提供商，再分配给客户端应用。",
    },
    {
      question: "Herald 使用什么技术栈？",
      answer:
        "后端用 Rust（Axum 框架）提供 API，前端是 React + TypeScript。数据存在 PostgreSQL（SeaORM），Redis 负责会话与缓存。",
    },
    {
      question: "Herald 免费开源吗？",
      answer:
        "是。Herald 采用 Apache-2.0 许可证发布，可以自由使用、修改和分发，包括商业项目，不收按用户数计算的许可证费用。",
    },
    {
      question: "评估或部署 Herald 时可以获得帮助吗？",
      answer:
        "可以。发邮件到 zsy.evan@gmail.com 说明你的产品场景，我们可以协助部署、支付提供商接入和架构问题。Bug 与功能需求也可以提交 GitHub Issue。",
    },
  ],
  ctaTitle: "构建用户愿意付费的 AI 产品",
  ctaDesc:
    "先在在线演示开个 Realm 逛一圈，或者克隆仓库自己跑。Apache-2.0、不按用户数收费、用户数据在自己的数据库里。",
  ctaContactHint: "有疑问，或正在基于 Herald 构建产品？欢迎邮件联系：",
  starGithub: "Star on GitHub",
  footer: {
    copyright: "Herald · Apache 2.0",
    contact: "联系我们",
    privacy: "隐私政策",
    terms: "服务条款",
  },
};
