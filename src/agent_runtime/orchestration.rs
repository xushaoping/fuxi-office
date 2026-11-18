use std::sync::atomic::{AtomicUsize, Ordering};
use std::{collections::BTreeMap, future::Future, pin::Pin, sync::Arc, time::Duration};

use anyhow::anyhow;
use serde::Serialize;
use tokio::{
    sync::{Semaphore, mpsc},
    task::JoinSet,
};

pub type AgentFuture<'a> =
    Pin<Box<dyn Future<Output = Result<AgentOutput, AgentFailure>> + Send + 'a>>;

#[derive(Debug, Clone, PartialEq)]
pub struct AgentOutput {
    pub output: serde_json::Value,
    pub completed_steps: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AgentFailure {
    pub failed_step: String,
    pub message: String,
    pub completed_steps: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AgentTool {
    pub name: String,
    pub description: String,
}

impl AgentTool {
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentStatus {
    Running,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentEvent {
    pub agent: String,
    pub status: AgentStatus,
}

pub trait Agent: Send + Sync {
    fn name(&self) -> &str;
    fn tools(&self) -> &[AgentTool];
    fn run<'a>(&'a self, input: serde_json::Value) -> AgentFuture<'a>;

    fn stream<'a>(
        &'a self,
        input: serde_json::Value,
        events: mpsc::Sender<AgentEvent>,
    ) -> AgentFuture<'a> {
        let name = self.name().to_owned();
        let execution = self.run(input);
        Box::pin(async move {
            let _ = events
                .send(AgentEvent {
                    agent: name.clone(),
                    status: AgentStatus::Running,
                })
                .await;
            let result = execution.await;
            let status = if result.is_ok() {
                AgentStatus::Succeeded
            } else {
                AgentStatus::Failed
            };
            let _ = events
                .send(AgentEvent {
                    agent: name,
                    status,
                })
                .await;
            result
        })
    }
}

#[derive(Clone)]
pub struct PipelineStage {
    name: String,
    timeout: Option<Duration>,
    tools: Vec<AgentTool>,
    execute: Arc<dyn Fn(serde_json::Value) -> AgentFuture<'static> + Send + Sync>,
}

impl PipelineStage {
    pub fn new<F, Fut>(name: impl Into<String>, execute: F) -> Self
    where
        F: Fn(serde_json::Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<serde_json::Value, String>> + Send + 'static,
    {
        let name = name.into();
        let stage_name = name.clone();
        Self {
            name,
            timeout: None,
            tools: Vec::new(),
            execute: Arc::new(move |input| {
                let stage_name = stage_name.clone();
                let execution = execute(input);
                Box::pin(async move {
                    execution
                        .await
                        .map(|output| AgentOutput {
                            output,
                            completed_steps: Vec::new(),
                        })
                        .map_err(|message| AgentFailure {
                            failed_step: stage_name,
                            message,
                            completed_steps: Vec::new(),
                        })
                })
            }),
        }
    }

    pub fn branch<C>(
        name: impl Into<String>,
        condition: C,
        when_true: Vec<PipelineStage>,
        when_false: Vec<PipelineStage>,
    ) -> Self
    where
        C: Fn(&serde_json::Value) -> bool + Send + Sync + 'static,
    {
        let name = name.into();
        Self {
            name,
            timeout: None,
            tools: Vec::new(),
            execute: Arc::new(move |input| {
                let steps = if condition(&input) {
                    &when_true
                } else {
                    &when_false
                };
                let steps = clone_stages(steps);
                let input = input.clone();
                Box::pin(async move {
                    match run_pipeline(steps, input).await {
                        Ok(report) => Ok(AgentOutput {
                            output: report.output,
                            completed_steps: report.completed_steps,
                        }),
                        Err(failure) => Err(AgentFailure {
                            failed_step: failure.failed_step,
                            message: failure.message,
                            completed_steps: failure.completed_steps,
                        }),
                    }
                })
            }),
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    pub fn with_tools(mut self, tools: Vec<AgentTool>) -> Self {
        self.tools = tools;
        self
    }
}

impl Agent for PipelineStage {
    fn name(&self) -> &str {
        &self.name
    }

    fn tools(&self) -> &[AgentTool] {
        &self.tools
    }

    fn run<'a>(&'a self, input: serde_json::Value) -> AgentFuture<'a> {
        (self.execute)(input)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PipelineReport {
    pub output: serde_json::Value,
    pub completed_steps: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PipelineFailure {
    pub failed_step: String,
    pub message: String,
    pub completed_steps: Vec<String>,
}

impl std::fmt::Display for PipelineFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "pipeline step '{}' failed: {}",
            self.failed_step, self.message
        )
    }
}

impl std::error::Error for PipelineFailure {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RefinementLimits {
    pub max_depth: usize,
    pub max_children_per_split: usize,
    pub max_total_tasks: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefinementDecision<T> {
    Complete(T),
    Split(Vec<T>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefinementFailure {
    pub depth: usize,
    pub reason: String,
}

impl std::fmt::Display for RefinementFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "refinement failed at depth {}: {}",
            self.depth, self.reason
        )
    }
}

impl std::error::Error for RefinementFailure {}

type RefinementFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, RefinementFailure>> + Send + 'a>>;
type StaticRefinementFuture<T> =
    Pin<Box<dyn Future<Output = Result<T, RefinementFailure>> + Send + 'static>>;

pub async fn refine_and_combine<T, R, RFut, C, CFut>(
    root: T,
    limits: RefinementLimits,
    refine: R,
    combine: C,
) -> Result<T, RefinementFailure>
where
    T: Clone + Send,
    R: Fn(T, usize) -> RFut + Send + Sync,
    RFut: Future<Output = Result<RefinementDecision<T>, String>> + Send,
    C: Fn(T, Vec<T>, usize) -> CFut + Send + Sync,
    CFut: Future<Output = Result<T, String>> + Send,
{
    fn visit<'a, T, R, RFut, C, CFut>(
        task: T,
        depth: usize,
        limits: RefinementLimits,
        total_tasks: &'a mut usize,
        refine: &'a R,
        combine: &'a C,
    ) -> RefinementFuture<'a, T>
    where
        T: Clone + Send + 'a,
        R: Fn(T, usize) -> RFut + Send + Sync + 'a,
        RFut: Future<Output = Result<RefinementDecision<T>, String>> + Send + 'a,
        C: Fn(T, Vec<T>, usize) -> CFut + Send + Sync + 'a,
        CFut: Future<Output = Result<T, String>> + Send + 'a,
    {
        Box::pin(async move {
            let parent = task.clone();
            match refine(task, depth)
                .await
                .map_err(|reason| RefinementFailure { depth, reason })?
            {
                RefinementDecision::Complete(result) => Ok(result),
                RefinementDecision::Split(children) => {
                    if depth >= limits.max_depth {
                        return Err(RefinementFailure {
                            depth,
                            reason: format!("maximum depth {} exceeded", limits.max_depth),
                        });
                    }
                    if children.is_empty() {
                        return Err(RefinementFailure {
                            depth,
                            reason: "split produced no child tasks".into(),
                        });
                    }
                    if children.len() > limits.max_children_per_split {
                        return Err(RefinementFailure {
                            depth,
                            reason: format!(
                                "split produced {} children; limit is {}",
                                children.len(),
                                limits.max_children_per_split
                            ),
                        });
                    }
                    *total_tasks = total_tasks.saturating_add(children.len());
                    if *total_tasks > limits.max_total_tasks {
                        return Err(RefinementFailure {
                            depth,
                            reason: format!(
                                "task count {} exceeds limit {}",
                                *total_tasks, limits.max_total_tasks
                            ),
                        });
                    }
                    let mut child_results = Vec::with_capacity(children.len());
                    for child in children {
                        child_results.push(
                            visit(child, depth + 1, limits, total_tasks, refine, combine).await?,
                        );
                    }
                    combine(parent, child_results, depth)
                        .await
                        .map_err(|reason| RefinementFailure { depth, reason })
                }
            }
        })
    }

    let mut total_tasks = 1;
    if limits.max_total_tasks == 0 {
        return Err(RefinementFailure {
            depth: 0,
            reason: "total task limit must allow the root task".into(),
        });
    }
    visit(root, 0, limits, &mut total_tasks, &refine, &combine).await
}

pub async fn refine_and_combine_parallel<T, R, RFut, C, CFut>(
    root: T,
    limits: RefinementLimits,
    max_parallelism: usize,
    refine: R,
    combine: C,
) -> Result<T, RefinementFailure>
where
    T: Clone + Send + 'static,
    R: Fn(T, usize) -> RFut + Send + Sync + 'static,
    RFut: Future<Output = Result<RefinementDecision<T>, String>> + Send + 'static,
    C: Fn(T, Vec<T>, usize) -> CFut + Send + Sync + 'static,
    CFut: Future<Output = Result<T, String>> + Send + 'static,
{
    fn visit<T, R, RFut, C, CFut>(
        task: T,
        depth: usize,
        limits: RefinementLimits,
        scheduled_tasks: Arc<AtomicUsize>,
        semaphore: Arc<Semaphore>,
        refine: Arc<R>,
        combine: Arc<C>,
    ) -> StaticRefinementFuture<T>
    where
        T: Clone + Send + 'static,
        R: Fn(T, usize) -> RFut + Send + Sync + 'static,
        RFut: Future<Output = Result<RefinementDecision<T>, String>> + Send + 'static,
        C: Fn(T, Vec<T>, usize) -> CFut + Send + Sync + 'static,
        CFut: Future<Output = Result<T, String>> + Send + 'static,
    {
        Box::pin(async move {
            let parent = task.clone();
            let permit =
                semaphore
                    .clone()
                    .acquire_owned()
                    .await
                    .map_err(|error| RefinementFailure {
                        depth,
                        reason: format!("refinement worker pool closed: {error}"),
                    })?;
            let decision = refine(task, depth).await;
            drop(permit);
            match decision.map_err(|reason| RefinementFailure { depth, reason })? {
                RefinementDecision::Complete(result) => Ok(result),
                RefinementDecision::Split(children) => {
                    if depth >= limits.max_depth {
                        return Err(RefinementFailure {
                            depth,
                            reason: format!("maximum depth {} exceeded", limits.max_depth),
                        });
                    }
                    if children.is_empty() {
                        return Err(RefinementFailure {
                            depth,
                            reason: "split produced no child tasks".into(),
                        });
                    }
                    if children.len() > limits.max_children_per_split {
                        return Err(RefinementFailure {
                            depth,
                            reason: format!(
                                "split produced {} children; limit is {}",
                                children.len(),
                                limits.max_children_per_split
                            ),
                        });
                    }
                    let reserved = scheduled_tasks.fetch_update(
                        Ordering::Relaxed,
                        Ordering::Relaxed,
                        |current| {
                            current
                                .checked_add(children.len())
                                .filter(|total| *total <= limits.max_total_tasks)
                        },
                    );
                    if let Err(current) = reserved {
                        return Err(RefinementFailure {
                            depth,
                            reason: format!(
                                "task count {} exceeds limit {}",
                                current.saturating_add(children.len()),
                                limits.max_total_tasks
                            ),
                        });
                    }

                    let mut tasks = JoinSet::new();
                    for (index, child) in children.into_iter().enumerate() {
                        let child = visit(
                            child,
                            depth + 1,
                            limits,
                            Arc::clone(&scheduled_tasks),
                            Arc::clone(&semaphore),
                            Arc::clone(&refine),
                            Arc::clone(&combine),
                        );
                        tasks.spawn(async move { (index, child.await) });
                    }
                    let mut child_results = Vec::new();
                    while let Some(joined) = tasks.join_next().await {
                        match joined {
                            Ok((index, Ok(result))) => child_results.push((index, result)),
                            Ok((_, Err(failure))) => {
                                tasks.abort_all();
                                return Err(failure);
                            }
                            Err(error) => {
                                tasks.abort_all();
                                return Err(RefinementFailure {
                                    depth,
                                    reason: format!("child refinement task failed: {error}"),
                                });
                            }
                        }
                    }
                    child_results.sort_by_key(|(index, _)| *index);
                    let child_results = child_results
                        .into_iter()
                        .map(|(_, result)| result)
                        .collect();
                    let permit =
                        semaphore
                            .acquire_owned()
                            .await
                            .map_err(|error| RefinementFailure {
                                depth,
                                reason: format!("refinement worker pool closed: {error}"),
                            })?;
                    let result = combine(parent, child_results, depth).await;
                    drop(permit);
                    result.map_err(|reason| RefinementFailure { depth, reason })
                }
            }
        })
    }

    let max_total_tasks = limits.max_total_tasks;
    if max_parallelism == 0 || max_total_tasks == 0 {
        return Err(RefinementFailure {
            depth: 0,
            reason: "parallelism and total task limits must be positive".into(),
        });
    }
    visit(
        root,
        0,
        limits,
        Arc::new(AtomicUsize::new(1)),
        Arc::new(Semaphore::new(max_parallelism)),
        Arc::new(refine),
        Arc::new(combine),
    )
    .await
}

pub async fn refine_recursively<T, F, Fut>(
    root: T,
    limits: RefinementLimits,
    refine: F,
) -> Result<Vec<T>, RefinementFailure>
where
    T: Send + 'static,
    F: Fn(T, usize) -> Fut + Send + Sync,
    Fut: Future<Output = Result<RefinementDecision<T>, String>> + Send,
{
    let mut pending = vec![(root, 0usize)];
    let mut total_tasks = 1usize;
    let mut completed = Vec::new();

    while let Some((task, depth)) = pending.pop() {
        let decision = refine(task, depth)
            .await
            .map_err(|reason| RefinementFailure { depth, reason })?;
        match decision {
            RefinementDecision::Complete(result) => completed.push(result),
            RefinementDecision::Split(children) => {
                if depth >= limits.max_depth {
                    return Err(RefinementFailure {
                        depth,
                        reason: format!("maximum depth {} exceeded", limits.max_depth),
                    });
                }
                if children.is_empty() {
                    return Err(RefinementFailure {
                        depth,
                        reason: "split produced no child tasks".into(),
                    });
                }
                if children.len() > limits.max_children_per_split {
                    return Err(RefinementFailure {
                        depth,
                        reason: format!(
                            "split produced {} children; limit is {}",
                            children.len(),
                            limits.max_children_per_split
                        ),
                    });
                }
                total_tasks = total_tasks.saturating_add(children.len());
                if total_tasks > limits.max_total_tasks {
                    return Err(RefinementFailure {
                        depth,
                        reason: format!(
                            "task count {} exceeds limit {}",
                            total_tasks, limits.max_total_tasks
                        ),
                    });
                }
                pending.extend(children.into_iter().rev().map(|child| (child, depth + 1)));
            }
        }
    }

    Ok(completed)
}

pub async fn run_pipeline(
    stages: Vec<PipelineStage>,
    input: serde_json::Value,
) -> Result<PipelineReport, PipelineFailure> {
    run_pipeline_inner(stages, input, None).await
}

pub async fn run_pipeline_stream(
    stages: Vec<PipelineStage>,
    input: serde_json::Value,
    events: mpsc::Sender<AgentEvent>,
) -> Result<PipelineReport, PipelineFailure> {
    run_pipeline_inner(stages, input, Some(events)).await
}

async fn run_pipeline_inner(
    stages: Vec<PipelineStage>,
    mut input: serde_json::Value,
    events: Option<mpsc::Sender<AgentEvent>>,
) -> Result<PipelineReport, PipelineFailure> {
    let mut completed_steps = Vec::with_capacity(stages.len());
    for stage in stages {
        let execution = match &events {
            Some(events) => stage.stream(input, events.clone()),
            None => stage.run(input),
        };
        let result = match stage.timeout {
            Some(limit) => match tokio::time::timeout(limit, execution).await {
                Ok(result) => result,
                Err(_) => Err(AgentFailure {
                    failed_step: stage.name.clone(),
                    message: format!("step timed out after {} ms", limit.as_millis()),
                    completed_steps: Vec::new(),
                }),
            },
            None => execution.await,
        };
        match result {
            Ok(result) => {
                input = result.output;
                completed_steps.extend(result.completed_steps);
                completed_steps.push(stage.name);
            }
            Err(failure) => {
                completed_steps.extend(failure.completed_steps);
                return Err(PipelineFailure {
                    failed_step: failure.failed_step,
                    message: failure.message,
                    completed_steps,
                });
            }
        }
    }
    Ok(PipelineReport {
        output: input,
        completed_steps,
    })
}

fn clone_stages(stages: &[PipelineStage]) -> Vec<PipelineStage> {
    stages.to_vec()
}

#[derive(Debug, Clone, PartialEq)]
pub struct VoteCandidate {
    pub model: String,
    pub weight: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VoteOutcome {
    pub model: String,
    pub weight: f64,
    pub result: Result<serde_json::Value, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Consensus<T> {
    pub decision: Option<T>,
    pub confidence: f64,
    pub requires_human_review: bool,
    pub outcomes: Vec<VoteOutcome>,
}

pub async fn parallel_vote<T, F, Fut>(
    candidates: Vec<VoteCandidate>,
    execute: F,
) -> anyhow::Result<Consensus<T>>
where
    T: Serialize + Send + 'static,
    F: Fn(String) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<T, String>> + Send + 'static,
{
    let candidates: Vec<_> = candidates
        .into_iter()
        .filter(|candidate| {
            !candidate.model.trim().is_empty()
                && candidate.weight.is_finite()
                && candidate.weight > 0.0
        })
        .collect();
    if candidates.len() < 2 {
        return Err(anyhow!("并行共识至少需要两个有效模型"));
    }
    let unique_models = candidates
        .iter()
        .map(|candidate| candidate.model.trim())
        .collect::<std::collections::BTreeSet<_>>();
    if unique_models.len() != candidates.len() {
        return Err(anyhow!("并行共识不能重复使用同一模型"));
    }
    let total_weight: f64 = candidates.iter().map(|candidate| candidate.weight).sum();

    let mut tasks = JoinSet::new();
    let execute = Arc::new(execute);
    for candidate in candidates {
        let execute = Arc::clone(&execute);
        let model = candidate.model.clone();
        let weight = candidate.weight;
        tasks.spawn(async move {
            let result = execute(model.clone()).await;
            (model, weight, result)
        });
    }

    let mut outcomes = Vec::new();
    let mut groups: BTreeMap<String, (f64, T)> = BTreeMap::new();
    while let Some(joined) = tasks.join_next().await {
        let (model, weight, result) =
            joined.map_err(|error| anyhow!("并行模型任务失败：{error}"))?;
        match result {
            Ok(value) => {
                let serialized = serde_json::to_value(&value)?;
                let key = canonical_json_key(&serialized);
                let group = groups.entry(key).or_insert_with(|| (0.0, value));
                group.0 += weight;
                outcomes.push(VoteOutcome {
                    model,
                    weight,
                    result: Ok(serialized),
                });
            }
            Err(error) => outcomes.push(VoteOutcome {
                model,
                weight,
                result: Err(error),
            }),
        }
    }

    let winner = groups
        .into_values()
        .max_by(|left, right| left.0.total_cmp(&right.0));
    let (decision, confidence) = match winner {
        Some((weight, value)) if total_weight > 0.0 && weight > total_weight / 2.0 => {
            (Some(value), weight / total_weight)
        }
        _ => (None, 0.0),
    };

    Ok(Consensus {
        requires_human_review: decision.is_none(),
        decision,
        confidence,
        outcomes,
    })
}

fn canonical_json_key(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Object(object) => {
            let mut entries = object.iter().collect::<Vec<_>>();
            entries.sort_by(|left, right| left.0.cmp(right.0));
            let fields = entries
                .into_iter()
                .map(|(key, value)| {
                    format!(
                        "{}:{}",
                        serde_json::to_string(key).unwrap_or_default(),
                        canonical_json_key(value)
                    )
                })
                .collect::<Vec<_>>();
            format!("{{{}}}", fields.join(","))
        }
        serde_json::Value::Array(values) => format!(
            "[{}]",
            values
                .iter()
                .map(canonical_json_key)
                .collect::<Vec<_>>()
                .join(",")
        ),
        _ => serde_json::to_string(value).unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_high_risk(value: &serde_json::Value) -> bool {
        value.get("risk").and_then(serde_json::Value::as_str) == Some("high")
    }

    #[tokio::test]
    async fn pipeline_stage_implements_agent_run_stream_and_tool_contract() {
        let agent = PipelineStage::new("architect", |mut input| async move {
            input["planned"] = serde_json::json!(true);
            Ok(input)
        })
        .with_tools(vec![AgentTool::new(
            "plan_decision_model",
            "生成结构化决策方案",
        )]);

        assert_eq!(agent.name(), "architect");
        assert_eq!(agent.tools()[0].name, "plan_decision_model");
        assert_eq!(
            agent.run(serde_json::json!({})).await.unwrap().output["planned"],
            true
        );

        let (sender, mut receiver) = mpsc::channel(2);
        let output = agent.stream(serde_json::json!({}), sender).await.unwrap();
        assert_eq!(output.output["planned"], true);
        assert_eq!(
            receiver.recv().await,
            Some(AgentEvent {
                agent: "architect".into(),
                status: AgentStatus::Running,
            })
        );
        assert_eq!(
            receiver.recv().await,
            Some(AgentEvent {
                agent: "architect".into(),
                status: AgentStatus::Succeeded,
            })
        );
    }

    #[tokio::test]
    async fn pipeline_passes_outputs_between_steps_and_takes_matching_branch() {
        let pipeline = vec![
            PipelineStage::new("analyze", |mut input| async move {
                input["risk"] = serde_json::json!("high");
                Ok(input)
            }),
            PipelineStage::branch(
                "risk_gate",
                is_high_risk,
                vec![PipelineStage::new("verify", |mut input| async move {
                    input["verification"] = serde_json::json!("required");
                    Ok(input)
                })],
                vec![PipelineStage::new("skip_verify", |mut input| async move {
                    input["verification"] = serde_json::json!("skipped");
                    Ok(input)
                })],
            ),
        ];

        let report = run_pipeline(pipeline, serde_json::json!({})).await.unwrap();
        assert_eq!(report.output["verification"], "required");
        assert_eq!(report.completed_steps, ["analyze", "verify", "risk_gate"]);
    }

    #[tokio::test]
    async fn streaming_pipeline_emits_each_agent_lifecycle_in_order() {
        let pipeline = vec![
            PipelineStage::new("architect", |input| async move { Ok(input) }),
            PipelineStage::new("validator", |input| async move { Ok(input) }),
        ];
        let (sender, mut receiver) = mpsc::channel(8);

        let report = run_pipeline_stream(pipeline, serde_json::json!({}), sender)
            .await
            .unwrap();
        let mut events = Vec::new();
        while let Some(event) = receiver.recv().await {
            events.push(event);
        }

        assert_eq!(report.completed_steps, ["architect", "validator"]);
        assert_eq!(
            events,
            [
                AgentEvent {
                    agent: "architect".into(),
                    status: AgentStatus::Running,
                },
                AgentEvent {
                    agent: "architect".into(),
                    status: AgentStatus::Succeeded,
                },
                AgentEvent {
                    agent: "validator".into(),
                    status: AgentStatus::Running,
                },
                AgentEvent {
                    agent: "validator".into(),
                    status: AgentStatus::Succeeded,
                },
            ]
        );
    }

    #[tokio::test]
    async fn streaming_pipeline_emits_failed_status_and_preserves_failure() {
        let pipeline = vec![PipelineStage::new("validator", |_| async {
            Err("invalid plan".into())
        })];
        let (sender, mut receiver) = mpsc::channel(4);

        let failure = run_pipeline_stream(pipeline, serde_json::json!({}), sender)
            .await
            .unwrap_err();
        let mut events = Vec::new();
        while let Some(event) = receiver.recv().await {
            events.push(event);
        }

        assert_eq!(failure.failed_step, "validator");
        assert_eq!(failure.message, "invalid plan");
        assert_eq!(
            events,
            [
                AgentEvent {
                    agent: "validator".into(),
                    status: AgentStatus::Running,
                },
                AgentEvent {
                    agent: "validator".into(),
                    status: AgentStatus::Failed,
                },
            ]
        );
    }

    #[tokio::test]
    async fn branch_failure_preserves_nested_failure_and_completed_steps() {
        let pipeline = vec![
            PipelineStage::new("analyze", |input| async move { Ok(input) }),
            PipelineStage::branch(
                "risk_gate",
                |_| true,
                vec![
                    PipelineStage::new("inspect", |input| async move { Ok(input) }),
                    PipelineStage::new("validate", |_| async { Err("invalid plan".into()) }),
                ],
                Vec::new(),
            ),
        ];

        let failure = run_pipeline(pipeline, serde_json::json!({}))
            .await
            .unwrap_err();

        assert_eq!(failure.failed_step, "validate");
        assert_eq!(failure.message, "invalid plan");
        assert_eq!(failure.completed_steps, ["analyze", "inspect"]);
    }

    #[tokio::test]
    async fn pipeline_stops_on_failure_and_reports_completed_steps() {
        let pipeline = vec![
            PipelineStage::new("analyze", |input| async move { Ok(input) }),
            PipelineStage::new("plan", |_| async { Err("invalid plan".into()) }),
            PipelineStage::new("generate_dsl", |input| async move { Ok(input) }),
        ];

        let failure = run_pipeline(pipeline, serde_json::json!({}))
            .await
            .unwrap_err();
        assert_eq!(failure.failed_step, "plan");
        assert_eq!(failure.message, "invalid plan");
        assert_eq!(failure.completed_steps, ["analyze"]);
    }

    #[tokio::test]
    async fn timed_out_stage_is_cancelled_and_later_stages_do_not_run() {
        struct DropFlag(Arc<std::sync::atomic::AtomicBool>);

        impl Drop for DropFlag {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }

        let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let observed = Arc::clone(&cancelled);
        let later_ran = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let later_ran_flag = Arc::clone(&later_ran);
        let pipeline = vec![
            PipelineStage::new("completed", |value| async move { Ok(value) }),
            PipelineStage::new("slow_agent", move |value| {
                let observed = Arc::clone(&observed);
                async move {
                    let _drop_flag = DropFlag(observed);
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    Ok(value)
                }
            })
            .with_timeout(Duration::from_millis(1)),
            PipelineStage::new("must_not_run", move |value| {
                later_ran_flag.store(true, Ordering::SeqCst);
                async move { Ok(value) }
            }),
        ];

        let failure = run_pipeline(pipeline, serde_json::json!({}))
            .await
            .unwrap_err();
        assert_eq!(failure.failed_step, "slow_agent");
        assert_eq!(failure.message, "step timed out after 1 ms");
        assert_eq!(failure.completed_steps, ["completed"]);
        assert!(cancelled.load(Ordering::SeqCst));
        assert!(!later_ran.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn recursive_refinement_processes_children_in_stable_order() {
        let results = refine_recursively(
            "root".to_owned(),
            RefinementLimits {
                max_depth: 3,
                max_children_per_split: 4,
                max_total_tasks: 8,
            },
            |task, _depth| async move {
                if task == "root" {
                    Ok(RefinementDecision::Split(vec![
                        "left".into(),
                        "right".into(),
                    ]))
                } else {
                    Ok(RefinementDecision::Complete(format!("{task}-done")))
                }
            },
        )
        .await
        .unwrap();

        assert_eq!(results, ["left-done", "right-done"]);
    }

    #[tokio::test]
    async fn recursive_refinement_enforces_depth_and_task_budgets() {
        let limits = RefinementLimits {
            max_depth: 1,
            max_children_per_split: 2,
            max_total_tasks: 3,
        };
        let depth_failure = refine_recursively("root", limits, |task, _depth| async move {
            Ok(RefinementDecision::Split(vec![task]))
        })
        .await
        .unwrap_err();
        assert_eq!(depth_failure.depth, 1);
        assert!(depth_failure.reason.contains("maximum depth"));

        let task_failure = refine_recursively(
            "root",
            RefinementLimits {
                max_depth: 2,
                ..limits
            },
            |task, _depth| async move {
                if task == "root" {
                    Ok(RefinementDecision::Split(vec!["a", "b"]))
                } else {
                    Ok(RefinementDecision::Split(vec!["c", "d"]))
                }
            },
        )
        .await
        .unwrap_err();
        assert!(task_failure.reason.contains("task count"));
    }

    #[tokio::test]
    async fn recursive_refinement_combines_child_results_at_each_parent() {
        let result =
            refine_and_combine(
                "root".to_owned(),
                RefinementLimits {
                    max_depth: 3,
                    max_children_per_split: 3,
                    max_total_tasks: 8,
                },
                |task, _depth| async move {
                    match task.as_str() {
                        "root" => Ok(RefinementDecision::Split(vec![
                            "left".into(),
                            "right".into(),
                        ])),
                        "left" => Ok(RefinementDecision::Split(vec![
                            "left-1".into(),
                            "left-2".into(),
                        ])),
                        _ => Ok(RefinementDecision::Complete(task)),
                    }
                },
                |parent, children, _depth| async move {
                    Ok(format!("{parent}({})", children.join(",")))
                },
            )
            .await
            .unwrap();

        assert_eq!(result, "root(left(left-1,left-2),right)");
    }

    #[tokio::test]
    async fn parallel_refinement_respects_limit_and_preserves_child_order() {
        let active = Arc::new(AtomicUsize::new(0));
        let max_active = Arc::new(AtomicUsize::new(0));
        let refine_active = Arc::clone(&active);
        let refine_max_active = Arc::clone(&max_active);
        let result =
            refine_and_combine_parallel(
                "root".to_owned(),
                RefinementLimits {
                    max_depth: 1,
                    max_children_per_split: 4,
                    max_total_tasks: 5,
                },
                2,
                move |task, _depth| {
                    let active = Arc::clone(&refine_active);
                    let max_active = Arc::clone(&refine_max_active);
                    async move {
                        if task == "root" {
                            return Ok(RefinementDecision::Split(vec![
                                "first".into(),
                                "second".into(),
                                "third".into(),
                            ]));
                        }
                        let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                        max_active.fetch_max(current, Ordering::SeqCst);
                        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                        active.fetch_sub(1, Ordering::SeqCst);
                        Ok(RefinementDecision::Complete(task))
                    }
                },
                |parent, children, _depth| async move {
                    Ok(format!("{parent}({})", children.join(",")))
                },
            )
            .await
            .unwrap();

        assert_eq!(result, "root(first,second,third)");
        assert!(max_active.load(Ordering::SeqCst) > 1);
        assert!(max_active.load(Ordering::SeqCst) <= 2);
    }

    fn models() -> Vec<VoteCandidate> {
        ["model-a", "model-b", "model-c"]
            .into_iter()
            .map(|model| VoteCandidate {
                model: model.into(),
                weight: 1.0,
            })
            .collect()
    }

    #[tokio::test]
    async fn majority_consensus_returns_winning_result() {
        let consensus = parallel_vote(models(), |model| async move {
            Ok::<_, String>(if model == "model-c" {
                "other"
            } else {
                "approved"
            })
        })
        .await
        .unwrap();

        assert_eq!(consensus.decision, Some("approved"));
        assert!(!consensus.requires_human_review);
        assert_eq!(consensus.confidence, 2.0 / 3.0);
    }

    #[tokio::test]
    async fn tie_or_insufficient_successes_requires_review() {
        let tied = parallel_vote(models(), |model| async move { Ok::<_, String>(model) })
            .await
            .unwrap();
        assert!(tied.decision.is_none());
        assert!(tied.requires_human_review);

        let partial = parallel_vote(models(), |model| async move {
            if model == "model-c" || model == "model-b" {
                Err("unavailable".into())
            } else {
                Ok("one vote")
            }
        })
        .await
        .unwrap();
        assert!(partial.decision.is_none());
        assert!(partial.requires_human_review);
    }

    #[tokio::test]
    async fn weighted_voting_uses_candidate_weights() {
        let candidates = vec![
            VoteCandidate {
                model: "major".into(),
                weight: 3.0,
            },
            VoteCandidate {
                model: "minor-a".into(),
                weight: 1.0,
            },
            VoteCandidate {
                model: "minor-b".into(),
                weight: 1.0,
            },
        ];
        let consensus = parallel_vote(candidates, |model| async move {
            Ok::<_, String>(if model == "major" { "yes" } else { "no" })
        })
        .await
        .unwrap();

        assert_eq!(consensus.decision, Some("yes"));
        assert_eq!(consensus.confidence, 0.6);
    }

    #[tokio::test]
    async fn duplicate_models_cannot_create_false_consensus() {
        let candidates = vec![
            VoteCandidate {
                model: "shared-model".into(),
                weight: 1.0,
            },
            VoteCandidate {
                model: "shared-model".into(),
                weight: 1.0,
            },
        ];
        let result = parallel_vote(candidates, |model| async move { Ok::<_, String>(model) }).await;

        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("不能重复使用同一模型")
        );
    }

    #[tokio::test]
    async fn equivalent_json_objects_reach_consensus_regardless_of_field_order() {
        let consensus = parallel_vote(models(), |model| async move {
            let json = if model == "model-a" {
                r#"{"name":"Discount","goal":"pricing"}"#
            } else {
                r#"{"goal":"pricing","name":"Discount"}"#
            };
            Ok::<_, String>(serde_json::from_str::<serde_json::Value>(json).unwrap())
        })
        .await
        .unwrap();

        assert!(consensus.decision.is_some());
        assert_eq!(consensus.confidence, 1.0);
        assert!(!consensus.requires_human_review);
    }
}
