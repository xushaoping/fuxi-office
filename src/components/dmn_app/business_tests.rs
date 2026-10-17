use leptos::prelude::*;

use crate::models::DecisionTestCase;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TestCaseFilter {
    All,
    Failed,
    PendingApproval,
    PendingRun,
}

impl TestCaseFilter {
    pub fn matches(self, test_case: &DecisionTestCase) -> bool {
        match self {
            Self::All => true,
            Self::Failed => matches!(test_case.status.as_str(), "failed" | "error"),
            Self::PendingApproval => {
                test_case.status == "passed" && test_case.approved_by.is_none()
            }
            Self::PendingRun => !matches!(test_case.status.as_str(), "passed" | "failed" | "error"),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "全部",
            Self::Failed => "失败 / 错误",
            Self::PendingApproval => "待批准",
            Self::PendingRun => "待运行",
        }
    }
}

#[component]
pub fn BusinessTestCaseSummary(
    cases: ReadSignal<Vec<DecisionTestCase>>,
    filter: ReadSignal<TestCaseFilter>,
    set_filter: WriteSignal<TestCaseFilter>,
) -> impl IntoView {
    view! {
        <div class="business-test-stats" aria-label="验收统计">
            <div><strong>{move || cases.get().len()}</strong><span>"总数"</span></div>
            <div class="is-success"><strong>{move || cases.get().iter().filter(|case| case.status == "passed").count()}</strong><span>"通过"</span></div>
            <div class="is-danger"><strong>{move || cases.get().iter().filter(|case| matches!(case.status.as_str(), "failed" | "error")).count()}</strong><span>"失败 / 错误"</span></div>
            <div><strong>{move || cases.get().iter().filter(|case| !matches!(case.status.as_str(), "passed" | "failed" | "error")).count()}</strong><span>"待运行"</span></div>
            <div class="is-warning"><strong>{move || cases.get().iter().filter(|case| case.status == "passed" && case.approved_by.is_none()).count()}</strong><span>"待批准"</span></div>
        </div>
        <div class="business-test-filters" role="group" aria-label="筛选验收案例">
            {[
                TestCaseFilter::All,
                TestCaseFilter::Failed,
                TestCaseFilter::PendingApproval,
                TestCaseFilter::PendingRun,
            ].into_iter().map(|case_filter| view! {
                <button
                    class=move || if filter.get() == case_filter { "button button-quiet is-active" } else { "button button-quiet" }
                    aria-pressed=move || filter.get() == case_filter
                    aria-controls="business-test-case-list"
                    on:click=move |_| set_filter.set(case_filter)
                >
                    {case_filter.label()}
                    <span>{move || cases.get().iter().filter(|case| case_filter.matches(case)).count()}</span>
                </button>
            }).collect_view()}
        </div>
    }
}
