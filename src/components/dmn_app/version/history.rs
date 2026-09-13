use leptos::prelude::*;

use crate::components::dmn_app::team::RollbackPanel;
use crate::models::{DecisionVersion, VersionChangeReview};

use super::SavedVersionReviewView;

#[component]
pub fn VersionHistoryPanel(
    selected_version_review: ReadSignal<Option<VersionChangeReview>>,
    selected_version_id: ReadSignal<Option<i64>>,
    versions: ReadSignal<Vec<DecisionVersion>>,
    current_version: ReadSignal<Option<DecisionVersion>>,
    rollback_reason: ReadSignal<String>,
    set_rollback_reason: WriteSignal<String>,
    rollback_confirmed: ReadSignal<bool>,
    set_rollback_confirmed: WriteSignal<bool>,
    rollback_busy: ReadSignal<bool>,
    on_create_rollback: Callback<()>,
    on_close: Callback<()>,
) -> impl IntoView {
    let can_rollback = move || {
        selected_version_id.get().is_some_and(|selected_id| {
            versions.get().iter().any(|version| {
                version.id == selected_id
                    && matches!(version.status.as_str(), "published" | "retired")
                    && current_version
                        .get()
                        .is_none_or(|current| current.id != version.id)
            })
        })
    };

    view! {
        <section class="version-history-panel">
            <div class="version-history-heading">
                <div>
                    <span class="yaml-kicker">"VERSION HISTORY"</span>
                    <strong>"已保存版本审查"</strong>
                </div>
                <button class="button button-quiet" on:click=move |_| on_close.run(())>
                    "关闭"
                </button>
            </div>
            {move || selected_version_review.get().map(|review| view! {
                <SavedVersionReviewView review=review />
            })}
            <Show when=can_rollback>
                <RollbackPanel
                    rollback_reason=rollback_reason
                    set_rollback_reason=set_rollback_reason
                    rollback_confirmed=rollback_confirmed
                    set_rollback_confirmed=set_rollback_confirmed
                    rollback_busy=rollback_busy
                    on_create_rollback=on_create_rollback
                />
            </Show>
        </section>
    }
}
