use leptos::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuleHitData {
    pub rule_id: String,
    pub rule_name: String,
    pub hit_count: u32,
}

/// Compact, responsive rule-hit heatmap for analytics pages.
#[component]
pub fn ExecutionHeatmap(rule_hits: Vec<RuleHitData>) -> impl IntoView {
    let max_hits = rule_hits
        .iter()
        .map(|rule| rule.hit_count)
        .max()
        .unwrap_or(1);
    view! {
        <section class="heatmap-container" aria-labelledby="heatmap-title">
            <h3 id="heatmap-title">"规则命中热图"</h3>
            <div class="heatmap-grid">
                <For each=move || rule_hits.clone() key=|rule| rule.rule_id.clone() children=move |rule| {
                    let intensity = (rule.hit_count.saturating_mul(100) / max_hits).min(100);
                    let name = rule.rule_name.clone();
                    view! {
                        <div class="heatmap-cell" style=format!("--heat-intensity: {intensity}%") title=format!("{}: {} 次命中", name, rule.hit_count)>
                            <span class="cell-label">{name.clone()}</span><span class="cell-count">{rule.hit_count}</span>
                        </div>
                    }
                }/>
            </div>
            <div class="heatmap-scale" aria-label="命中次数从低到高"><span>"低"</span><span class="scale-gradient"></span><span>"高"</span></div>
        </section>
    }
}
