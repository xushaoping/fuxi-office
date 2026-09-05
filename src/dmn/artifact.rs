use super::compile::CompiledDmnModel;
use super::error::{DmnError, Result};
use super::model::DmnModel;
use super::result::DmnResult;
use super::runtime::DmnExecutor;
use super::xml::parse_dmn;
use crate::feel::FeelValue;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const COMPILED_DMN_ARTIFACT_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ArtifactEnvelope {
    format_version: u32,
    model: DmnModel,
}

#[derive(Serialize)]
struct ArtifactEnvelopeRef<'a> {
    format_version: u32,
    model: &'a DmnModel,
}

#[derive(Debug, Clone)]
pub struct OwnedCompiledDmnModel {
    compiled: CompiledDmnModel<'static>,
    artifact_json: String,
}

impl OwnedCompiledDmnModel {
    #[allow(dead_code)] // 编译态模型访问器：回归用例当前经 evaluate_* 断言结果，暂无直接调用
    pub fn model(&self) -> &DmnModel {
        self.compiled.model()
    }

    pub fn to_artifact_json(&self) -> Result<String> {
        Ok(self.artifact_json.clone())
    }

    pub fn from_artifact_json(input: &str) -> Result<Self> {
        let artifact: ArtifactEnvelope =
            serde_json::from_str(input).map_err(|error| DmnError::Artifact(error.to_string()))?;
        if artifact.format_version != COMPILED_DMN_ARTIFACT_VERSION {
            return Err(DmnError::Artifact(format!(
                "unsupported format version {}",
                artifact.format_version
            )));
        }
        Self::compile(artifact.model)
    }

    pub fn evaluate_by_id(&self, id: &str, inputs: HashMap<String, FeelValue>) -> DmnResult {
        DmnExecutor::from_compiled(&self.compiled).evaluate_by_id(id, inputs)
    }

    #[allow(dead_code)] // 按名称执行预留 API（与 evaluate_by_id 对称），回归用例暂未覆盖
    pub fn evaluate_by_name(&self, name: &str, inputs: HashMap<String, FeelValue>) -> DmnResult {
        DmnExecutor::from_compiled(&self.compiled).evaluate_by_name(name, inputs)
    }

    #[allow(dead_code)] // 全量执行预留 API，回归用例暂未覆盖
    pub fn evaluate_all(&self, inputs: HashMap<String, FeelValue>) -> DmnResult {
        DmnExecutor::from_compiled(&self.compiled).evaluate_all(inputs)
    }

    pub fn evaluate_decision_service(
        &self,
        service: &str,
        inputs: HashMap<String, FeelValue>,
    ) -> DmnResult {
        DmnExecutor::from_compiled(&self.compiled).evaluate_decision_service(service, inputs)
    }

    fn compile(model: DmnModel) -> Result<Self> {
        let compiled = CompiledDmnModel::compile_owned(model)?;
        let artifact_json = serde_json::to_string(&ArtifactEnvelopeRef {
            format_version: COMPILED_DMN_ARTIFACT_VERSION,
            model: compiled.model(),
        })
        .map_err(|error| DmnError::Artifact(error.to_string()))?;
        Ok(Self {
            compiled,
            artifact_json,
        })
    }
}

pub fn compile_owned(model: DmnModel) -> Result<OwnedCompiledDmnModel> {
    OwnedCompiledDmnModel::compile(model)
}

pub fn compile_dmn_xml(xml: &str) -> Result<OwnedCompiledDmnModel> {
    compile_owned(parse_dmn(xml)?)
}
