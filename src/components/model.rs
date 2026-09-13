use leptos::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DependencyNode {
    pub id: String,
    pub name: String,
    pub kind: DependencyNodeKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DependencyNodeKind {
    Input,
    Decision,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DependencyEdge {
    pub from: String,
    pub to: String,
}

/// Accessible SVG dependency graph. SVG keeps the graph crisp on mobile and
/// allows keyboard users to inspect individual nodes via focusable labels.
#[component]
pub fn DependencyGraph(nodes: Vec<DependencyNode>, edges: Vec<DependencyEdge>) -> impl IntoView {
    view! {
        <section class="dependency-graph-container" aria-label="决策依赖关系图">
            <svg class="dependency-graph-svg" viewBox="0 0 800 420" role="img">
                <For each=move || edges.clone() key=|edge| format!("{}:{}", edge.from, edge.to) children=move |edge| view! {
                    <line class="dependency-edge" x1="40" y1="40" x2="760" y2="380" data-from=edge.from data-to=edge.to/>
                }/>
                <For each=move || nodes.clone() key=|node| node.id.clone() children=move |node| {
                    let kind = match node.kind { DependencyNodeKind::Input => "input", DependencyNodeKind::Decision => "decision" };
                    let name = node.name.clone();
                    view! { <g class=format!("dependency-node dependency-node-{kind}") tabindex="0" role="img" aria-label=name.clone()><circle cx="400" cy="210" r="30"/><text x="400" y="260" text-anchor="middle">{name.clone()}</text></g> }
                }/>
            </svg>
            <div class="graph-legend"><span><i class="legend-color legend-input"></i>"输入数据"</span><span><i class="legend-color legend-decision"></i>"决策节点"</span></div>
        </section>
    }
}
