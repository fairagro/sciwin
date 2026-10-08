---
title: 'SciWIn-Client: Reproducible Computational Workflows from the Command-line'

tags:
  - CWL
  - workflow
  - FAIRagro

authors:
  - name: Jens Krumsieck
    orcid: 0000-0001-6242-5846
    affiliation: 1
  - name: Antonia Leidel
    orcid: 0009-0007-1765-0527
    affiliation: 2
  - name: Xaver Stiensmeier
    orcid: 0009-0005-3274-122X
    affiliation: 3
  - name: Patrick König
    orcid: 0000-0002-8948-6793
    affiliation: 2
  - name: Florian Hoedt
    orcid: 0000-0002-6068-1659
    affiliation: 1,4
  - name: Harald von Waldow
    orcid: 0000-0003-4800-2833
    affiliation: 1

affiliations:
  - name: Johann Heinrich von Thünen Institute, Braunschweig, Germany
    ror: 00mr84n67
    index: 1
  - name: Leibniz Institute of Plant Genetics and Crop Plant Research, Gatersleben, Germany
    ror: 02skbsp27
    index: 2
  - name: Bielefeld University, Bielefeld, Germany
    ror: 02hpadn98
    index: 3
  - name: PowerCo SE, Salzgitter, Germany (current affiliation)
    index: 4

date: XX XXXX 202X
bibliography: paper.bib
---

# Summary
SciWIn-Client is a command-line tool developed within the FAIRagro consortium [@Ewert.2023] as part of the Scientific Workflow Infrastructure (SciWIn). It is designed to streamline the creation, execution and management of reproducible computational workflows using the _Common Workflow Language (CWL)_[@Crusoe.2022]. By wrapping ordinary command-line commands with a thin layer of tooling, SciWIn-Client automatically generates CWL definitions, allowing scientists to write CWL using the well-known commands rather than hand-authoring verbose specifications.
Implemented in Rust for reliability, SciWIn-Client integrates natively with Git for version control and provenance tracking. It supports both local and remote workflow execution and is interoperable with the Workflow RO-Crate [@{Bacall.2026}] and Workflow Run RO-Crate [@Leo.2024] standards. Furthermore SciWIn-Client is interoperable with research data management frameworks such as DataPLANT's ARC format [@DataPLANT.2025;@Weil.2023].

# Statement of need
Automated computational workflows are essential for managing complex, multi-step data analysis across various scientific disciplines. Significant effort has been invested into domain-specific languages that formalize and standardize computational scientific processes, thereby enhancing reproducibility, scalability and efficiency. 
To harmonize those workflow languages, the Common Workflow Language (CWL) was introduced as universal standard [@Crusoe.2022]. Its design emphasizes flexibility and machine readability but its verbose YAML-based syntax poses a substantial barrier to adoption among researchers unfamiliar with structured data formats. 

CWL therefore is predestined to be written by machines rather than humans, which ultimately motivated the conception of SciWIn-Client. 
SciWIn-Client provides an intuitive command-line interface that automates CWL generation and management. It translates typical research computing tasks into structured, version-controlled workflow definitions, effectively allowing scientists to "write CWL by doing science."

# State of the field
The landscape of scientific workflow management is broad and fragmented. Numerous platforms and languages have emerged to address the need for reproducible, automated data analysis pipelines. Tools such as Nextflow [@DiTommaso.2017], Snakemake [@Molder.2021] and Galaxy [@Giardine.2005;@TheGalaxyCommunity.2024] have achieved significant adoption within the scientific community. All three tools offer powerful execution environments and rich graphical or scripting environments. All three platforms put significant effort in providing a broad set of scripts especially for the OMICS-community (e.g. nf-core [@Ewels.2020]), however lacking in the agro-community where individual scripting plays a key part. 

Bringing such individual scripts into established workflow platforms can present a substantial hurdle. With Nextflow, researchers need to learn the Groovy-based workflow DSL. In Galaxy, tools may need to undergo a curation and integration process before they become available on a public usegalaxy.* server. Moreover, workflows authored for Galaxy are often coupled to the capabilities and configuration of a particular Galaxy instance, and portability across infrastructures may require additional adaptation.

CWL was introduced as a vendor-neutral, platform agnostic standard to address fragmentation. CWL workflows are portable by design as they in principle can run on any compliant execution engine. There were efforts to make Galaxy and Nextflow compliant to this standard [@DiTommaso.2018]. However bridges to CWL have remained partial. The cwl2nxf converter for Nextflow was archived in 2025, and CWL support in Galaxy has been an open draft since 2021 [@Soranzo.2021]. Snakemake has `--export-cwl` which however just wraps a call to the Snakemake program in a CWL description using the Snakemake Docker container.
The adoption of CWL remains lower than that of Snakemake, Nextflow and Galaxy, and the standard provides comparatively little support for workflow creation. Its verbose, YAML-based syntax requires familiarity with structured data formats and workflow abstractions that many domain researchers do not possess. This creates a gap between the portability offered by a standardized workflow representation and the accessibility of authoring such workflows.

The CWL ecosystem further compounds this problem. A number of great runner implementations exist (e.g. cwltool, Toil [@Vivian.2017], REANA, Arvados)[@{CommonWorkflowLanguage.b}], which are mostly written in Python, but however do not support Windows natively. cwltool, the reference implementation, requires the Windows Subsystem for Linux (WSL) on Windows. The space of authoring tools is sparse [@{CommonWorkflowLanguage.}]. Rabix offered a graphical editor (Rabix Composer)[@Kaushik.2016] but further development was made closed-source and moved into the Seven Bridges Platform [@RabixComposercontributors.2021]. The open-sourced version has been unmaintained for over 5 years and is significantly outdated. Many generators are outdated as well, meaning there is no actively developed open and lightweight CWL generator that integrates naturally into a researcher's existing command-line-driven "workflow". `argparse2tool`[@Rasche.2024] (last commit 2024) only covers Python argparse programs, `scriptcwl` [@Van_der_zwaan.2019] (last commit 2020) and `Janis` [@PMCCBioinformaticsCore.2023] (last commit 2023) require writing Python scripts and `ToolJig` [@Piccolo.2021] uses web forms. `zatsu-cwl-generator` [@Tanjo.2022] (last commit 2022) is the closest to our approach, however it parses a command without running it, so it cannot observe files the command creates.

SciWIn-Client addresses this authoring gap by allowing researchers to create CWL workflows from their existing command-line-oriented workflows without requiring them to author the CWL document manually. It has no dependency on a specific workflow platform, integrates with Git-based workflows, and is distributed as a single self-contained binary.

A related but distinct line of work concerns tools that capture computational environments or provenance by observing program execution. Examples include ReproZip [@Chirigati.2016], noWorkflow [@Pimentel.2017], and Sciunit [@TonThat.2017]. These systems demonstrate that execution traces and runtime information can be used to support reproducibility and provenance capture. They are therefore conceptually close to the execution-based workflow creation approach implemented by SciWIn-Client. However, their primary objective is to capture, package, or reproduce information about an execution, rather than to transform an existing command-line execution into an explicit, reusable workflow specification. SciWIn-Client builds on the information available from execution while using it to generate a machine-actionable workflow description, including the workflow specification and its associated inputs and execution requirements.

# Software design
SciWIn-Client (short: `s4n`) was built around one simple design decision. A CWL `CommandLineTool` document is created from observing a command's actual execution, rather than by parsing the command line or asking the user to fill in a form. A command line alone provides no information about which files are written, so a purely static approach (such as `zatsu-cwl-generator`) cannot produce the `outputs` of a tool. Everything else follows from this decision and the goal of seamlessly integrating into the researcher's existing mode of operation: using a terminal, scripts and Git. Git integration also provides built-in version control and interoperability with research data management frameworks such as DataPLANT's ARC [@DataPLANT.2025;@Weil.2023] format which can be viewed as a Git-based implementation of the RO-Crate standard[@SoilandReyes.2022]. 

![Overview of SciWIn-Client design. The `s4n` binary is a thin front end over the shared `sciwin` library, which builds on the `commonwl`, `reana` and `rocrate` libraries. Tools are authored by observing a command inside a Git repository, are connected into workflows, and are executed on a local, Docker, TES or REANA backend. Each run can be exported as a Workflow Run RO-Crate.\label{fig:overview}](assets/overview.png)

## Architecture
`s4n` is a thin command-line layer around the `sciwin` library, so that multiple frontends can be supported. 
`sciwin` is organized into authoring (tool and workflow generation), execution, provenance, container resolution and repository handling. It combines three further libraries that were developed alongside it because no promising equivalent existed in the Rust ecosystem: `commonwl` (CWL parsing and an execution engine), `reana` (a client for the REANA [@Simko.2019] API) and `rocrate` (reading and building RO-Crates). They are published on `crates.io` separately, so they can be reused independently of SciWIn.

Rust was chosen for practical reasons beyond type safety. It produces a single self-contained binary without a language runtime, which lowers the installation and configuration barrier for researchers, and it runs natively on Windows, Linux and macOS. This fills a gap the reference CWL runner has, as it requires WSL on Windows. The price is a smaller pool of potential scientific contributors compared to Python, and the need to implement a CWL engine instead of reusing an existing one.

An overview of the architecture is shown in \autoref{fig:overview}.

## Authoring tools
When users invoke a command or script using the `s4n create` prefix SciWIn-Client automatically generates a tool. 

Example:
```bash
s4n create -c Dockerfile --container-tag pyplot --enable-network \
python3 code/plot_election.py --data data.csv --features features.json
```
would produce a `CommandLineTool` with two prefixed inputs as well as Docker- and Network-Requirements:
```yaml
#!/usr/bin/env cwl-runner

cwlVersion: v1.2
class: CommandLineTool

requirements:
- class: InitialWorkDirRequirement
  listing:
  - entryname: code/plot_election.py
    entry:
      $include: ../../code/plot_election.py
- class: DockerRequirement
  dockerFile:
    $include: ../../Dockerfile
  dockerImageId: pyplot
- class: NetworkAccess
  networkAccess: true

inputs:
- id: data
  type: File
  default:
    class: File
    location: ../../data.csv
  inputBinding:
    prefix: --data
- id: features
  type: File
  default:
    class: File
    location: ../../features.json
  inputBinding:
    prefix: --features

outputs:
- id: election
  type: File
  outputBinding:
    glob: election.png

baseCommand:
- python
- code/plot_election.py
```

The tool generation procedure follows a simple 4 step process:

1. The command-line is used to parse `inputs` and `baseCommand` to construct a preliminary `CommandLineTool`.
2. The preliminary `CommandLineTool` is executed locally ("probe"). Git is used in background to determine changes in the file system. Changed files and directories are added as `outputs`. 
3. Requirement metadata such as containers, network access, environment variables are added to the preliminary `CommandLineTool` by `s4n create`'s command options. 
4. The preliminary `CommandLineTool` is finalized by post processing steps such as EDAM format annotation, path relativization and array input detection.

As Git is used to determine changes, a Git repository is a hard requirement. However we accept this cost because version control is already best practice for the intended users and gives versioning of the generated files for free. A `.gitignore` can be used to ignore temporary directories or files may be created by the command.

While the system can automatically infer `inputs` and `outputs`, it also has inherent limits. Therefore users also have the option to define `inputs` and `outputs` explicitly.

### Containers
A tool recorded from a local run depends on the researcher's environment. To make the resulting CWL portable, a container can be attached either as an image reference or as a path to a local Dockerfile that is built on execution. For Python and R scripts, SciWIn-Client is able to resolve a container automatically from the project's dependency files (`requirements.txt`, `pyproject.toml`, `DESCRIPTION`) using a curated SBOM-based registry maintained by the project.

## Connecting tools
`s4n connect` links the output of one tool to the input of another by name or adds new workflow-wide `inputs` and `outputs`. The workflow is therefore built incrementally from tools that already exist, and the generated CWL Workflow stays a plain, editable document. 

## Workflow Execution
SciWIn-Client supports workflow execution on multiple backends (see \autoref{{tbl:backends}}) through the `s4n execute` command. The desired backend can be selected using the `--engine` flag. `s4n execute` accepts a CWL document or a Workflow RO-Crate [@{Bacall.2026}] or Workflow Run RO-Crate [@Leo.2024] for every backend.

: Overview of available backends.\label{tbl:backends}
| Backend | Purpose |
| --- | --- |
| `local` | Runs on the researcher's machine. Steps with a `DockerRequirement` run in Docker, Singularity or Podman. |
| `docker` | Runs every step in a container through the local Docker daemon. |
| `tes` | Submits tasks to a GA4GH Task Execution Service (TES) 1.1 server [@Kanitz.2024]. |
| `reana` | Submits the workflow to a REANA instance. (Not part of `commonwl`) |

The conformance with the CWL conformance test suite of `commonwl`'s backends is tracked using continuous integration and is 99% (97% required) across all current backends. 

## Trade-offs and limitations
- A Git repository is required, and observation only captures effects visible in the working tree.
- Authored tools are only as portable as their container definition. Without a container, a tool depends on the local environment.
- The tool targets command-line driven workflows. Interactive or graphical programs cannot be recorded.

# Research impact statement
SciWIn-Client is in use in FAIRagro, where it was used to build the workflow for Use Case 6 (high-throughput crop growth simulation) [@Gitahi.2026;@Gitahi.2026b]. The resulting workflow was published on WorkflowHub [@Gustafsson.2025;@Gitahi.2026b] and annotated using DataPLANT tooling, which shows that SciWIn-created workflows can be moved into ARC-based research data management [@Brilhaus.2026]. Workflows can be executed on the FAIRagro REANA instance operated in the de.NBI Cloud.

The software has been taught and presented to to its target community of agricultural and life science researchers. SciWIn-Client was presented at the 2nd Conference on Research Data Infrastructure (CoRDI 2025) [@Krumsieck.2025;@Krumsieck.2025b]. A hands-on workshop on SciWIn-Client was held at the Boosting Biodata Bootcamp 2026 [@Leidel.2026]. It has also been demonstrated at several further events, including the NFDI4LS Conference and the FAIRagro Community Summit [@Krumsieck.2025c;@Krumsieck.2025d;@Krumsieck.2026b;@Krumsieck.2026].

The release binaries have been downloaded more than 1700 times as of October 2026 ([Collected download data](https://github.com/fairagro/m4.4_metrics)). The supporting libraries `commonwl`, `reana` and `rocrate` are published separately on `crates.io`.

The source code is openly available at https://github.com/fairagro/sciwin under the MIT or Apache-2.0 license, and the project welcomes community contributions.

# CRediT authorship contribution statement

**Jens Krumsieck**: Conceptualization, Methodology, Software, Validation, Writing - Original Draft, Writing - Review & Editing, Visualization, 
**Antonia Leidel**: Conceptualization, Methodology, Software, Validation, Writing - Original Draft, Writing - Review & Editing, Visualization, 
**Xaver Stiensmeier**: Conceptualization, Methodology, Validation, Writing - Original Draft, Writing - Review & Editing, 
**Patrick König**: Conceptualization, Methodology, Project administration, 
**Florian Hoedt**: Conceptualization, Funding acquisition, 
**Harald von Waldow**: Conceptualization, Methodology, Writing - Original Draft, Writing - Review & Editing, Supervision, Project administration

# Acknowledgements 
We gratefully acknowledge the financial support of the German Research Foundation (DFG) – project number 501899475.

# AI usage disclosure
All paper content was written manually and reflects the careful thought and input of the authors. 
SciWIn is an open source project, and as such contributors are free to use any tools, AI or otherwise, to generate code contained in pull requests or commits. Claude Code was used as assistant to help with implementation of individual parts. It was mainly used for code generation, refatoring and documentation. All commits and pull requests are reviewed by the core developers and often iterated on multiple times; therefore, all content in the repository represents the effort and judgment of the authors.

# References
