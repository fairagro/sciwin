use commonwl::{CWLDocument, StringOrDocument, Workflow, WorkflowStep};
use std::{error::Error, fs, path::Path};

pub fn render<R: FlowchartRenderer>(r: &mut R, cwl: &Workflow, filename: &Path, no_defaults: bool) -> Result<String, Box<dyn Error>> {
    r.initialize();

    r.begin_graph();

    r.begin_cluster("inputs", Some("Workflow Inputs"), RenderStyle::Input);
    for input in &cwl.inputs {
        r.node(&input.id, None, RenderStyle::Input);
    }
    r.end_cluster();

    r.begin_cluster("outputs", Some("Workflow Outputs"), RenderStyle::Output);
    for output in &cwl.outputs {
        r.node(&output.id, None, RenderStyle::Output);
    }
    r.end_cluster();

    for step in &cwl.steps {
        r.node(&step.id, None, RenderStyle::Default);

        for input in &step.in_ {
            if let Some(src) = &input.source {
                let src_id = src.split('/').next().unwrap();
                r.edge(src_id, &step.id, Some(&input.id), RenderStyle::Default);
            }
        }

        if !no_defaults && let Some(doc) = load_step(step, filename) {
            for input in &doc.inputs {
                if !step.in_.iter().any(|i| i.id == input.id)
                    && let Some(input_default) = &input.default
                {
                    let node_id = format!("{}_{}", step.id, input.id);
                    r.node(&node_id, Some(&input_default.as_value_string()), RenderStyle::Small);
                    r.edge(&node_id, &step.id, Some(&input.id), RenderStyle::Small);
                }
            }
        }
    }
    for output in &cwl.outputs {
        if let Some(output_source) = &output.output_source {
            let src = output_source.split('/').next().unwrap();
            r.edge(src, &output.id, Some(&output.id), RenderStyle::Default);
        }
    }

    r.end_graph();

    Ok(r.render())
}

fn load_step(step: &WorkflowStep, filename: &Path) -> Option<CWLDocument> {
    match &step.run {
        StringOrDocument::String(f) => {
            let step_path = filename.parent().unwrap_or(Path::new("")).join(f);
            if step_path.exists() {
                Some(serde_yaml::from_str(&fs::read_to_string(step_path).ok()?).ok()?)
            } else {
                None
            }
        }
        StringOrDocument::Document(doc) => Some((**doc).clone()),
    }
}

pub trait FlowchartRenderer {
    fn initialize(&mut self);

    fn begin_graph(&mut self);
    fn end_graph(&mut self);

    fn begin_cluster(&mut self, id: &str, label: Option<&str>, style: RenderStyle);
    fn end_cluster(&mut self);

    fn node(&mut self, id: &str, label: Option<&str>, style: RenderStyle);
    fn edge(&mut self, from: &str, to: &str, label: Option<&str>, style: RenderStyle);

    fn render(&self) -> String;
}

static BROWN_LIGHT: &str = "#F8CBAD";
static BROWN_DARK: &str = "#823909";
static GRAY_LIGHT: &str = "#EEEEEE";
static GRAY_DARK: &str = "#818281";
static GREEN_LIGHT: &str = "#C5E0B4";
static GREEN_DARK: &str = "#385723";
static BLUE_LIGHT: &str = "#6FC1B5";
static BLUE_DARK: &str = "#0f9884";
static BLUE_LIGHTER: &str = "#9FD6CE";
static BLUE_LIGHTEST: &str = "#cfeae6";

pub enum RenderStyle {
    Default,
    Input,
    Output,
    Small,
}

#[derive(Default)]
pub struct MermaidRenderer {
    storage: Vec<String>,
    styles: Vec<String>,
}

impl FlowchartRenderer for MermaidRenderer {
    fn initialize(&mut self) {
        self.storage = vec![];
        self.styles = vec![];

        self.storage.push(format!(
            r#"---
config:
  theme: base
  look: neo
  themeVariables:
    primaryColor: '{GREEN_LIGHT}'
    primaryTextColor: '#231f20'
    secondaryColor: '{GRAY_LIGHT}'
    lineColor: '{GREEN_DARK}'    
    fontSize: 12px
    tertiaryTextColor: '#231f20'
    fontFamily: 'Fira Sans, trebuchet ms, verdana, arial'
---"#
        ));
    }

    fn begin_graph(&mut self) {
        self.storage.push("flowchart TB".to_string());
        self.storage.push(format!("  linkStyle default stroke:{GREEN_DARK},stroke-width: 2px;"));
    }

    fn end_graph(&mut self) {}

    fn begin_cluster(&mut self, id: &str, label: Option<&str>, style: RenderStyle) {
        self.storage.push(format!("  subgraph {id}[{}]", label.unwrap_or(id)));
        self.storage.push("    direction TB".to_string());

        match style {
            RenderStyle::Input | RenderStyle::Output => self.styles.push(format!("  style {id} fill:{GRAY_LIGHT},stroke-width:2px;")),
            _ => unimplemented!(),
        }
    }

    fn end_cluster(&mut self) {
        self.storage.push("  end".to_string());
    }

    fn node(&mut self, id: &str, label: Option<&str>, style: RenderStyle) {
        self.storage.push(format!("    {id}({})", label.unwrap_or(id)));
        self.styles.push(match style {
            RenderStyle::Default => format!("  style {id} stroke:{GREEN_DARK},stroke-width:2px;"),
            RenderStyle::Input => format!("  style {id} stroke:{BLUE_DARK},fill:{BLUE_LIGHT},stroke-width:2px;"),
            RenderStyle::Output => format!("  style {id} stroke:{BROWN_DARK},fill:{BROWN_LIGHT},stroke-width:2px;"),
            RenderStyle::Small => format!("  style {id} font-size:9px,fill:{BLUE_LIGHTEST}, stroke:{BLUE_LIGHTER},stroke-width:2px;"),
        });
    }

    fn edge(&mut self, from: &str, to: &str, label: Option<&str>, _style: RenderStyle) {
        self.storage.push(if let Some(l) = label {
            format!("  {from} --> |{l}|{to}")
        } else {
            format!("  {from} --> {to}")
        });
    }

    fn render(&self) -> String {
        let content = self.storage.join("\n");
        let styles = self.styles.join("\n");
        format!("{content}\n{styles}")
    }
}

#[derive(Default)]
pub struct DotRenderer {
    storage: Vec<String>,
}

impl FlowchartRenderer for DotRenderer {
    fn initialize(&mut self) {
        self.storage = vec![];
    }

    fn begin_graph(&mut self) {
        self.storage.extend(vec![
            "digraph workflow {".to_string(),
            "  rankdir=TB;".to_string(),
            "  bgcolor=\"transparent\";".to_string(),
            "  node [fontname=\"Fira Sans\", style=filled, shape=record, penwidth=2];".to_string(),
            format!("  edge [fontname=\"Fira Sans\", fontsize=\"9\",fontcolor=\"{GRAY_DARK}\",penwidth=2, color=\"{GREEN_DARK}\"]"),
        ]);
    }

    fn end_graph(&mut self) {
        self.storage.push("}".to_string());
    }

    fn begin_cluster(&mut self, id: &str, label: Option<&str>, style: RenderStyle) {
        self.storage.push(format!("  subgraph cluster_{id} {{"));
        self.storage.push(format!("    label=\"{}\";", label.unwrap_or(id)));
        self.storage.push("    fontname=\"Fira Sans\";".to_string());
        if matches!(style, RenderStyle::Output) {
            self.storage.push("    labelloc=b;".to_string());
        } else {
            self.storage.push("    labelloc=t;".to_string());
        }
        self.storage.push("    penwidth=2;".to_string());
        self.storage.push("    style=\"filled\";".to_string());
        if matches!(style, RenderStyle::Input) || matches!(style, RenderStyle::Output) {
            self.storage.push(format!("    color=\"{GRAY_DARK}\";"));
            self.storage.push(format!("    fillcolor=\"{GRAY_LIGHT}\";"));
        }
    }

    fn end_cluster(&mut self) {
        self.storage.push("  }".to_string());
    }

    fn node(&mut self, id: &str, label: Option<&str>, style: RenderStyle) {
        let fillcolor = match style {
            RenderStyle::Default => GREEN_LIGHT,
            RenderStyle::Input => BLUE_LIGHT,
            RenderStyle::Output => BROWN_LIGHT,
            RenderStyle::Small => BLUE_LIGHTEST,
        };

        let color = match style {
            RenderStyle::Default => GREEN_DARK,
            RenderStyle::Input => BLUE_DARK,
            RenderStyle::Output => BROWN_DARK,
            RenderStyle::Small => BLUE_LIGHTER,
        };

        self.storage.push(if matches!(style, RenderStyle::Small) {
            format!(
                "    {id} [label=\"{}\", height=0.25, fontsize=10, fillcolor=\"{fillcolor}\", color=\"{color}\"];",
                label.unwrap_or(id)
            )
        } else {
            format!(
                "    {id} [label=\"{}\", fillcolor=\"{fillcolor}\", color=\"{color}\"];",
                label.unwrap_or(id)
            )
        });
    }

    fn edge(&mut self, from: &str, to: &str, label: Option<&str>, style: RenderStyle) {
        let styling = if matches!(style, RenderStyle::Small) { ", style=dashed" } else { "" };
        self.storage.push(if let Some(l) = label {
            format!("    {from} -> {to}[label=\"{l}\"{styling}];")
        } else if matches!(style, RenderStyle::Small) {
            format!("    {from} -> {to}[{styling}];")
        } else {
            format!("    {from} -> {to};")
        });
    }

    fn render(&self) -> String {
        self.storage.join("\n")
    }
}
