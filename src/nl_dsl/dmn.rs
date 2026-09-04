use crate::nl_dsl::*;
use dmn::Definitions;
use quick_xml::Writer;
use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};
use std::io::Cursor;

const DMN_NS: &str = "https://www.omg.org/spec/DMN/20231201/MODEL/";
const DMNDI_NS: &str = "https://www.omg.org/spec/DMN/20231201/DMNDI/";
const DI_NS: &str = "https://www.omg.org/spec/DMN/20231201/DI/";
const DC_NS: &str = "https://www.omg.org/spec/DMN/20231201/DC/";
const FEEL_NS: &str = "https://www.omg.org/spec/DMN/20231201/FEEL/";

/// quick-xml 写入器别名
type XmlWriter = Writer<Cursor<Vec<u8>>>;

pub fn generate_dmn_xml(defs: &Definitions) -> anyhow::Result<String> {
    let mut writer = Writer::new_with_indent(Cursor::new(Vec::new()), b' ', 2);
    writer.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))?;

    let mut defs_start = BytesStart::new("dmn:definitions");
    defs_start.push_attribute(("id", "definitions_1"));
    defs_start.push_attribute(("name", defs.name.as_str()));
    defs_start.push_attribute(("namespace", defs.namespace.as_str()));
    defs_start.push_attribute(("xmlns:dmn", DMN_NS));
    defs_start.push_attribute(("xmlns:dmndi", DMNDI_NS));
    defs_start.push_attribute(("xmlns:di", DI_NS));
    defs_start.push_attribute(("xmlns:dc", DC_NS));
    defs_start.push_attribute(("xmlns:feel", FEEL_NS));
    writer.write_event(Event::Start(defs_start))?;

    let mut dmn_gen = DmnGenerator::new();
    dmn_gen.generate_body(defs, &mut writer)?;

    writer.write_event(Event::End(BytesEnd::new("dmn:definitions")))?;

    let result = String::from_utf8(writer.into_inner().into_inner())?;
    Ok(result)
}

struct DmnGenerator {
    counter: usize,
    id_map: std::collections::HashMap<String, String>,
    relationship_ids: std::collections::HashMap<(String, String), String>,
}

impl DmnGenerator {
    fn new() -> Self {
        Self {
            counter: 0,
            id_map: std::collections::HashMap::new(),
            relationship_ids: std::collections::HashMap::new(),
        }
    }

    fn next_id(&mut self, prefix: &str) -> String {
        self.counter += 1;
        format!("{}_{}", prefix, self.counter)
    }

    fn generate_body(&mut self, defs: &Definitions, w: &mut XmlWriter) -> anyhow::Result<()> {
        // 预分配 ID
        for item in &defs.item_definitions {
            let id = self.next_id("itemDefinition");
            self.id_map.insert(format!("itemDef:{}", item.name), id);
        }
        for input in &defs.inputs {
            let id = self.next_id("inputData");
            self.id_map.insert(format!("input:{}", input.name), id);
        }
        for dec in &defs.decisions {
            let id = self.next_id("decision");
            self.id_map.insert(format!("decision:{}", dec.name), id);
        }
        for bkm in &defs.bkms {
            let id = self.next_id("bkm");
            self.id_map.insert(format!("bkm:{}", bkm.name), id);
        }
        for ds in &defs.decision_services {
            let id = self.next_id("decisionService");
            self.id_map.insert(format!("ds:{}", ds.name), id);
        }
        for ks in &defs.knowledge_sources {
            let id = self.next_id("knowledgeSource");
            self.id_map.insert(format!("ks:{}", ks.name), id);
        }
        // 优先生成 Import 声明
        for imp in &defs.imports {
            let mut start = BytesStart::new("dmn:import");
            start.push_attribute(("name", imp.name.as_str()));
            start.push_attribute(("namespace", imp.namespace.as_str()));
            start.push_attribute(("locationURI", imp.location.as_str()));
            start.push_attribute(("importType", imp.import_type.as_str()));
            w.write_event(Event::Empty(start))?;
        }
        if let Some(d) = &defs.description {
            write_text(w, "dmn:description", d)?;
        }

        for item in &defs.item_definitions {
            self.write_item_definition(item, w)?;
        }
        for input in &defs.inputs {
            self.write_input_data(input, w)?;
        }
        for dec in &defs.decisions {
            self.write_decision(dec, w)?;
        }
        for bkm in &defs.bkms {
            self.write_bkm(bkm, w)?;
        }
        for ds in &defs.decision_services {
            self.write_decision_service(ds, w)?;
        }
        for ks in &defs.knowledge_sources {
            self.write_knowledge_source(ks, w)?;
        }
        for ta in &defs.text_annotations {
            self.write_text_annotation(ta, w)?;
        }

        if let Some(diagram) = &defs.diagram {
            self.write_diagram(diagram, defs, w)?;
        }
        Ok(())
    }

    // ---------- 元素写入 ----------

    fn write_item_definition(
        &mut self,
        item: &ItemDefinition,
        w: &mut XmlWriter,
    ) -> anyhow::Result<()> {
        let id = self.id_map[&format!("itemDef:{}", item.name)].clone();
        let mut start = BytesStart::new("dmn:itemDefinition");
        start.push_attribute(("id", id.as_str()));
        start.push_attribute(("name", item.name.as_str()));
        let is_collection = item.is_collection || matches!(item.type_, ItemType::List { .. });
        start.push_attribute(("isCollection", is_collection.to_string().as_str()));
        w.write_event(Event::Start(start))?;

        self.write_item_type(&item.type_, w)?;
        w.write_event(Event::End(BytesEnd::new("dmn:itemDefinition")))?;
        Ok(())
    }

    fn write_item_type(&mut self, item_type: &ItemType, w: &mut XmlWriter) -> anyhow::Result<()> {
        match item_type {
            ItemType::Simple { type_ref } => {
                write_text(w, "dmn:typeRef", &prefix_type(type_ref))?;
            }
            ItemType::Structure { fields } => {
                for f in fields {
                    let mut comp = BytesStart::new("dmn:itemComponent");
                    comp.push_attribute(("name", f.name.as_str()));
                    comp.push_attribute(("typeRef", prefix_type(&f.type_).as_str()));
                    if f.is_collection {
                        comp.push_attribute(("isCollection", "true"));
                    }
                    w.write_event(Event::Empty(comp))?;
                }
            }
            ItemType::List { item_type } => {
                if matches!(item_type.as_ref(), ItemType::List { .. }) {
                    anyhow::bail!("嵌套列表类型暂不支持生成 DMN XML");
                }
                self.write_item_type(item_type, w)?;
            }
            ItemType::Enumeration { type_ref, values } => {
                write_text(w, "dmn:typeRef", &prefix_type(type_ref))?;
                w.write_event(Event::Start(BytesStart::new("dmn:allowedValues")))?;
                let values = values
                    .iter()
                    .map(|value| format!("\"{}\"", escape_feel_string(value)))
                    .collect::<Vec<_>>()
                    .join(", ");
                write_text(w, "dmn:text", &values)?;
                w.write_event(Event::End(BytesEnd::new("dmn:allowedValues")))?;
            }
        }
        Ok(())
    }

    fn write_input_data(&mut self, input: &InputData, w: &mut XmlWriter) -> anyhow::Result<()> {
        let id = self.id_map[&format!("input:{}", input.name)].clone();
        let mut start = BytesStart::new("dmn:inputData");
        start.push_attribute(("id", id.as_str()));
        start.push_attribute(("name", input.name.as_str()));
        w.write_event(Event::Start(start))?;

        if let Some(d) = &input.description {
            write_text(w, "dmn:description", d)?;
        }
        let mut var = BytesStart::new("dmn:variable");
        var.push_attribute(("name", input.name.as_str()));
        var.push_attribute((
            "typeRef",
            prefix_type(input.type_ref.as_deref().unwrap_or("Any")).as_str(),
        ));
        w.write_event(Event::Empty(var))?;

        w.write_event(Event::End(BytesEnd::new("dmn:inputData")))?;
        Ok(())
    }

    fn write_decision(&mut self, dec: &Decision, w: &mut XmlWriter) -> anyhow::Result<()> {
        let id = self.id_map[&format!("decision:{}", dec.name)].clone();
        let mut start = BytesStart::new("dmn:decision");
        start.push_attribute(("id", id.as_str()));
        start.push_attribute(("name", dec.name.as_str()));
        w.write_event(Event::Start(start))?;

        if let Some(d) = &dec.description {
            write_text(w, "dmn:description", d)?;
        }
        let mut var = BytesStart::new("dmn:variable");
        var.push_attribute(("name", dec.name.as_str()));
        var.push_attribute((
            "typeRef",
            prefix_type(dec.type_ref.as_deref().unwrap_or("Any")).as_str(),
        ));
        w.write_event(Event::Empty(var))?;

        // Information Requirements
        for ri in &dec.required_inputs {
            if let Some(target_id) = self.id_map.get(&format!("input:{}", ri)).cloned() {
                let relationship_id = self.next_id("informationRequirement");
                self.relationship_ids
                    .insert((ri.clone(), dec.name.clone()), relationship_id.clone());
                let mut requirement = BytesStart::new("dmn:informationRequirement");
                requirement.push_attribute(("id", relationship_id.as_str()));
                w.write_event(Event::Start(requirement))?;
                let mut req = BytesStart::new("dmn:requiredInput");
                req.push_attribute(("href", format!("#{}", target_id).as_str()));
                w.write_event(Event::Empty(req))?;
                w.write_event(Event::End(BytesEnd::new("dmn:informationRequirement")))?;
            }
        }
        for rd in &dec.required_decisions {
            if let Some(target_id) = self.id_map.get(&format!("decision:{}", rd)).cloned() {
                let relationship_id = self.next_id("informationRequirement");
                self.relationship_ids
                    .insert((rd.clone(), dec.name.clone()), relationship_id.clone());
                let mut requirement = BytesStart::new("dmn:informationRequirement");
                requirement.push_attribute(("id", relationship_id.as_str()));
                w.write_event(Event::Start(requirement))?;
                let mut req = BytesStart::new("dmn:requiredDecision");
                req.push_attribute(("href", format!("#{}", target_id).as_str()));
                w.write_event(Event::Empty(req))?;
                w.write_event(Event::End(BytesEnd::new("dmn:informationRequirement")))?;
            }
        }
        // Knowledge Requirements
        for rk in &dec.required_knowledge {
            if let Some(target_id) = self.id_map.get(&format!("bkm:{}", rk)).cloned() {
                let relationship_id = self.next_id("knowledgeRequirement");
                self.relationship_ids
                    .insert((rk.clone(), dec.name.clone()), relationship_id.clone());
                let mut requirement = BytesStart::new("dmn:knowledgeRequirement");
                requirement.push_attribute(("id", relationship_id.as_str()));
                w.write_event(Event::Start(requirement))?;
                let mut req = BytesStart::new("dmn:requiredKnowledge");
                req.push_attribute(("href", format!("#{}", target_id).as_str()));
                w.write_event(Event::Empty(req))?;
                w.write_event(Event::End(BytesEnd::new("dmn:knowledgeRequirement")))?;
            }
        }

        self.write_boxed_expression(&dec.logic, w)?;
        w.write_event(Event::End(BytesEnd::new("dmn:decision")))?;
        Ok(())
    }

    fn write_bkm(&mut self, bkm: &BusinessKnowledgeModel, w: &mut XmlWriter) -> anyhow::Result<()> {
        let id = self.id_map[&format!("bkm:{}", bkm.name)].clone();
        let mut start = BytesStart::new("dmn:businessKnowledgeModel");
        start.push_attribute(("id", id.as_str()));
        start.push_attribute(("name", bkm.name.as_str()));
        w.write_event(Event::Start(start))?;

        if let Some(d) = &bkm.description {
            write_text(w, "dmn:description", d)?;
        }
        let mut var = BytesStart::new("dmn:variable");
        var.push_attribute(("name", bkm.name.as_str()));
        var.push_attribute((
            "typeRef",
            prefix_type(bkm.type_ref.as_deref().unwrap_or("feel:Any")).as_str(),
        ));
        w.write_event(Event::Empty(var))?;

        w.write_event(Event::Start(BytesStart::new("dmn:encapsulatedLogic")))?;
        if !bkm.parameters.is_empty() {
            for p in &bkm.parameters {
                let mut fp = BytesStart::new("dmn:formalParameter");
                fp.push_attribute(("name", p.name.as_str()));
                fp.push_attribute(("typeRef", prefix_type(&p.type_ref).as_str()));
                if p.is_collection {
                    fp.push_attribute(("isCollection", "true"));
                }
                w.write_event(Event::Empty(fp))?;
            }
        }
        self.write_boxed_expression(&bkm.logic, w)?;
        w.write_event(Event::End(BytesEnd::new("dmn:encapsulatedLogic")))?;
        w.write_event(Event::End(BytesEnd::new("dmn:businessKnowledgeModel")))?;
        Ok(())
    }

    fn write_decision_service(
        &mut self,
        ds: &DecisionService,
        w: &mut XmlWriter,
    ) -> anyhow::Result<()> {
        let id = self.id_map[&format!("ds:{}", ds.name)].clone();
        let mut start = BytesStart::new("dmn:decisionService");
        start.push_attribute(("id", id.as_str()));
        start.push_attribute(("name", ds.name.as_str()));
        w.write_event(Event::Start(start))?;

        let mut var = BytesStart::new("dmn:variable");
        var.push_attribute(("name", ds.name.as_str()));
        w.write_event(Event::Empty(var))?;

        for d in &ds.output_decisions {
            let target_id = self.id_map[&format!("decision:{d}")].clone();
            let mut e = BytesStart::new("dmn:outputDecision");
            e.push_attribute(("href", format!("#{target_id}").as_str()));
            w.write_event(Event::Empty(e))?;
        }
        for d in &ds.encapsulated_decisions {
            let target_id = self.id_map[&format!("decision:{d}")].clone();
            let mut e = BytesStart::new("dmn:encapsulatedDecision");
            e.push_attribute(("href", format!("#{target_id}").as_str()));
            w.write_event(Event::Empty(e))?;
        }
        for i in &ds.input_data {
            let target_id = self.id_map[&format!("input:{i}")].clone();
            let mut e = BytesStart::new("dmn:inputData");
            e.push_attribute(("href", format!("#{target_id}").as_str()));
            w.write_event(Event::Empty(e))?;
        }

        w.write_event(Event::End(BytesEnd::new("dmn:decisionService")))?;
        Ok(())
    }

    fn write_knowledge_source(
        &mut self,
        ks: &KnowledgeSource,
        w: &mut XmlWriter,
    ) -> anyhow::Result<()> {
        let id = self.id_map[&format!("ks:{}", ks.name)].clone();
        let mut start = BytesStart::new("dmn:knowledgeSource");
        start.push_attribute(("id", id.as_str()));
        start.push_attribute(("name", ks.name.as_str()));
        w.write_event(Event::Start(start))?;

        if let Some(d) = &ks.description {
            write_text(w, "dmn:description", d)?;
        }
        if let Some(u) = &ks.url {
            write_text(w, "dmn:URI", u)?;
        }
        for target in &ks.authority_targets {
            if let Some(tid) = self.id_map.get(&format!("decision:{}", target)) {
                w.write_event(Event::Start(BytesStart::new("dmn:authorityRequirement")))?;
                let mut req = BytesStart::new("dmn:requiredDecision");
                req.push_attribute(("href", format!("#{}", tid).as_str()));
                w.write_event(Event::Empty(req))?;
                w.write_event(Event::End(BytesEnd::new("dmn:authorityRequirement")))?;
            }
        }

        w.write_event(Event::End(BytesEnd::new("dmn:knowledgeSource")))?;
        Ok(())
    }

    fn write_text_annotation(
        &mut self,
        ta: &TextAnnotation,
        w: &mut XmlWriter,
    ) -> anyhow::Result<()> {
        let id = self.next_id("textAnnotation");
        let mut start = BytesStart::new("dmn:textAnnotation");
        start.push_attribute(("id", id.as_str()));
        start.push_attribute(("textFormat", "text/plain"));
        w.write_event(Event::Start(start))?;
        write_text(w, "dmn:text", &ta.text)?;
        w.write_event(Event::End(BytesEnd::new("dmn:textAnnotation")))?;

        // 关联到目标元素
        let assoc_id = self.next_id("association");
        let source = self.resolve_element_id(&ta.target);
        let mut assoc = BytesStart::new("dmn:association");
        assoc.push_attribute(("id", assoc_id.as_str()));
        w.write_event(Event::Start(assoc))?;
        let mut src = BytesStart::new("dmn:sourceRef");
        src.push_attribute(("href", format!("#{}", source).as_str()));
        w.write_event(Event::Empty(src))?;
        let mut tgt = BytesStart::new("dmn:targetRef");
        tgt.push_attribute(("href", format!("#{}", id).as_str()));
        w.write_event(Event::Empty(tgt))?;
        w.write_event(Event::End(BytesEnd::new("dmn:association")))?;
        Ok(())
    }

    fn write_diagram(
        &mut self,
        diagram: &Diagram,
        defs: &Definitions,
        w: &mut Writer<Cursor<Vec<u8>>>,
    ) -> anyhow::Result<()> {
        w.write_event(Event::Start(BytesStart::new("dmndi:DMNDI")))?;
        let mut diag_start = BytesStart::new("dmndi:DMNDiagram");
        diag_start.push_attribute(("name", defs.name.as_str()));
        w.write_event(Event::Start(diag_start))?;

        // 1. 生成节点 Shape
        w.write_event(Event::Start(BytesStart::new("dmndi:DMNDiagramElement")))?;
        for node in &diagram.nodes {
            let element_id = self
                .id_map
                .get(&format!("decision:{}", node.element))
                .or_else(|| self.id_map.get(&format!("input:{}", node.element)))
                .cloned()
                .unwrap_or_default();

            let mut shape = BytesStart::new("dmndi:DMNShape");
            shape.push_attribute(("dmnElementRef", element_id.as_str()));
            w.write_event(Event::Start(shape))?;

            let mut bounds = BytesStart::new("dc:Bounds");
            bounds.push_attribute(("x", node.x.to_string().as_str()));
            bounds.push_attribute(("y", node.y.to_string().as_str()));
            bounds.push_attribute(("width", node.width.unwrap_or(180.0).to_string().as_str()));
            bounds.push_attribute(("height", node.height.unwrap_or(80.0).to_string().as_str()));
            w.write_event(Event::Empty(bounds))?;

            w.write_event(Event::End(BytesEnd::new("dmndi:DMNShape")))?;
        }
        w.write_event(Event::End(BytesEnd::new("dmndi:DMNDiagramElement")))?;

        // 2. 生成连线 Edge (核心优化点)
        w.write_event(Event::Start(BytesStart::new("dmndi:DMNDiagramElement")))?;
        for edge in &diagram.edges {
            let relationship_id = self
                .relationship_ids
                .get(&(edge.source.clone(), edge.target.clone()))
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "图连线 '{}' -> '{}' 没有对应的 DMN 依赖关系",
                        edge.source,
                        edge.target
                    )
                })?;
            let mut e_start = BytesStart::new("dmndi:DMNEdge");
            e_start.push_attribute(("dmnElementRef", relationship_id.as_str()));
            w.write_event(Event::Start(e_start))?;

            // 如果有 waypoints 则生成，否则取 source/target 节点中心直连
            let waypoints = if edge.waypoints.is_empty() {
                let sp = self.node_center(diagram, &edge.source);
                let tp = self.node_center(diagram, &edge.target);
                vec![sp, tp]
            } else {
                edge.waypoints.clone()
            };

            for (x, y) in waypoints {
                let mut wp = BytesStart::new("di:waypoint");
                wp.push_attribute(("x", x.to_string().as_str()));
                wp.push_attribute(("y", y.to_string().as_str()));
                w.write_event(Event::Empty(wp))?;
            }
            w.write_event(Event::End(BytesEnd::new("dmndi:DMNEdge")))?;
        }
        w.write_event(Event::End(BytesEnd::new("dmndi:DMNDiagramElement")))?;

        w.write_event(Event::End(BytesEnd::new("dmndi:DMNDiagram")))?;
        w.write_event(Event::End(BytesEnd::new("dmndi:DMNDI")))?;
        Ok(())
    }

    fn write_boxed_expression(
        &mut self,
        expr: &BoxedExpression,
        w: &mut XmlWriter,
    ) -> anyhow::Result<()> {
        match expr {
            BoxedExpression::DecisionTable {
                hit_policy,
                aggregation,
                preferred_orientation,
                inputs,
                outputs,
                rules,
            } => {
                let id = self.next_id("decisionTable");
                let mut start = BytesStart::new("dmn:decisionTable");
                start.push_attribute(("id", id.as_str()));
                start.push_attribute((
                    "hitPolicy",
                    hit_policy
                        .clone()
                        .unwrap_or_else(|| "UNIQUE".to_string())
                        .as_str(),
                ));
                if let Some(a) = aggregation {
                    start.push_attribute(("aggregation", a.as_str()));
                }
                if let Some(o) = preferred_orientation {
                    start.push_attribute(("preferredOrientation", o.as_str()));
                }
                w.write_event(Event::Start(start))?;

                for inp in inputs {
                    let iid = self.next_id("input");
                    let mut inp_start = BytesStart::new("dmn:input");
                    inp_start.push_attribute(("id", iid.as_str()));
                    if let Some(l) = &inp.name {
                        inp_start.push_attribute(("label", l.as_str()));
                    }
                    w.write_event(Event::Start(inp_start))?;

                    let eid = self.next_id("inputExpression");
                    let mut exp_start = BytesStart::new("dmn:inputExpression");
                    exp_start.push_attribute(("id", eid.as_str()));
                    exp_start.push_attribute((
                        "typeRef",
                        prefix_type(inp.type_ref.as_deref().unwrap_or("Any")).as_str(),
                    ));
                    w.write_event(Event::Start(exp_start))?;
                    write_text(w, "dmn:text", &inp.expression)?;
                    w.write_event(Event::End(BytesEnd::new("dmn:inputExpression")))?;
                    w.write_event(Event::End(BytesEnd::new("dmn:input")))?;
                }

                for out in outputs {
                    let oid = self.next_id("output");
                    let mut out_start = BytesStart::new("dmn:output");
                    out_start.push_attribute(("id", oid.as_str()));
                    out_start.push_attribute(("name", out.name.as_str()));
                    out_start.push_attribute(("typeRef", prefix_type(&out.type_ref).as_str()));
                    if let Some(def) = &out.default_output {
                        out_start.push_attribute(("defaultOutputEntry", def.as_str()));
                    }
                    w.write_event(Event::Empty(out_start))?;
                }

                for rule in rules {
                    let rid = self.next_id("rule");
                    let mut rule_start = BytesStart::new("dmn:rule");
                    rule_start.push_attribute(("id", rid.as_str()));
                    w.write_event(Event::Start(rule_start))?;

                    for entry in &rule.input_entries {
                        let eid = self.next_id("inputEntry");
                        let mut es = BytesStart::new("dmn:inputEntry");
                        es.push_attribute(("id", eid.as_str()));
                        w.write_event(Event::Start(es))?;
                        write_text(w, "dmn:text", entry)?;
                        w.write_event(Event::End(BytesEnd::new("dmn:inputEntry")))?;
                    }
                    for entry in &rule.output_entries {
                        let eid = self.next_id("outputEntry");
                        let mut es = BytesStart::new("dmn:outputEntry");
                        es.push_attribute(("id", eid.as_str()));
                        w.write_event(Event::Start(es))?;
                        write_text(w, "dmn:text", entry)?;
                        w.write_event(Event::End(BytesEnd::new("dmn:outputEntry")))?;
                    }
                    if let Some(ann) = &rule.annotation {
                        let aid = self.next_id("annotationEntry");
                        let mut es = BytesStart::new("dmn:annotationEntry");
                        es.push_attribute(("id", aid.as_str()));
                        w.write_event(Event::Start(es))?;
                        write_text(w, "dmn:text", ann)?;
                        w.write_event(Event::End(BytesEnd::new("dmn:annotationEntry")))?;
                    }
                    w.write_event(Event::End(BytesEnd::new("dmn:rule")))?;
                }
                w.write_event(Event::End(BytesEnd::new("dmn:decisionTable")))?;
            }

            BoxedExpression::Context {
                entries,
                return_entry,
            } => {
                w.write_event(Event::Start(BytesStart::new("dmn:context")))?;
                for entry in entries {
                    w.write_event(Event::Start(BytesStart::new("dmn:contextEntry")))?;
                    let mut var = BytesStart::new("dmn:variable");
                    var.push_attribute(("name", entry.key.as_str()));
                    w.write_event(Event::Empty(var))?;
                    self.write_boxed_expression(&entry.value, w)?;
                    w.write_event(Event::End(BytesEnd::new("dmn:contextEntry")))?;
                }
                if let Some(ret) = return_entry {
                    w.write_event(Event::Start(BytesStart::new("dmn:contextEntry")))?;
                    write_literal(w, ret, None)?;
                    w.write_event(Event::End(BytesEnd::new("dmn:contextEntry")))?;
                }
                w.write_event(Event::End(BytesEnd::new("dmn:context")))?;
            }

            BoxedExpression::FunctionDefinition { parameters, body } => {
                w.write_event(Event::Start(BytesStart::new("dmn:functionDefinition")))?;
                if !parameters.is_empty() {
                    for p in parameters {
                        let mut fp = BytesStart::new("dmn:formalParameter");
                        fp.push_attribute(("name", p.name.as_str()));
                        fp.push_attribute(("typeRef", prefix_type(&p.type_ref).as_str()));
                        if p.is_collection {
                            fp.push_attribute(("isCollection", "true"));
                        }
                        w.write_event(Event::Empty(fp))?;
                    }
                }
                self.write_boxed_expression(body, w)?;
                w.write_event(Event::End(BytesEnd::new("dmn:functionDefinition")))?;
            }

            BoxedExpression::Invocation { bkm, bindings } => {
                w.write_event(Event::Start(BytesStart::new("dmn:invocation")))?;
                write_literal(w, bkm, None)?;
                for b in bindings {
                    w.write_event(Event::Start(BytesStart::new("dmn:binding")))?;
                    let mut p = BytesStart::new("dmn:parameter");
                    p.push_attribute(("name", b.parameter.as_str()));
                    w.write_event(Event::Empty(p))?;
                    write_literal(w, &b.expression, None)?;
                    w.write_event(Event::End(BytesEnd::new("dmn:binding")))?;
                }
                w.write_event(Event::End(BytesEnd::new("dmn:invocation")))?;
            }

            BoxedExpression::LiteralExpression {
                expression,
                type_ref,
            } => {
                write_literal(w, expression, type_ref.as_deref())?;
            }

            BoxedExpression::List { items } => {
                w.write_event(Event::Start(BytesStart::new("dmn:list")))?;
                for item in items {
                    write_literal(w, item, None)?;
                }
                w.write_event(Event::End(BytesEnd::new("dmn:list")))?;
            }

            BoxedExpression::Relation { columns, rows } => {
                w.write_event(Event::Start(BytesStart::new("dmn:relation")))?;
                for col in columns {
                    let mut c = BytesStart::new("dmn:column");
                    c.push_attribute(("name", col.as_str()));
                    w.write_event(Event::Empty(c))?;
                }
                for row in rows {
                    let rid = self.next_id("row");
                    let mut r = BytesStart::new("dmn:row");
                    r.push_attribute(("id", rid.as_str()));
                    w.write_event(Event::Start(r))?;
                    for cell in row {
                        write_literal(w, cell, None)?;
                    }
                    w.write_event(Event::End(BytesEnd::new("dmn:row")))?;
                }
                w.write_event(Event::End(BytesEnd::new("dmn:relation")))?;
            }

            BoxedExpression::Conditional {
                condition,
                then,
                else_,
            } => {
                w.write_event(Event::Start(BytesStart::new("dmn:conditional")))?;
                w.write_event(Event::Start(BytesStart::new("dmn:if")))?;
                write_literal(w, condition, None)?;
                w.write_event(Event::End(BytesEnd::new("dmn:if")))?;
                w.write_event(Event::Start(BytesStart::new("dmn:then")))?;
                self.write_boxed_expression(then, w)?;
                w.write_event(Event::End(BytesEnd::new("dmn:then")))?;
                w.write_event(Event::Start(BytesStart::new("dmn:else")))?;
                self.write_boxed_expression(else_, w)?;
                w.write_event(Event::End(BytesEnd::new("dmn:else")))?;
                w.write_event(Event::End(BytesEnd::new("dmn:conditional")))?;
            }

            BoxedExpression::Filter { source, filter } => {
                w.write_event(Event::Start(BytesStart::new("dmn:filter")))?;
                w.write_event(Event::Start(BytesStart::new("dmn:in")))?;
                write_literal(w, source, None)?;
                w.write_event(Event::End(BytesEnd::new("dmn:in")))?;
                w.write_event(Event::Start(BytesStart::new("dmn:matchingLogic")))?;
                write_literal(w, filter, None)?;
                w.write_event(Event::End(BytesEnd::new("dmn:matchingLogic")))?;
                w.write_event(Event::End(BytesEnd::new("dmn:filter")))?;
            }

            BoxedExpression::For {
                variable,
                in_,
                return_,
            } => {
                w.write_event(Event::Start(BytesStart::new("dmn:for")))?;
                w.write_event(Event::Start(BytesStart::new("dmn:in")))?;
                let mut v = BytesStart::new("dmn:variable");
                v.push_attribute(("name", variable.as_str()));
                w.write_event(Event::Empty(v))?;
                write_literal(w, in_, None)?;
                w.write_event(Event::End(BytesEnd::new("dmn:in")))?;
                w.write_event(Event::Start(BytesStart::new("dmn:return")))?;
                write_literal(w, return_, None)?;
                w.write_event(Event::End(BytesEnd::new("dmn:return")))?;
                w.write_event(Event::End(BytesEnd::new("dmn:for")))?;
            }

            BoxedExpression::Quantified {
                kind,
                variable,
                in_,
                satisfies,
            } => {
                let tag = if kind == "some" {
                    "dmn:some"
                } else {
                    "dmn:every"
                };
                w.write_event(Event::Start(BytesStart::new(tag)))?;
                w.write_event(Event::Start(BytesStart::new("dmn:in")))?;
                let mut v = BytesStart::new("dmn:variable");
                v.push_attribute(("name", variable.as_str()));
                w.write_event(Event::Empty(v))?;
                write_literal(w, in_, None)?;
                w.write_event(Event::End(BytesEnd::new("dmn:in")))?;
                w.write_event(Event::Start(BytesStart::new("dmn:satisfies")))?;
                write_literal(w, satisfies, None)?;
                w.write_event(Event::End(BytesEnd::new("dmn:satisfies")))?;
                w.write_event(Event::End(BytesEnd::new(tag)))?;
            }
        }
        Ok(())
    }

    // ---------- 辅助 ----------

    /// 通过元素名查找预分配 ID（input/decision/bkm/ks/ds）
    fn resolve_element_id(&self, name: &str) -> String {
        self.id_map
            .get(&format!("itemDef:{}", name))
            .or_else(|| self.id_map.get(&format!("input:{}", name)))
            .or_else(|| self.id_map.get(&format!("decision:{}", name)))
            .or_else(|| self.id_map.get(&format!("bkm:{}", name)))
            .or_else(|| self.id_map.get(&format!("ks:{}", name)))
            .or_else(|| self.id_map.get(&format!("ds:{}", name)))
            .cloned()
            .unwrap_or_else(|| name.to_string())
    }

    /// 计算图中元素的中心坐标（用于自动连线）
    fn node_center(&self, diagram: &Diagram, element: &str) -> (f64, f64) {
        diagram
            .nodes
            .iter()
            .find(|n| n.element == element)
            .map(|n| {
                let w = n.width.unwrap_or(180.0);
                let h = n.height.unwrap_or(80.0);
                (n.x + w / 2.0, n.y + h / 2.0)
            })
            .unwrap_or((0.0, 0.0))
    }
}

// ---------- 自由辅助函数 ----------

/// 写入 `<tag>text</tag>`
fn write_text(w: &mut XmlWriter, tag: &str, text: &str) -> anyhow::Result<()> {
    w.write_event(Event::Start(BytesStart::new(tag)))?;
    w.write_event(Event::Text(BytesText::new(text)))?;
    w.write_event(Event::End(BytesEnd::new(tag)))?;
    Ok(())
}

/// 写入 `<dmn:literalExpression [typeRef]>text</dmn:literalExpression>`
fn write_literal(w: &mut XmlWriter, text: &str, type_ref: Option<&str>) -> anyhow::Result<()> {
    let mut start = BytesStart::new("dmn:literalExpression");
    if let Some(t) = type_ref {
        start.push_attribute(("typeRef", prefix_type(t).as_str()));
    }
    w.write_event(Event::Start(start))?;
    write_text(w, "dmn:text", text)?;
    w.write_event(Event::End(BytesEnd::new("dmn:literalExpression")))?;
    Ok(())
}

fn prefix_type(type_ref: &str) -> String {
    if type_ref.starts_with("feel:") || type_ref.starts_with("http") || type_ref.contains(':') {
        type_ref.to_string()
    } else if matches!(
        type_ref,
        "Any"
            | "any"
            | "number"
            | "string"
            | "boolean"
            | "date"
            | "time"
            | "dateTime"
            | "date and time"
            | "duration"
            | "yearMonthDuration"
            | "dayTimeDuration"
            | "years and months duration"
            | "days and time duration"
            | "context"
            | "list"
            | "function"
    ) {
        format!("feel:{type_ref}")
    } else {
        type_ref.to_string()
    }
}

fn escape_feel_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dmn::{CompiledDmnModel, DmnExecutor, parse_dmn};
    use crate::feel::FeelValue;
    use std::collections::HashMap;

    #[test]
    fn diagram_edges_reference_generated_requirement_ids() {
        let definitions = parse_definitions(
            r#"
name: diagram
inputs:
  - name: applicant
    typeRef: string
decisions:
  - name: approved
    requiredInputs: [applicant]
    logic:
      type: literalExpression
      expression: applicant = "yes"
diagram:
  nodes:
    - element: applicant
      x: 0
      y: 0
    - element: approved
      x: 240
      y: 0
  edges:
    - source: applicant
      target: approved
"#,
        )
        .unwrap();
        validate(&definitions).unwrap();

        let xml = generate_dmn_xml(&definitions).unwrap();
        assert!(xml.contains("<dmn:informationRequirement id=\"informationRequirement_"));
        assert!(xml.contains("<dmndi:DMNEdge dmnElementRef=\"informationRequirement_"));
        assert!(xml.contains("\n  <dmn:inputData"));
    }

    #[test]
    fn text_annotation_resolves_item_definition_ids() {
        let definitions = parse_definitions(
            r#"
name: annotations
itemDefinitions:
  - name: ApplicantType
    type:
      kind: simple
      typeRef: string
inputs: []
decisions: []
textAnnotations:
  - text: Applicant schema
    target: ApplicantType
"#,
        )
        .unwrap();
        validate(&definitions).unwrap();

        let xml = generate_dmn_xml(&definitions).unwrap();
        assert!(xml.contains("<dmn:sourceRef href=\"#itemDefinition_1\""));
    }

    #[test]
    fn generated_decision_service_uses_ids_and_executes_its_output() {
        let definitions = parse_definitions(
            r#"
name: service-model
imports:
  - name: SharedTypes
    namespace: https://example.com/dmn/shared
    location: shared-types.dmn
    importType: https://www.omg.org/spec/DMN/20231201/MODEL/
inputs:
  - name: amount
    typeRef: number
decisions:
  - name: BaseRate
    typeRef: number
    requiredInputs: [amount]
    logic:
      type: literalExpression
      expression: amount * 0.1
      typeRef: number
  - name: Premium
    typeRef: number
    requiredDecisions: [BaseRate]
    logic:
      type: literalExpression
      expression: BaseRate + 5
      typeRef: number
decisionServices:
  - name: PremiumService
    outputDecisions: [Premium]
    encapsulatedDecisions: [BaseRate]
    inputData: [amount]
"#,
        )
        .unwrap();
        validate(&definitions).unwrap();

        let xml = generate_dmn_xml(&definitions).unwrap();
        assert!(xml.contains("locationURI=\"shared-types.dmn\""));
        assert!(xml.contains("<dmn:outputDecision href=\"#decision_"));
        assert!(xml.contains("<dmn:inputData href=\"#inputData_"));

        let model = parse_dmn(&xml).unwrap();
        assert_eq!(model.imports[0].name.as_deref(), Some("SharedTypes"));
        assert_eq!(
            model.imports[0].location_uri.as_deref(),
            Some("shared-types.dmn")
        );
        let service = &model.decision_services[0];
        assert_eq!(service.output_decisions, ["decision_3"]);
        assert_eq!(service.encapsulated_decisions, ["decision_2"]);
        assert_eq!(service.input_data, ["inputData_1"]);

        let compiled = CompiledDmnModel::compile(&model).unwrap();
        let result = DmnExecutor::from_compiled(&compiled).evaluate_decision_service(
            "PremiumService",
            HashMap::from([("amount".into(), FeelValue::Number(100.0))]),
        );
        assert!(!result.has_errors(), "{:?}", result.messages);
        assert_eq!(result.decision_results.len(), 1);
        assert_eq!(result.decision_results[0].decision_name, "Premium");
        assert_eq!(result.decision_results[0].value, FeelValue::Number(15.0));
    }

    #[test]
    fn generated_list_type_preserves_element_type_constraints() {
        let definitions = parse_definitions(
            r#"
name: list-model
itemDefinitions:
  - name: Scores
    type:
      kind: list
      itemType:
        kind: simple
        typeRef: number
inputs:
  - name: scores
    typeRef: Scores
decisions:
  - name: ScoreCount
    typeRef: number
    requiredInputs: [scores]
    logic:
      type: literalExpression
      expression: count(scores)
      typeRef: number
"#,
        )
        .unwrap();
        validate(&definitions).unwrap();

        let xml = generate_dmn_xml(&definitions).unwrap();
        assert!(xml.contains("name=\"Scores\" isCollection=\"true\""));
        assert!(xml.contains("<dmn:typeRef>feel:number</dmn:typeRef>"));
        let model = parse_dmn(&xml).unwrap();
        assert!(model.item_definitions[0].is_collection);
        assert_eq!(
            model.item_definitions[0].type_ref.as_deref(),
            Some("feel:number")
        );
        let compiled = CompiledDmnModel::compile(&model).unwrap();

        let valid = DmnExecutor::from_compiled(&compiled).evaluate_by_name(
            "ScoreCount",
            HashMap::from([(
                "scores".into(),
                FeelValue::List(vec![FeelValue::Number(10.0), FeelValue::Number(20.0)]),
            )]),
        );
        assert!(!valid.has_errors(), "{:?}", valid.messages);
        assert_eq!(
            valid.decision_by_name("ScoreCount").unwrap().value,
            FeelValue::Number(2.0)
        );

        let invalid = DmnExecutor::from_compiled(&compiled).evaluate_by_name(
            "ScoreCount",
            HashMap::from([(
                "scores".into(),
                FeelValue::List(vec![FeelValue::String("wrong".into())]),
            )]),
        );
        assert!(invalid.has_errors());
        assert!(
            invalid
                .errors()
                .any(|message| message.code == "DMN-EVAL-TYPE")
        );
    }

    #[test]
    fn generated_bkm_invocation_preserves_parameters_and_knowledge_requirements() {
        let definitions = parse_definitions(
            r#"
name: bkm-model
inputs:
  - name: amount
    typeRef: number
bkms:
  - name: AddFee
    typeRef: number
    parameters:
      - name: value
        typeRef: number
    logic:
      type: literalExpression
      expression: value + 5
      typeRef: number
decisions:
  - name: Premium
    typeRef: number
    requiredInputs: [amount]
    requiredKnowledge: [AddFee]
    logic:
      type: invocation
      bkm: AddFee
      bindings:
        - parameter: value
          expression: amount
"#,
        )
        .unwrap();
        validate(&definitions).unwrap();

        let xml = generate_dmn_xml(&definitions).unwrap();
        assert!(xml.contains("<dmn:formalParameter name=\"value\" typeRef=\"feel:number\""));
        assert!(!xml.contains("<dmn:formalParameters>"));
        let model = parse_dmn(&xml).unwrap();
        let bkm = &model.business_knowledge_models[0];
        let crate::dmn::DecisionLogic::FunctionDefinition(function) = &bkm.logic else {
            panic!("expected BKM function definition");
        };
        assert_eq!(function.parameters, ["value"]);
        assert_eq!(function.parameter_type_refs, [Some("feel:number".into())]);
        assert_eq!(
            model.decisions[0].knowledge_requirements[0].required_knowledge,
            bkm.id
        );

        let compiled = CompiledDmnModel::compile(&model).unwrap();
        let result = DmnExecutor::from_compiled(&compiled).evaluate_by_name(
            "Premium",
            HashMap::from([("amount".into(), FeelValue::Number(10.0))]),
        );
        assert!(!result.has_errors(), "{:?}", result.messages);
        assert_eq!(
            result.decision_by_name("Premium").unwrap().value,
            FeelValue::Number(15.0)
        );
    }
}
