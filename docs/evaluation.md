# 评测指南

仓库内的评测工具分别衡量检索、Agent 运行、偏差复核、漂移阈值、解释可读性、Tester 提案和优化影子结果。先定义评测问题、数据集版本和目标，再选择对应工具；不同指标不能合并成一个笼统的“模型准确率”。

评测输出是样本证据，不自动构成生产放行结论。确认数据覆盖、标签质量、时间范围、样本量、业务影响和人工复核结果后，再判断是否达标。

## 工具概览

| 工具 | 输入 | 主要用途 |
| --- | --- | --- |
| `script/eval-rag.py` | TSV 标注集 + 受保护诊断 API | 比较关键词、向量和融合检索质量及延迟。 |
| `script/eval-agent-orchestrations.py` | 项目编排历史 API | 汇总持久化的编排成功、步骤状态和 token 用量。 |
| `script/eval-deviation-reviews.py` | 偏差分析复核 JSONL，可选完整清单 | 评估已复核归因，不把未复核样本当负例。 |
| `script/eval-drift.py` | 漂移窗口或已复核事件 JSONL | 扫描阈值组合，或汇总已触发事件的复核结果。 |
| `script/eval-decision-explanation-reviews.py` | 盲审解释 JSONL | 评估最新解释修订的可读率和置信区间。 |
| `script/eval-tester-proposal-reviews.py` | 候选清单和人工复核 JSONL | 评估测试提案的人工接受度。 |
| `script/eval-optimization-shadow-evaluations.py` | 优化影子运行 JSONL | 评估候选优化的影子效果和样本充分性。 |

各工具都支持 `--help`。建议把报告写入访问受控的路径；某些数据包含业务查询、项目 ID 或评审信息。

## RAG 检索

标注 TSV 每行包含查询和相关知识块 ID：

```text
如何处理已撤销的订单?\t12,19
```

先离线校验标签文件，再对隔离或批准的环境运行诊断：

```sh
python3 script/eval-rag.py knowledge-golden.tsv --validate-only
API_TOKEN="$API_TOKEN" PROJECT_ID=123 \
  python3 script/eval-rag.py knowledge-golden.tsv \
  --api-base http://127.0.0.1:3000 --output rag-report.json
```

在线知识评测需要 JWT 和项目 ID，并会调用受保护的项目知识检索诊断接口。报告比较 `keyword`、`vector`、`fused`，包含 Recall@5、MRR、nDCG@5、命中率、延迟和数据集摘要。默认不导出原始查询；只有明确批准且报告受保护时才用 `--include-query-text`。最少 100 个样本才满足默认质量样本提示，延迟对比默认需要 30 个样本。样本数不足不表示质量已达标。

## Agent 编排

```sh
API_TOKEN="$API_TOKEN" PROJECT_ID=123 \
  python3 script/eval-agent-orchestrations.py \
  --api-base http://127.0.0.1:3000 --output agent-report.json
```

工具从受保护 API 分页读取项目的编排历史，报告状态率、步骤和用量汇总，并对比例给出 Wilson 95% 区间。默认至少 100 个案例才视为样本充分。它描述真实历史运行，不是控制变量的模型质量实验；任务构成、重试、并发、模型路由和缺失用量都可能影响比较，相关重试也会破坏简单独立样本假设。

## 复核型离线评测

偏差归因、解释和 Tester 提案工具消费由对应 `export-*.py` 脚本生成或按脚本字段定义整理的 JSONL。每行一个 JSON 对象，ID 与标签必须符合脚本的精确 schema；重复评审、未知标签、无效 JSON 和超限文件会被拒绝。例如：

```sh
python3 script/export-deviation-reviews.py --help
python3 script/eval-deviation-reviews.py reviewed-deviations.jsonl \
  --inventory deviation-inventory.jsonl --output deviation-report.json
python3 script/eval-decision-explanation-reviews.py explanation-reviews.jsonl \
  > explanation-report.json
python3 script/eval-tester-proposal-reviews.py tester-inventory.jsonl tester-reviews.jsonl \
  > tester-report.json
```

先运行各命令的 `--help` 确认本地版本的参数。解释评测按每个解释的最新提供修订计数，并将 `inconclusive` 排除在可读率分母之外。偏差评测基于被复核归因；即使提供运行清单，也要单独分析未被发现或未被复核的偏差。Tester 提案接受度受提案生成和人工选择影响，不能单独解释为回归测试覆盖率。

漂移评测示例：

```sh
python3 script/eval-drift.py labeled-windows.jsonl --output drift-report.json
python3 script/eval-drift.py reviewed-events.jsonl --reviewed-events
```

窗口标签可用于历史时间切分和阈值扫描；有独立时间留出集时可使用 `--holdout-from`。`--reviewed-events` 只统计已触发并被复核事件的确认率或误报相关指标，不能估算从未触发窗口的漏报率。

## 报告解释

- Wilson 95% 区间表示样本比例的不确定性，不会纠正选择偏差、相关样本或标签错误。
- 报告中的“样本充分”只表示达到工具配置的最小数量，不等于业务验收通过。
- 在线检索、运行历史和提示词评测会读取部署环境数据。先确认访问权限、数据保留和报告脱敏要求。
- 固定数据集、标签规则、评测脚本版本和运行时间；模型或阈值变更后使用未参与调参的时间或项目留出集复测。
- 记录失败样本并做人工抽查。生产放行还需要业务负责人设定目标、风险门禁和回滚条件。
