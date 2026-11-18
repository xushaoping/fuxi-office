# API 参考

API 以 JSON 为主，基础路径为 `/api`。受保护的接口需要登录后取得的 JWT：

```http
Authorization: Bearer <JWT>
Content-Type: application/json
```

浏览器登录使用 `POST /api/login`，登录前通过 `GET /api/captcha` 获取一次性 PoW challenge；不要关闭或绕过登录保护。项目接口还会按成员角色执行权限校验。管理员接口须使用管理员账户。公开决策服务接口使用服务专用 `X-Decision-Key`，不接受应用 JWT。

路径中的 `{project_id}`、`{version_id}` 等为服务返回的资源 ID。接口通常以 JSON 返回业务错误；HTTP 状态码和响应字段应以部署版本的响应为准。长耗时工作流接口采用 Server-Sent Events (SSE)，客户端需发送 `Accept: text/event-stream` 并读取终止事件。

## 健康和认证

| 方法 | 路径 | 访问 | 用途 |
| --- | --- | --- | --- |
| `GET` | `/api/health` | 公开 | 进程存活探测。数据库故障时仍可能返回成功。 |
| `GET` | `/api/ready` | 公开 | 数据库及部署基线就绪探测；不就绪时返回 503。 |
| `GET` | `/api/captcha` | 公开 | 获取登录 PoW challenge。 |
| `POST` | `/api/login` | 公开 | 校验凭据及 challenge 并签发会话令牌。 |
| `GET` | `/api/me` | JWT | 获取当前登录用户。 |

## 项目和成员

| 方法 | 路径 | 用途 |
| --- | --- | --- |
| `GET`, `POST` | `/api/projects` | 查询可访问项目；创建项目。 |
| `GET`, `PUT`, `DELETE` | `/api/projects/{project_id}` | 读取、更新或删除项目。 |
| `GET` | `/api/projects/{project_id}/members` | 查询项目成员。 |
| `PUT`, `DELETE` | `/api/projects/{project_id}/members/{username}` | 设置或移除项目成员角色。 |
| `GET`, `POST` | `/api/projects/{project_id}/versions` | 查询版本或创建草稿版本。 |

角色权限由项目访问控制实施。复核与发布流程还会校验操作者是否满足独立性规则；不要把拥有多个角色等同于可跳过独立复核。

## 建模工作流

| 方法 | 路径 | 用途 |
| --- | --- | --- |
| `GET` | `/api/projects/{project_id}/workflow` | 读取当前工作流状态。 |
| `POST` | `/api/projects/{project_id}/workflow/orchestrate-stream` | 按已保存状态推进统一工作流，覆盖需求分析、澄清、计划和 DSL 生成。 |
| `POST` | `/api/projects/{project_id}/workflow/analyze-stream` | 流式分析需求。 |
| `POST` | `/api/projects/{project_id}/workflow/analyze` | 非流式需求分析。 |
| `POST` | `/api/projects/{project_id}/workflow/clarify` | 提交澄清答案。 |
| `POST` | `/api/projects/{project_id}/workflow/plan` | 生成决策计划。 |
| `POST` | `/api/projects/{project_id}/workflow/plan-stream` | 流式生成决策计划。 |
| `POST` | `/api/projects/{project_id}/workflow/confirm` | 确认计划并推进人工门禁。 |
| `GET` | `/api/projects/{project_id}/agent-runs` | 查看 Agent 调用历史。 |
| `GET` | `/api/projects/{project_id}/orchestrations` | 查询编排历史。 |
| `GET` | `/api/projects/{project_id}/orchestrations/{orchestration_id}/events` | 按游标回放编排事件。 |
| `POST` | `/api/projects/{project_id}/orchestrations/{orchestration_id}/cancel` | 请求取消编排。 |

工作流更新使用 `expectedUpdatedAt` 等并发版本字段时，应从最近一次读取或 SSE 结果中带回该值；状态冲突后刷新工作流，不要盲目重放生成请求。

## DMN 编译和执行

以下操作接口需要 JWT：

| 方法 | 路径 | 用途 |
| --- | --- | --- |
| `POST` | `/api/validate` | 校验 DSL。 |
| `POST` | `/api/compile` | 将 DSL 编译为 DMN XML。 |
| `POST` | `/api/validate-dmn` | 校验 DMN XML。 |
| `POST` | `/api/analyze-inputs` | 分析 DMN 服务输入。 |
| `POST` | `/api/evaluate` | 对模型求值。 |
| `POST` | `/api/generate` | 调用模型生成 DSL。 |
| `POST` | `/api/generate-change-intent` | 将修改请求转换为结构化变更意图。 |
| `POST` | `/api/run` | 运行组合式建模处理管线。 |

请求体字段因接口而异；常见字段包括 `dsl`、`xml` 和执行输入对象。具体格式可在 `src/api.rs` 的请求类型和 handler 中核对。对外集成优先使用已发布决策服务，而非生成或调试接口。

## 版本、测试和发布

| 方法 | 路径 | 用途 |
| --- | --- | --- |
| `POST` | `/api/projects/{project_id}/versions/{version_id}/test-cases` | 创建业务验收用例。 |
| `GET` | `/api/projects/{project_id}/versions/{version_id}/test-cases` | 查询验收用例。 |
| `POST` | `/api/projects/{project_id}/versions/{version_id}/test-cases/run` | 执行验收用例。 |
| `PUT` | `/api/projects/{project_id}/versions/{version_id}/test-cases/{case_id}/approval` | 记录业务用例审批。 |
| `POST` | `/api/projects/{project_id}/versions/{version_id}/regression-tests/run` | 运行历史回归。 |
| `POST` | `/api/projects/{project_id}/versions/{version_id}/verification` | 执行版本技术及业务验证。 |
| `GET` | `/api/projects/{project_id}/versions/{version_id}/review` | 查看复核状态。 |
| `POST` | `/api/projects/{project_id}/versions/{version_id}/review-approval` | 记录独立版本复核。 |
| `POST` | `/api/projects/{project_id}/versions/{version_id}/publish` | 发布通过门禁的版本。 |
| `POST` | `/api/projects/{project_id}/versions/{version_id}/rollback` | 回退当前发布版本。 |

生成的 Tester 提案有独立 judge、转换和执行接口；自动生成的测试不能取代业务验收及独立审批。发布前应通过版本验证与配置要求的历史回归。

## 决策服务

| 方法 | 路径 | 访问 | 用途 |
| --- | --- | --- | --- |
| `GET` | `/api/projects/{project_id}/decision-services` | JWT | 列出已发布服务。 |
| `POST` | `/api/projects/{project_id}/decision-service-keys` | JWT | 创建范围受限的服务调用凭证。 |
| `DELETE` | `/api/projects/{project_id}/decision-service-keys/{key_id}` | JWT | 撤销服务调用凭证。 |
| `POST` | `/api/projects/{project_id}/decision-services/execute` | JWT | 在项目上下文中执行已发布服务。 |
| `POST` | `/api/public/projects/{project_id}/decision-services/execute` | `X-Decision-Key` | 面向外部调用方执行已发布服务。 |
| `GET` | `/api/projects/{project_id}/decision-runs` | JWT | 查询执行记录。 |
| `PUT` | `/api/projects/{project_id}/decision-runs/{run_id}/outcome` | JWT | 回传单次决策业务结果。 |
| `POST` | `/api/public/projects/{project_id}/decision-runs/{run_id}/outcome` | `X-Decision-Key` | 外部回传单次业务结果。 |
| `POST` | `/api/public/projects/{project_id}/decision-runs/outcomes:batch` | `X-Decision-Key` | 批量回传业务结果。 |

服务密钥仅应在创建响应时安全保存，泄露后立即撤销并重新签发。公开运行路径只暴露经发布的版本。

## 知识、分析和管理接口

- 知识源：`/api/knowledge-sources` 管理账户级知识源；`/api/projects/{project_id}/knowledge-sources` 管理项目关联；`/api/projects/{project_id}/knowledge-search-debug?q=...` 提供受权限保护的检索诊断。
- 决策运营：`/api/projects/{project_id}/decision-analytics`、`/api/projects/{project_id}/drift-events`、`/api/projects/{project_id}/deviation-analyses`、`/api/projects/{project_id}/optimization-proposals` 和 `/api/projects/{project_id}/copilot/query`。
- 模型库：`/api/model-library`、`/api/model-library/recommendations`、`/api/model-library/analytics`。
- 管理员用户与指标：`/api/admin/users`、`/api/admin/model-performance`、`/api/admin/model-usage`、`/api/admin/model-experiments`、`/api/admin/business-quality`。
- 提示词注册表：`/api/admin/prompts` 及其评测、激活、回退和事件子路径，仅限管理员访问。

项目运维路由完整列表以 `src/projects.rs` 和 `src/workflow_api.rs` 的路由定义为准。敏感运维接口统一依赖 JWT、管理员检查及项目权限，不应通过反向代理将它们改造成匿名接口。
