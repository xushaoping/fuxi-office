//! DMN 1.5 executable model, compiler, and runtime.

mod artifact;
mod compile;
mod diagnostics;
mod error;
mod model;
mod result;
mod runtime;
mod serialize;
mod xml;

pub use artifact::{OwnedCompiledDmnModel, compile_dmn_xml, compile_owned};
pub(crate) use compile::validate_generated_feel_name_bindings;
pub use compile::{CompiledDmnModel, compile_dmn};
pub use error::DmnError;
pub use model::{
    DMN_20231201_NAMESPACE, DMN_NAMESPACE, DecisionLogic, DecisionService, DmnModel, HitPolicy,
    ItemDefinition,
};
pub use result::{DecisionStatus, DmnResult, DmnResultStatus};
pub use runtime::DmnExecutor;
pub use xml::parse_dmn;

#[cfg(test)]
mod tests;
