# 端到端工作流验收

仓库的 `../script/e2e-workflow.sh` 验证从需求到发布和运营索引的受保护 API 链路。它会创建真实项目及版本、写入审批记录和运行模型工作流；只应在已授权的隔离验收环境执行，不要对生产项目运行。

## 前置条件

- 应用和已迁移数据库正在运行，`/api/ready` 返回成功。
- 环境启用了可用的 LLM 配置，因为统一工作流会分析需求并生成 DSL。
- 本机安装 `curl` 和 `jq`。
- 准备三个不同用户的有效 JWT：项目创建者、独立复核者和发布审批者。脚本不尝试绕过 PoW 登录，也不会替你签发 token。
- 提供至少一条业务验收输入和期望输出。

## 运行

```sh
BASE_URL=http://127.0.0.1:3000 \
AUTH_TOKEN="$OWNER_JWT" AUTH_USERNAME=owner \
REVIEW_AUTH_TOKEN="$REVIEWER_JWT" REVIEW_USERNAME=reviewer \
PUBLISH_AUTH_TOKEN="$APPROVER_JWT" PUBLISH_USERNAME=approver \
TEST_CASES_JSON='[{"name":"满额折扣","inputJson":{"orderTotal":500},"expectedOutputJson":{"discountRate":0.95}}]' \
script/e2e-workflow.sh
```

脚本通过用户名和 token 进行角色独立性校验。不要把 token 写入脚本、提交到版本库或输出到共享日志。默认需求是订单满 500 元打 95 折；可用 `REQUIREMENT`、`PROJECT_NAME`、`PROJECT_DESCRIPTION` 和 `RISK_LEVEL` 调整场景。

## 澄清输入

脚本可能遇到 Agent 要求的必答澄清：

- `ANSWERS_JSON` 提供第一轮答案数组，格式为 `[{"clarificationId":"...","answer":"..."}]`。
- `ANSWERS_ROUNDS_JSON` 提供多轮答案数组，例如 `[[{"clarificationId":"...","answer":"..."}],[...]]`。
- 在交互式终端运行时，如果未提供后续答案，脚本会提示输入。非交互运行应预先提供所需轮次。

每轮澄清会携带服务端的 `expectedUpdatedAt`，最多自动推进 8 轮；仍需澄清时脚本失败并输出尚未回答的问题。

## 验收内容

脚本按顺序执行：创建项目、分配 reviewer/approver、SSE 需求分析及澄清、规划和计划确认、SSE DSL 生成、DSL 校验和编译、创建草稿版本、执行并审批业务验收用例、独立版本复核、技术与业务验证、发布、检查决策分析、生成冷启动优化建议、查询决策模式推荐并检查模型库索引。

任一步 HTTP 状态、业务状态或数据断言不符合预期都会以非零码退出。成功时会输出项目 ID 和版本 ID。失败时脚本临时文件会自动删除；按需在隔离环境的数据库和应用审计记录中核对已经完成的阶段。流程不会自动删除创建的项目或撤销已发布版本。

## 结果边界

该脚本验证当时部署版本上的端到端 API 和真实模型调用链路；运行结果受提示词、模型服务、网络和数据库状态影响。它不是确定性单元测试，也不能替代业务人员确认模型输出、版本回滚演练、真实负载测试或生产发布审批。

若只需测试固定 DSL、权限和镜像资产，可使用生产冒烟脚本；它们与本工作流脚本覆盖不同路径。详见[生产发布运行手册](production-release-runbook.md)。
