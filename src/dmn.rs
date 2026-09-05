//! DMN 1.5 executable model, compiler, and runtime.

mod compile;
mod diagnostics;
mod error;
mod model;
mod result;
mod runtime;
mod xml;
// 已编译工件序列化与模型 XML 序列化：仅由 dmn/tests.rs 往返回归测试引用，
// 产品执行链路（解析→编译→求值）不消费，因此只在测试构建中编译。
#[cfg(test)]
mod artifact;
#[cfg(test)]
mod serialize;

pub use compile::compile_dmn;
pub(crate) use compile::validate_generated_feel_name_bindings;
pub use model::{DecisionLogic, DecisionService, DmnModel, HitPolicy, ItemDefinition};
pub use result::{DecisionStatus, DmnResult, DmnResultStatus};
pub use runtime::DmnExecutor;
pub use xml::parse_dmn;

// 以下再导出仅被单元测试模块引用（dmn/tests.rs、nl_dsl/dmn.rs、projects 等），
// 非测试构建视为未使用，故限定在测试构建中提供。
#[cfg(test)]
pub use artifact::{OwnedCompiledDmnModel, compile_dmn_xml, compile_owned};
#[cfg(test)]
pub use compile::CompiledDmnModel;
#[cfg(test)]
pub use error::DmnError;
#[cfg(test)]
pub use model::{DMN_20231201_NAMESPACE, DMN_NAMESPACE};

#[cfg(test)]
mod tests;
