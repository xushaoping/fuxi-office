# R5 检索评测与验收口径

`script/eval-rag.py` 使用业务人员标注的查询集，对受项目权限保护的检索诊断 API 比较关键词、向量和 RRF 融合结果。它可验证标注格式并生成诊断报告；人工标注质量、真实 Embedding 模型身份和代表性负载验收仍需独立证据。

方案要求：同一查询集上的融合 Top-5 命中率比关键词基线提高至少 **15 个百分点**，检索 P95 不超过 **500ms**。知识与模式应各自评测，不把不同语料或不同权限范围的结果合并成一个提升值。

## 准备真实标注集

UTF-8 TSV 每行两列：`查询<TAB>相关 ID 列表`，多个相关 ID 用英文逗号分隔。知识使用知识块 ID，模式使用模式 ID；ID 为正的有符号 64 位整数，每条查询至少一个相关 ID，不得重复。查询去首尾空格后不能重复；知识最多 2000 个 UTF-8 字节，模式另限制 500 个字符。每个文件最多 100,000 条、32 MiB，每行最多 64 KiB。空行和 `#` 注释行不算案例，但仍受文件/行大小限制。

应先固定项目权限、知识有效期、语料版本、业务场景、标注规则和排除条件。重复或近似查询会影响独立性；工具拒绝相同查询，但不能证明语义去重或标注代表性。不应删除未命中查询以抬高分数，也不能用模型自己生成的相关 ID 标签冒充人工黄金结果。

可选 `--categories` JSONL 每行恰好一个 `category` 字符串，与有效 TSV 查询逐行对应。类别用于按场景、来源或复杂度切片，允许多条查询属于同一类别；工具拒绝额外字段、重复 JSON 键、非有限数、控制字符及记录数不匹配。

先执行纯本地校验，不需要令牌、服务地址或项目 ID，也不发送网络请求：

```bash
python3 script/eval-rag.py labeled-knowledge.tsv --validate-only \
  --categories labeled-knowledge-categories.jsonl
python3 script/eval-rag.py labeled-patterns.tsv --kind patterns --validate-only
```

该模式输出查询数、样本门槛、类别数和 `datasetDigest`，不输出查询正文或相关 ID。摘要绑定查询、排序后的相关 ID 与可选类别，可供保存评测证据时识别标注集；摘要不证明标签来自合格业务人员。全部标注与分类在开始任何在线请求之前校验，最后一条无效也不会先调用前面的查询。

## 在线诊断与指标分母

有明确授权的目标环境中，使用 Viewer 凭证调用；知识评测必须指定项目，模式可附项目 ID 校验该项目查看权限：

```bash
API_TOKEN='<project-view-token>' PROJECT_ID='<项目 ID>' \
  python3 script/eval-rag.py labeled-knowledge.tsv \
  --categories labeled-knowledge-categories.jsonl --output target/rag-diagnostics.json
```

所有阶段使用同一批查询、同一个相关 ID 集合：

- Top-5 命中率：前五结果中至少出现一个相关 ID 的查询数 / 查询数。
- 融合提升：`(融合命中查询数 - 关键词命中查询数) / 查询数 × 100`，单位为百分点；比较使用未四舍五入的整数分子与分母。
- Recall@5 宏平均：先按每条查询计算命中的相关 ID 比例，再按查询平均。
- Recall@5 micro：所有查询前五结果中命中的相关 ID 总数 / 全部标注相关 ID 总数。它的分母不同于查询数。
- MRR/nDCG@5 和阶段首个相关结果排名继续保留。Top-5 和 micro Recall 提供分子、分母与 Wilson 95% 区间。

默认至少 **100 条不同标注查询**才能判定质量目标；`--minimum-quality-samples` 可按预先批准的方案调整。少于该数量时 `acceptance.quality.status=insufficient_samples`；足量后按 15pp 输出 `target_met` 或 `target_not_met`。样本门槛是当前工具的默认策略，不意味着方案已规定该样本规模，也不能纠正样本偏差。

## Embedding 与延迟证据边界

知识 API 的 `vector_status` 随每例写入报告：`ready`、`no_matches` 表示服务端报告执行过向量检索；`disabled`、`pgvector_unavailable`、`provider_error`、`query_error`、`not_run` 分别计入降级/未执行，不能伪装成正常语义召回。`no_matches` 虽证明向量阶段运行，却不代表检索命中。

旧 API 或当前模式 API 未提供状态时记为 `unknown`；不能仅凭向量候选非空或融合提升推断用了真实语义模型。现有诊断响应不证明 provider/model 身份，可能使用本地词法哈希而非预训练语义模型，因此 `embeddingEvidence.providerAndModelVerified=false`。正式验收需要另外保存部署配置、向量模型/维度/语料版本、请求审计和无降级运行证据。

延迟是**串行调用诊断端点的请求往返时间**，包含该端点的关键词、向量与融合诊断开销。默认至少 30 次才进行诊断 P95 比较；足量后只输出 `diagnostic_target_met` / `diagnostic_target_not_met`。即使 P95 ≤500ms 且样本多，也不证明真实用户并发、代表性数据量、冷暖缓存、生产硬件或正式检索端点下的 P95。报告明确 `representativeLoadVerified=false`、`eligibleForProductionAcceptance=false`。

因此本工具始终输出 `acceptance.accepted=false`：样本不足为 `insufficient_samples`，足量但融合质量未达 15pp 为 `target_not_met`，诊断质量达到目标但仍缺真实模型/稳定负载证据为 `insufficient_evidence`。`reasons` 保留全部可见问题，不会因质量数值好看而隐藏降级或缺失证据。该工具不提供能把人工布尔声明转成已验收的开关。

程序退出 0 只表示输入有效、诊断报告生成成功。默认不输出查询正文；仅经确认适合保存标注内容时才使用 `--include-query-text`。完整 R5 验收仍需审核数据集、明确采样与指标口径、验证真实模型与权限隔离，并在目标部署环境单独完成稳定负载测试。
