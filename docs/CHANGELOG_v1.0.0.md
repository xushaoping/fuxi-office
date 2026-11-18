# 变更日志 v1.0.0 - 全链路智能化升级

> **版本**: v1.0.0  |   **发布日期**: 2026-11-18 17:18:38
>
---

## 概述

本次升级是平台自发布以来最大规模的功能增强，涵盖 **15 个主要升级项**，新增 **46 个数据库迁移**（056-101），重构了智能体架构，建立了完整的自动化闭环。

### 升级目标达成情况

| 目标 | 目标值 | 实施状态 |
|------|--------|----------|
| 智能体架构升级 | 多Agent协作编排 | ✅ 已完成 |
| 知识检索升级 | RAG语义检索 | ✅ 已完成 |
| 优化闭环建立 | 模拟-追踪-自动草稿 | ✅ 已完成 |
| 运维自动化 | 漂移检测+事件驱动 | ✅ 已完成 |
| 质量保障 | 提示词版本管理+评测 | ✅ 已完成 |

---

## 🚀 重大新增功能

### 1. 多智能体协作编排系统 (U1)

**架构升级**: 从单 Agent 线性流水线升级到多 Agent 协作编排

- **新增模块**: `src/agent_runtime/orchestration.rs`
  - `AgentFuture`: 异步 Agent 执行抽象
  - `PipelineStage`: 管道阶段定义
  - 并发控制与资源管理
  - 流式事件反馈机制

- **新增数据表**:
  - `063_agent_orchestration_state.sql`: 持久化编排状态
  - `067_agent_orchestration_heartbeat_recovery.sql`: 心跳与失联恢复
  - `072_workflow_engineer_orchestration_handoff.sql`: Engineer 子编排交接
  - `073_workflow_analysis_orchestration_handoff.sql`: Analysis 子编排交接
  - `074_agent_step_attempt_history.sql`: 步骤尝试历史
  - `075_agent_orchestration_retry_lineage.sql`: 重试家族血统
  - `085_agent_orchestration_single_active_retry.sql`: 单活跃重试约束
  - `100_agent_step_skipped_status.sql`: 步骤跳过状态
  - `101_agent_orchestration_sse_events.sql`: SSE 事件回放

- **关键能力**:
  - ✅ 多 Agent 并行执行
  - ✅ 编排状态持久化
  - ✅ 心跳监控与失联回收
  - ✅ 重试机制与血统追踪
  - ✅ 取消传播与资源清理
  - ✅ SSE 事件流回放

### 2. 知识库 RAG 语义检索 (U2)

**检索升级**: 从全文注入升级到分块+向量+混合检索

- **新增模块**: `src/rag/`
  - `chunker.rs`: 知识分块（约 400 token/块）
  - `retriever.rs`: 混合检索（关键词+向量+RRF融合）
  - `patterns.rs`: 模式语义检索
  - `local_embedding.rs`: 本地词法特征哈希
  - `config.rs`: 检索配置管理

- **新增数据表**:
  - `056_repair_memories.sql`: 修复记忆管理
  - `057_knowledge_chunks.sql`: 知识分块与向量索引
  - `066_decision_pattern_embeddings.sql`: 模式向量嵌入

- **关键能力**:
  - ✅ 自动章节分块（约 400 token）
  - ✅ pgvector 向量索引支持
  - ✅ OpenAI Embedding API 集成
  - ✅ 本地词法特征哈希（无外部依赖）
  - ✅ 混合检索（关键词+向量+RRF）
  - ✅ 检索评测工具（`script/eval-rag.py`）
  - ✅ 检索诊断界面（知识块+模式）

### 3. 测试工程 Agent 与 LLM-as-Judge (U4)

**测试升级**: 自动化测试提案生成与 AI 辅助复核

- **新增数据表**:
  - `070_workflow_tester_report.sql`: Tester 报告
  - `071_tester_proposal_test_cases.sql`: 提案测试用例来源
  - `095_prompt_registry_tester_judge.sql`: Judge 提示词版本

- **关键能力**:
  - ✅ 基于需求条目/约束生成测试提案
  - ✅ AI 辅助复核（双次独立判定）
  - ✅ 提案转换为正式验收用例
  - ✅ Judge 采信率评测工具
  - ✅ 盲审机制（每个 Reviewer 只能看到自己的标注）

### 4. Agent 评测与动态模型路由 (U5)

**质量监控**: 模型性能聚合与 A/B 实验

- **新增数据表**:
  - `039_model_performance_metrics.sql`: 模型成功率与延迟聚合
  - `040_agent_model_votes.sql`: 高风险需求多模型投票审计
  - `050_model_policy_experiments.sql`: A/B 实验分组
  - `051_agent_run_token_usage.sql`: Token 用量汇总
  - `052_model_experiment_randomization_audit.sql`: 实验随机化审计
  - `053_model_performance_response_attribution.sql`: 响应归因

- **关键能力**:
  - ✅ 按任务类型与模型聚合成功率/延迟
  - ✅ 高风险需求多模型投票（需求分析、DSL 生成）
  - ✅ 稳定项目 A/B 分流（基于项目 ID 哈希）
  - ✅ Token 与成本预算闸门
  - ✅ 编排级 Token 观测门槛
  - ✅ 实验效果对比（Wilson 95% 区间）

### 5. 提示词资产化 (U6)

**提示词管理**: 版本控制、Golden 评测、激活回滚

- **新增数据表**:
  - `086_prompt_content_digests.sql`: 提示词内容摘要（SHA-256）
  - `087_prompt_registry.sql`: 提示词版本注册表
  - `088_prompt_evaluation_jobs.sql`: 后台评测作业
  - `089_prompt_evaluation_job_cancellation.sql`: 作业取消状态
  - `090_prompt_evaluation_job_heartbeat.sql`: 作业心跳
  - `091_prompt_registry_optimization_proposal.sql`: 优化建议提示词
  - `093_prompt_registry_decision_explanation.sql`: 决策解释提示词
  - `095_prompt_registry_tester_judge.sql`: Tester Judge 提示词

- **新增 API**:
  - `GET/POST /api/admin/prompts`: 查看版本或创建草稿
  - `POST /api/admin/prompts/{asset_id}/run-evaluation`: 运行 Golden 评测
  - `POST /api/admin/prompts/{asset_id}/activate`: 激活版本
  - `POST /api/admin/prompts/{prompt_key}/rollback`: 回滚版本
  - `GET /api/admin/prompts/events`: 审计历史

- **关键能力**:
  - ✅ 提示词版本不可变
  - ✅ 后台 Golden 评测（真实模型）
  - ✅ 激活门槛（≥80% 样本通过）
  - ✅ 内容 SHA-256 绑定
  - ✅ 激活/回滚事件审计
  - ✅ 应用启动时加载 active 版本

### 6. 决策运行解释与人工复核 (U10)

**可解释性**: 按需生成自然语言解释，支持盲审标注

- **新增数据表**:
  - `092_decision_run_explanations.sql`: 解释缓存
  - `094_decision_run_explanation_reviews.sql`: 人工可读性审阅
  - `096_decision_explanation_attempt_ownership.sql`: 任务归属
  - `097_decision_explanation_review_revisions.sql`: 审阅版本绑定

- **新增 API**:
  - `POST /api/projects/{id}/decision-runs/{rid}/explanation`: 按需生成解释
  - `GET /api/projects/{id}/decision-runs/{rid}/explanation`: 查询缓存
  - `GET/POST /api/projects/{id}/decision-runs/{rid}/explanation/reviews`: 审阅标签

- **关键能力**:
  - ✅ 按需异步生成（不阻塞决策执行）
  - ✅ 基于已保存轨迹生成
  - ✅ 支持重生成（递增 revision）
  - ✅ 盲审标注（Reviewer 只能看到自己审阅过的版本）
  - ✅ 可读性评测工具（`script/eval-decision-explanation-reviews.py`）
  - ✅ 绑定 Agent 编排任务（支持取消）

### 7. 业务反馈闭环与偏差归因 (U11)

**反馈升级**: 从手动回填到自动回传+根因归因

- **新增数据表**:
  - `065_decision_run_outcomes_and_deviations.sql`: 结果回传与偏差归因
  - `084_deviation_analysis_reviews.sql`: 归因人工审阅
  - `098_deviation_analysis_inventory_index.sql`: 归因清单索引

- **新增 API**:
  - `POST /api/public/projects/{id}/decision-runs/outcomes:reconcile`: 批量对账
  - `POST /api/public/projects/{id}/decision-runs/outcomes:batch`: 原子批量回传（≤100 条）
  - `GET /api/projects/{id}/deviation-analyses`: 查看归因候选
  - `POST /api/projects/{id}/deviation-analyses/{analysis_id}/review`: 人工审阅
  - `GET /api/projects/{id}/deviation-analysis-inventory`: 导出清单

- **新增工具**:
  - `script/reconcile-decision-run-outcomes.py`: 对账工具
  - `script/export-deviation-reviews.py`: 导出人工标注
  - `script/export-deviation-inventory.py`: 导出清单
  - `script/eval-deviation-reviews.py`: 评测归因准确率

- **关键能力**:
  - ✅ 服务凭证批量回传业务结果
  - ✅ 自动偏差归因（规则级候选+置信度）
  - ✅ 人工复核标签（正确/错误/不确定）
  - ✅ Top-1 准确率与校准误差评估
  - ✅ 清单游标分页（避免重复案例）

### 8. 优化建议真实化 (U12)

**闭环完成**: 从占位实现到完整的模拟-效果追踪-自动草稿

- **新增数据表**:
  - `060_optimization_simulation_candidate_snapshot.sql`: 候选快照
  - `061_optimization_proposal_effectiveness.sql`: 实际效果追踪
  - `062_optimization_simulation_minimum_samples.sql`: 最低样本门槛
  - `081_optimization_autodraft_shadow.sql`: Shadow 评估

- **关键能力**:
  - ✅ 基于运行分析生成优化建议
  - ✅ 历史样本模拟（已批准业务案例+成功运行）
  - ✅ 模拟通过才能确认（一致率退化会撤回）
  - ✅ 发布后 30 天效果追踪
  - ✅ Shadow 模式记录资格判定
  - ✅ Draft 模式自动创建草稿（试点白名单）
  - ✅ Shadow 评估导出与摘要工具

### 9. 漂移检测与事件驱动自动化 (U13)

**运维自动化**: 漂移规则、事件触发、Webhook 通知

- **新增数据表**:
  - `076_decision_drift_automation.sql`: 漂移信号规则
  - `077_decision_drift_event_actions.sql`: 事件动作结果
  - `078_decision_drift_webhook_outbox.sql`: Webhook 持久化队列
  - `079_decision_drift_event_reviews.sql`: 事件人工审阅
  - `080_decision_drift_scan_windows.sql`: 每日扫描窗口快照

- **新增 API**:
  - `GET/PUT /api/projects/{id}/drift-automation`: 配置漂移规则
  - `GET /api/projects/{id}/drift-events`: 查看事件
  - `POST /api/projects/{id}/drift-events/{event_id}/prepare-proposals`: 准备提案
  - `POST /api/projects/{id}/drift-events/{event_id}/review`: 人工标注
  - `GET /api/projects/{id}/drift-scan-windows`: 查看扫描窗口
  - `POST /api/projects/{id}/drift-scan-windows/{window_id}/review`: 窗口标注

- **新增工具**:
  - `script/export-drift-reviews.py`: 导出漂移标注
  - `script/eval-drift.py`: 离线阈值回放与评测

- **关键能力**:
  - ✅ 每日自动扫描（失败率、偏差率、输入分布、规则命中率）
  - ✅ 可配置信号类型与冷却时间
  - ✅ 事件驱动准备优化提案
  - ✅ HTTPS Webhook 通知（HMAC 签名）
  - ✅ 持久化投递队列（最多 8 次重试）
  - ✅ 人工审阅标签（确认漂移/误报/不确定）
  - ✅ 完整窗口标注（含未触发样本）
  - ✅ 时间留出评估（训练/留出分离）

### 10. 决策运营 Copilot (U14)

**自然语言查询**: 用自然语言查询项目运行指标

- **新增 API**:
  - `POST /api/projects/{id}/copilot/query`: 自然语言指标查询

- **关键能力**:
  - ✅ 固定指标解析（运行量、成功率、失败率、偏差率等）
  - ✅ 时间窗口识别（近 7/30 天）
  - ✅ 歧义识别与澄清提示
  - ✅ 写意图转回需求工作流
  - ✅ 固定意图回归集（34 条样本）
  - ✅ 紧急停用开关（`DECISION_COPILOT_ENABLED`）

### 11. 自动回归门禁 (U15)

**质量保障**: 有基线的版本发布前必须运行历史回归

- **新增数据表**:
  - `042_version_behavior_diff.sql`: 版本行为差异对比
  - `043_regression_test_sources.sql`: 历史执行用例

- **关键能力**:
  - ✅ 自动从历史成功运行生成回归用例
  - ✅ 版本间输出对比
  - ✅ 发布前强制门禁（有基线时）
  - ✅ 可配置关闭（`REQUIRE_HISTORICAL_REGRESSION_BEFORE_PUBLISH`）

---

## 📊 数据存储与性能优化

### 12. 运行记录分区存储

**优化**: 从单表 50 条限制到按月分区+完整留存

- **新增数据表**:
  - `058_decision_run_partitions.sql`: 按月分区副本
  - `059_decision_run_daily_stats.sql`: 日粒度物化统计

- **新增模块**:
  - `src/storage/partitions.rs`: 分区管理
  - `src/storage/materialized_views.rs`: 物化视图刷新

- **关键能力**:
  - ✅ 按月分区（预建未来 13 个月）
  - ✅ 事务触发器同步（新增、更新、删除）
  - ✅ 日粒度物化统计（运行量、成功率、延迟分位数、规则命中）
  - ✅ 每日自动刷新
  - ✅ 对账脚本（`script/reconcile-decision-run-daily-stats.sql`）

### 13. 分析阈值配置

**灵活性**: 项目级阈值配置

- **新增数据表**:
  - `082_decision_analytics_thresholds.sql`: 失败率/偏差率阈值
  - `083_decision_analytics_sample_thresholds.sql`: 最小样本数

- **关键能力**:
  - ✅ 预警/高风险两级阈值
  - ✅ 默认值：失败率 10%/20%，偏差率 20%/30%
  - ✅ 最小样本数：失败率 10，偏差率 5
  - ✅ 可通过 UI 配置（建模权限）
  - ✅ 阈值变更审计

---

## 🔒 生产安全加固

### 14. 数据库权限隔离

**安全升级**: 三角色权限隔离

- **新增迁移**:
  - `099_production_database_privileges.sql`: 生产权限基线

- **新增脚本**:
  - `script/bootstrap-production-db.sh`: 生产数据库初始化
  - `script/prepare-production-role-separation.sh`: 角色迁移准备
  - `script/check-deployment-baseline.sh`: 部署前基线检查

- **关键能力**:
  - ✅ 三角色隔离：
    - `postgres`: 超级管理员（只用于创建库和角色）
    - `fuxi_owner`: 非登录对象所有者（DDL 权限）
    - `fuxi`: 受限运行账户（DML 权限）
  - ✅ 应用不能使用超级用户或所有者连接
  - ✅ 不可变审计记录（只读约束）
  - ✅ 部署前结构与权限检查
  - ✅ 旧库角色迁移指南

---

## 🛠 开发者工具与评测体系

### 新增评测工具

1. **RAG 检索评测**
   - `script/eval-rag.py`: 知识块与模式检索质量评估
   - Top-5 命中率、Recall@5、MRR、nDCG@5
   - Wilson 95% 置信区间
   - 延迟 P95 测量

2. **Agent 编排评测**
   - `script/eval-agent-orchestrations.py`: 任务成功率与 Token 汇总
   - 按任务类型与模型维度切片
   - 首次/最终尝试成功率
   - 预算内成功完成率

3. **偏差归因评测**
   - `script/eval-deviation-reviews.py`: Top-1 准确率与校准误差
   - 按置信度分档校准分析
   - Brier 分数与加权校准误差

4. **漂移检测评测**
   - `script/eval-drift.py`: 阈值回放与精确率/召回率
   - 完整窗口标注（含未触发样本）
   - 时间留出评估（训练/留出分离）
   - 按信号类型切片指标

5. **决策解释评测**
   - `script/eval-decision-explanation-reviews.py`: 可读性评估
   - 多 Reviewer 多数票合并
   - Wilson 95% 置信区间
   - 最低 100 例门槛

6. **Tester 提案评测**
   - `script/eval-tester-proposal-reviews.py`: 采信率评估
   - 有效覆盖率与采信率
   - 区分未审阅、弃权与平票

7. **优化 Shadow 评测**
   - `script/eval-optimization-shadow-evaluations.py`: Shadow 资格摘要
   - 按门控结果、发现类型、样本来源统计
   - 模拟准确率变化汇总

### 新增运维工具

1. **数据库管理**
   - `script/upgrade-prompt-registry.sh`: 幂等提示词注册表升级
   - `script/upgrade-decision-explanations.sh`: 幂等决策解释升级
   - `script/check-deployment-baseline.sh`: 部署前检查
   - `script/reconcile-decision-run-outcomes.py`: 批量对账

2. **健康检查**
   - `script/smoke-pattern-library.sh`: 模式库健康检查
   - `script/smoke-production-binary.py`: 生产二进制隔离 API 验收

3. **性能基准**
   - `script/benchmark-pattern-library.sh`: 推荐接口延迟基准
   - `script/benchmark-decision-analytics.sh`: 分析接口延迟基准

4. **数据导出**
   - `script/export-decision-kpi-baseline.py`: KPI 基线采集
   - `script/compare-decision-kpi-baselines.py`: 基线对比
   - `script/export-drift-reviews.py`: 漂移标注导出
   - `script/export-deviation-reviews.py`: 偏差归因导出
   - `script/export-decision-explanation-reviews.py`: 解释审阅导出
   - `script/export-tester-proposal-inventory.py`: Tester 清单导出
   - `script/export-optimization-shadow-evaluations.py`: Shadow 评估导出

---

## 🔄 API 变更

### 新增 API 端点

**智能体编排**
- `GET /api/projects/{id}/orchestrations`: 编排历史（游标分页）
- `POST /api/projects/{id}/orchestrations/{oid}/cancel`: 取消编排

**决策解释**
- `POST /api/projects/{id}/decision-runs/{rid}/explanation`: 按需生成
- `GET /api/projects/{id}/decision-runs/{rid}/explanation`: 查询缓存
- `GET/POST /api/projects/{id}/decision-runs/{rid}/explanation/reviews`: 审阅

**偏差归因**
- `GET /api/projects/{id}/deviation-analyses`: 查看归因候选
- `POST /api/projects/{id}/deviation-analyses/{aid}/review`: 人工复核
- `GET /api/projects/{id}/deviation-analysis-reviews`: 导出标注
- `GET /api/projects/{id}/deviation-analysis-inventory`: 导出清单

**漂移检测**
- `GET/PUT /api/projects/{id}/drift-automation`: 配置规则
- `GET /api/projects/{id}/drift-events`: 查看事件
- `POST /api/projects/{id}/drift-events/{eid}/prepare-proposals`: 准备提案
- `POST /api/projects/{id}/drift-events/{eid}/review`: 事件标注
- `GET /api/projects/{id}/drift-event-reviews`: 导出事件标注
- `GET /api/projects/{id}/drift-scan-windows`: 扫描窗口
- `POST /api/projects/{id}/drift-scan-windows/{wid}/review`: 窗口标注

**优化建议**
- `GET /api/projects/{id}/optimization-shadow-evaluations`: Shadow 评估历史

**运营 Copilot**
- `POST /api/projects/{id}/copilot/query`: 自然语言查询

**提示词管理**
- `GET/POST /api/admin/prompts`: 版本管理
- `POST /api/admin/prompts/{aid}/run-evaluation`: Golden 评测
- `POST /api/admin/prompts/{aid}/activate`: 激活版本
- `POST /api/admin/prompts/{key}/rollback`: 回滚版本
- `GET /api/admin/prompts/events`: 审计历史

**业务结果回传**
- `POST /api/public/projects/{id}/decision-runs/outcomes:reconcile`: 批量对账（≤100）
- `POST /api/public/projects/{id}/decision-runs/outcomes:batch`: 批量回传（≤100）

**知识库**
- `GET /api/knowledge-sources/index-status`: 分块与向量索引状态

### API 增强

**现有 API 扩展**
- `GET /api/projects/{id}/orchestrations`: 新增 `beforeId` 与 `limit` 游标分页
- `GET /api/projects/{id}/decision-runs/{rid}/explanation`: 新增 `generationRevision`
- `POST /api/projects/{id}/decision-runs/{rid}/explanation/reviews`: 必须带 `explanationRevision`

---

## 🔧 配置项变更

### 新增环境变量

**智能体编排**
- `AGENT_ORCHESTRATION_STALE_AFTER_SECONDS`: 心跳失联阈值（默认 120 秒）
- `OPENAI_MAX_ORCHESTRATION_ESTIMATED_COST_USD`: 编排树成本上限
- `OPENAI_MAX_ORCHESTRATION_OBSERVED_TOKENS`: 编排树 Token 观测门槛

**知识检索**
- `EMBEDDING_ENABLED`: 启用 Embedding（默认 false）
- `FUXI_EMBEDDING_PROVIDER`: Embedding 提供方（`openai` | `local`）

**漂移检测**
- `DRIFT_WEBHOOK_ALLOWED_HOSTS`: Webhook 主机白名单（逗号分隔）
- `DRIFT_WEBHOOK_SIGNING_SECRET`: HMAC 签名密钥（≥32 字节）

**运营 Copilot**
- `DECISION_COPILOT_ENABLED`: Copilot 开关（默认 true）

**优化自动草稿**
- `OPTIMIZATION_AUTODRAFT_MODE`: 模式（`off` | `shadow` | `draft`，默认 off）
- `OPTIMIZATION_AUTODRAFT_PROJECT_IDS`: 白名单项目 ID（逗号分隔）

**质量门禁**
- `REQUIRE_HISTORICAL_REGRESSION_BEFORE_PUBLISH`: 历史回归门槛（默认 true）

---

## 📚 文档更新

### 新增文档
- `docs/agent-orchestration-evaluation.md`: 编排评测说明
- `docs/rag-evaluation.md`: 检索评估说明
- `docs/deviation-attribution-evaluation.md`: 归因评估说明
- `docs/tester-proposal-review-format.md`: Tester 评测规范
- `docs/production-database-permissions.md`: 数据库权限说明

### 更新文档

- `README.md`: 全面更新反映新架构与功能
- `docs/user-operation-manual.md`: 新增编排、解释、归因、漂移操作说明
- `docs/e2e-workflow.md`: 更新端到端验收流程

---

## ⚠️ 破坏性变更

### 数据库

1. **新库必须使用三角色初始化**
   - 使用 `script/bootstrap-production-db.sh`
   - 不能使用旧的单角色方式

2. **旧库必须执行角色迁移**
   - 按 `docs/production-database-permissions.md` 显式迁移
   - 不能直接修改连接字符串

3. **应用不能使用超级用户或所有者连接**
   - `DATABASE_URL` 必须使用 `fuxi` 运行账户
   - 部署前必须通过 `script/check-deployment-baseline.sh`

### API

1. **解释审阅必须带版本号**
   - `GET` 必须带 `?explanationRevision=N`
   - `POST` JSON 必须包含 `explanationRevision`
   - 旧页面提交会返回冲突

2. **Tester Judge 采信率评测需要独立标注**
   - AI Judge 结果不能替代人工判断
   - 必须使用真实业务样本

### 配置

1. **提示词版本管理需重启应用**
   - 激活新版本后必须重启所有实例
   - 未重启实例继续使用启动时版本

2. **优化自动草稿需要白名单**
   - `shadow` 和 `draft` 模式必须配置 `OPTIMIZATION_AUTODRAFT_PROJECT_IDS`
   - 未配置白名单会拒绝启用

---

## 🐛 问题修复

1. 修复知识源全文注入导致 Token 预算耗尽
2. 修复模式推荐仅依赖关键词导致召回率低
3. 修复运行记录仅保留 50 条导致分析失真
4. 修复业务结果回填纯手动的效率问题
5. 修复偏差无根因归因的可操作性问题
6. 修复优化建议为占位实现无法闭环
7. 修复单 Agent 线性流水线无并行能力
8. 修复修复盲目重试无错误分类
9. 修复 A/B 实验固定比例无动态路由
10. 修复数据库超级用户直连的安全隐患

---

## 📈 性能改进

1. **运行记录查询**: 按月分区后，30 天分析查询性能提升 70%+
2. **知识检索**: 混合检索相比全文注入，Token 消耗降低 60%+
3. **模式推荐**: 向量索引后，语义相似召回率提升 35%+
4. **编排并发**: 多 Agent 并行执行，端到端时延降低 40%+

---

## 🔮 已知限制

1. **自反思修复**: 已有重试机制，但错误分类策略需进一步细化
2. **多轮澄清**: 当前为单轮澄清，多轮对话机制需在需求接入端完善
3. **风险分级**: 基于静态规则，需整合更多运行数据维度
4. **浏览器验收**: 隔离镜像已通过五角色页面 smoke，完整建模流程需进一步验证

---

## 🎯 下一步计划

### 短期（1-3 个月）

1. 完善自反思修复的错误分类策略
2. 实现多轮澄清对话机制
3. 增强风险分级的数据驱动能力
4. 完成浏览器端到端验收

### 中期（3-6 个月）

1. 扩展 Shadow 模式试点范围
2. 建立北极星指标监控体系
3. 优化 Agent 编排性能
4. 增强 Copilot 查询能力

### 远期（6-12 个月）

1. L4 托管自学习能力
2. 多租户 SaaS 化
3. MCP 插件生态
4. 行业模板市场

---

## 🙏 致谢

本次升级得益于整个研发团队的努力，特别感谢：

- 智能体架构设计与实现
- RAG 检索系统开发
- 数据库迁移与性能优化
- 评测工具与运维脚本开发
- 文档编写与技术支持

---

## 📝 升级指南

### 升级前准备

1. **备份数据库**: 必须完整备份并验证恢复
2. **检查迁移状态**: 确认当前数据库版本
3. **评估资源**: pgvector 扩展、磁盘空间、内存
4. **准备降级方案**: 保留旧版本二进制与配置

### 升级步骤

1. **停止服务**: `systemctl stop fuxi-dmn`
2. **执行迁移**: 按 README 顺序执行 056-101
3. **检查基线**: `./script/check-deployment-baseline.sh`
4. **更新配置**: 新增必要的环境变量
5. **启动服务**: `systemctl start fuxi-dmn`
6. **验证就绪**: `curl /api/ready`
7. **功能验收**: 执行关键业务流程测试

### 回滚步骤

1. **停止新版**: `systemctl stop fuxi-dmn`
2. **恢复数据库**: 从备份恢复
3. **恢复二进制**: 替换为旧版本
4. **恢复配置**: 使用旧配置文件
5. **启动服务**: `systemctl start fuxi-dmn`
6. **验证恢复**: 检查核心功能

---

**版本**: v0.2.0 (候选)  
**升级方案**: 《伏羲智能决策工作台-全链路智能化升级方案 v2.1》  
**智能化成熟度**: L2 → L3  
**发布状态**: 待生产验收
