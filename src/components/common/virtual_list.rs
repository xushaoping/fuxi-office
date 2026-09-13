use leptos::prelude::*;

/// Renders only the rows intersecting the scroll viewport. `item_height` must
/// be fixed for predictable offsets; callers can still provide rich row views.
#[component]
pub fn VirtualList<T, F, IV>(
    items: Vec<T>,
    item_height: f64,
    #[prop(optional, default = 420.0)] container_height: f64,
    render_item: F,
) -> impl IntoView
where
    T: Clone + Send + 'static,
    F: Fn(T) -> IV + Clone + Send + 'static,
    IV: IntoView + 'static,
{
    let scroll_top = RwSignal::new(0.0_f64);
    let container_ref = NodeRef::<leptos::html::Div>::new();
    let total_height = items.len() as f64 * item_height;
    let visible = move || {
        let start = (scroll_top.get() / item_height.max(1.0)).floor() as usize;
        let count = (container_height / item_height.max(1.0)).ceil() as usize + 2;
        let end = (start + count).min(items.len());
        (start..end)
            .map(|index| (index, items[index].clone()))
            .collect::<Vec<_>>()
    };
    view! {
        <div class="virtual-list-container" node_ref=container_ref style=format!("height:{container_height}px; overflow-y:auto;") on:scroll=move |_| {
            if let Some(element) = container_ref.get() { scroll_top.set(element.scroll_top() as f64); }
        }>
            <div class="virtual-list-spacer" style=format!("height:{total_height}px; position:relative;")>
                <For each=visible key=|(index, _)| *index children=move |(index, item)| {
                    let top = index as f64 * item_height;
                    let render = render_item.clone();
                    view! { <div class="virtual-list-item" style=format!("position:absolute; top:{top}px; height:{item_height}px; width:100%;")>{render(item)}</div> }
                }/>
            </div>
        </div>
    }
}
