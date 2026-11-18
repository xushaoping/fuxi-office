# 伏羲智能决策工作台

伏羲是一款全栈 Rust 构建的企业级 DMN 决策自动化平台。用户以自然语言描述业务规则，平台通过多智能体协作生成结构化决策 DSL，并经多层校验编译为符合 DMN 1.5 标准的 XML 模型；同时覆盖版本管理、业务验收、发布运行与数据分析，形成完整的决策治理闭环。

**线上演示**：[https://fuxioffice.cn/](https://fuxioffice.cn/)

## 版本状态

- **当前版本**: v1.0.0 (release)
- **智能化成熟度**: L3（半自动闭环）
- **详细变更**: [v1.0.0 变更日志](docs/CHANGELOG_v1.0.0.md) 

**注意**: 工作区包含未提交变更，生产部署前需完成完整验收。

---

## 核心特性

### 🤖 智能建模
- **多智能体协作**: Analyst、Architect、Engineer、Tester 并行工作，心跳监控与失联恢复
- **自然语言驱动**: 无需学习 DSL/DMN/FEEL，直接用业务语言描述
- **RAG 知识增强**: 向量检索 + 关键词检索 + RRF 融合，自动注入相关上下文
- **阶段化工作流**: 需求分析 → 澄清 → 方案 → 生成，每步可控可验证
- **增量修改**: 结构化补丁保留未受影响的元素

### ⚙️ 决策引擎
- **DMN 1.5 标准**: 完整支持决策表、DRD、上下文、FEEL 表达式
- **原生 Rust 实现**: 无 JVM 依赖，支持 WASM 客户端执行
- **执行轨迹**: 结构化记录每条规则的匹配结果和中间值

### ✅ 质量保障
- **多层校验门禁**: DSL → DMN 编译 → FEEL 语法 → 依赖完整性 → 契约测试
- **测试工程 Agent**: 自动生成测试提案，AI 辅助复核（LLM-as-Judge）
- **历史回归**: 有基线的版本必须通过历史回归才能发布
- **静态分析**: 规则冲突、不可达规则、覆盖缺口检测

### 📊 生产运营
- **版本化部署**: 不可变版本历史，自动退役旧版本
- **决策解释**: 按需生成自然语言解释，支持盲审标注
- **业务反馈闭环**: 批量回传业务结果 + 智能偏差归因 + 人工复核
- **漂移检测**: 每日扫描失败率/偏差率/输入分布，事件驱动触发优化
- **运营 Copilot**: 自然语言查询项目指标
- **优化闭环**: 自动生成建议 → 历史样本模拟 → Shadow 评估 → 效果追踪

### 👥 协作与治理
- **多角色权限**: 查看者、建模者、管理员，项目级隔离
- **版本审阅**: 风险分级、差异分析、人工确认
- **提示词版本管理**: 版本控制、Golden 评测、激活回滚
- **审计日志**: 完整的操作审计与回滚记录

---

## 技术架构

### 技术栈

- **后端**: Rust 1.90 / Axum 0.8 / SQLx 0.8
- **前端**: Leptos 0.8 CSR（客户端渲染）
- **数据库**: PostgreSQL 14+ / pgvector
- **标准**: DMN 1.5 / FEEL 1.3

### 关键模块

```
src/
├── agent_runtime/          多智能体协作编排
├── rag/                    RAG 语义检索（分块、向量、混合检索）
├── dmn/                    DMN 引擎（解析、编译、运行时）
├── feel/                   FEEL 表达式引擎
├── nl_dsl/                 自然语言 → DSL 转换
├── storage/                数据存储（分区、物化视图）
├── api.rs                  REST API
└── components/             Leptos 前端组件
```

---

## 快速开始

### 前置条件

- Rust 1.90+（需启用 `wasm32-unknown-unknown` target）
- PostgreSQL 14+（需启用 pgvector 扩展）
- Docker Compose（可选，用于容器化部署）

### 本地开发

#### 1. 环境配置

创建 `.env` 文件（参考 `.env.example`）：

```bash
# 核心配置
DATABASE_URL=postgresql://fuxi:password@localhost:5432/fuxi_dmn
JWT_SECRET=your-32-char-random-secret-here
ADMIN_DEFAULT_PASSWORD=admin123

# LLM 配置（至少配置一个）
OPENAI_API_KEY=sk-...
ANTHROPIC_API_KEY=sk-ant-...

# 可选配置
SERVER_ADDR=127.0.0.1:3000
ENVIRONMENT=development
```

完整配置说明见 [配置文档](docs/configuration.md)。

#### 2. 数据库初始化

```bash
# 创建数据库（三角色权限隔离）
POSTGRES_USER=postgres POSTGRES_DB=fuxi_dmn ./script/bootstrap-production-db.sh

# 验证权限基线
./script/check-deployment-baseline.sh
```

#### 3. 启动应用

```bash
# 安装依赖
cargo build --release

# 启动后端
cargo run --release

# 前端已编译到 WASM，随后端一起启动
# 访问 http://127.0.0.1:3000
```

### Docker 部署

```bash
# 启动所有服务（PostgreSQL + 应用）
docker-compose up -d

# 查看日志
docker-compose logs -f fuxi-dmn

# 访问 http://localhost:3000
```

详细部署说明见 [部署文档](docs/deployment.md)。

---

## 核心工作流

### 1. 创建决策项目

```bash
# 登录（默认用户名/密码：admin / admin123）
# 创建项目 → 输入项目名称和描述
```

### 2. 自然语言建模

```bash
# 进入项目 → 点击"新建版本"
# 输入自然语言需求，例如：
"信用评分决策：
1. 如果信用分数 >= 700，批准贷款
2. 如果信用分数 < 700 且收入 >= 50000，人工审核
3. 否则拒绝"

# 系统自动执行：需求分析 → 澄清 → 方案设计 → 生成 DSL → 编译 DMN
```

### 3. 测试与验证

```bash
# 自动生成测试提案 → 人工审阅 → 批准或修改
# 添加业务验收用例 → 运行所有测试 → 全部通过后发布
```

### 4. 发布与运行

```bash
# 创建决策服务 → 获取 API Key
# 调用决策服务：
curl -X POST https://fuxioffice.cn/api/public/decision-services/{service_id}/run \
  -H "Authorization: Bearer {api_key}" \
  -H "Content-Type: application/json" \
  -d '{"creditScore": 750, "income": 60000}'
```

### 5. 监控与优化

```bash
# 查看运行监控 → 分析失败/偏差
# 系统自动检测漂移 → 生成优化建议
# 历史样本模拟 → Shadow 评估 → 确认后创建新版本
```

完整操作流程见 [用户手册](docs/user-operation-manual.md)。

---

## API 概览

### 核心 API

| 分类 | 端点示例 | 说明 |
|------|---------|------|
| 认证 | `POST /api/auth/login` | 用户登录 |
| 项目 | `GET/POST /api/projects` | 项目管理 |
| 版本 | `GET/POST /api/projects/{id}/versions` | 版本管理 |
| 工作流 | `POST /api/projects/{id}/workflows` | 启动建模工作流 |
| 测试 | `GET/POST /api/projects/{id}/test-cases` | 测试用例管理 |
| 发布 | `POST /api/projects/{id}/versions/{vid}/publish` | 发布版本 |
| 服务 | `POST /api/public/decision-services/{sid}/run` | 决策服务调用 |

### 新增 API（v0.2.0）

| 分类 | 端点 | 说明 |
|------|------|------|
| 编排 | `GET /api/projects/{id}/orchestrations` | 查看智能体编排历史 |
| 解释 | `POST /api/projects/{id}/decision-runs/{rid}/explanation` | 生成决策解释 |
| 归因 | `GET /api/projects/{id}/deviation-analyses` | 查看偏差归因 |
| 漂移 | `GET /api/projects/{id}/drift-events` | 查看漂移事件 |
| 优化 | `GET /api/projects/{id}/optimization-proposals` | 查看优化建议 |
| Copilot | `POST /api/projects/{id}/copilot/query` | 自然语言查询指标 |
| 提示词 | `GET /api/admin/prompts` | 提示词版本管理 |

完整 API 文档见 [API 参考](docs/api-reference.md)。

---

## 评测与验收

平台提供完整的评测工具链：

```bash
# RAG 检索评测
python3 script/eval-rag.py labeled-knowledge.tsv

# Agent 编排评测
python3 script/eval-agent-orchestrations.py <project_id>

# 偏差归因评测
python3 script/eval-deviation-reviews.py deviation-reviews.jsonl

# 漂移检测评测
python3 script/eval-drift.py drift-windows.jsonl

# 决策解释评测
python3 script/eval-decision-explanation-reviews.py explanation-reviews.jsonl
```

所有评测工具提供 Wilson 95% 置信区间、小样本警告、数据质量检查。详见 [评测文档](docs/evaluation.md)。

---

## 常见问题

### 1. 数据库连接失败
```bash
# 检查 PostgreSQL 是否启动
psql -U postgres -d fuxi_dmn -c 'SELECT 1'

# 检查 DATABASE_URL 配置
echo $DATABASE_URL

# 验证角色权限
./script/check-deployment-baseline.sh
```

### 2. LLM API 调用失败
```bash
# 检查 API Key 配置
echo $OPENAI_API_KEY

# 测试连接
curl https://api.openai.com/v1/models \
  -H "Authorization: Bearer $OPENAI_API_KEY"
```

### 3. 前端无法访问
```bash
# 检查后端是否启动
curl http://127.0.0.1:3000/api/health

# 检查 WASM 是否编译
ls target/wasm32-unknown-unknown/release/*.wasm
```

### 4. 编排任务失联
```bash
# 查看编排历史
curl http://127.0.0.1:3000/api/projects/{id}/orchestrations

# 手动取消任务
curl -X POST http://127.0.0.1:3000/api/projects/{id}/orchestrations/{oid}/cancel
```

更多问题见 [故障排查文档](docs/troubleshooting.md)。

---

## 文档导航

### 用户文档
- [用户操作手册](docs/user-operation-manual.md) - 完整业务操作流程
- [端到端工作流](docs/e2e-workflow.md) - 从创建到发布的完整示例
- [知识库示例](docs/knowledge-base-examples.md) - 知识库录入与使用

### 技术文档
- [v1.0.0 变更日志](docs/CHANGELOG_v1.0.0.md) - 详细变更记录
- [配置文档](docs/configuration.md) - 完整环境变量说明
- [API 参考](docs/api-reference.md) - 完整 API 文档
- [部署文档](docs/deployment.md) - 生产部署指南
- [数据库权限](docs/production-database-permissions.md) - 三角色权限隔离
- [评测文档](docs/evaluation.md) - 评测工具使用说明
- [故障排查](docs/troubleshooting.md) - 常见问题与解决方案

### 设计文档
- [平台设计方案](docs/intelligent-decision-platform-design.md) - 整体架构设计
- [DMN 规范说明](docs/dmn-spec-notes.md) - DMN 1.5 实现细节

### 开发文档
- [贡献指南](CONTRIBUTING.md) - 开发环境与提交规范
- [安全策略](SECURITY.md) - 漏洞报告与处理
- [行为准则](CODE_OF_CONDUCT.md)

---

## 技术亮点

- ✅ **全栈 Rust**: 后端 Axum + 前端 Leptos CSR，类型安全端到端
- ✅ **多智能体协作**: 并行执行、心跳监控、失联恢复、取消传播
- ✅ **RAG 语义检索**: 向量索引 + 关键词检索 + RRF 融合
- ✅ **完整闭环**: 模拟 → Shadow → 效果追踪 → 自动草稿
- ✅ **生产级质量**: 多层门禁、历史回归、三角色权限隔离
- ✅ **可观测性**: 分区存储、物化视图、完整评测工具链

---

## 许可证

本项目自有代码以 **MIT License** 发布，详见 [LICENSE](LICENSE)。  
`resource/` 下的第三方 DMN 示例可能适用独立许可证，详见 [NOTICE](NOTICE)。

---

**伏羲智能决策工作台** - 让业务规则建模像对话一样简单

🌐 [线上演示](https://fuxioffice.cn/) | 📧 联系我们 | 💬 [问题反馈](https://github.com/fuxi-office/dmn/issues)
