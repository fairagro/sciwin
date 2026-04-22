---
title: 'SciWin-Client: something subtitle'

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
  - name: Harald von Waldow
    orcid: 0000-0003-4800-2833
    affiliation: 1
  - name: Patrick König
    orcid: 0000-0002-8948-6793
    affiliation: 3
  - name: Xaver Stiensmeier
    orcid: 0009-0005-3274-122X
    affiliation: 4
  - name: Florian Hoedt
    orcid: 0000-0002-6068-1659
    affiliation: 5

affiliations:
  - name: Johann Heinrich von Thünen-Institute, Braunschweig, Germany
    ror: 00mr84n67 
    index: 1
  - name:  Leibniz Institute of Plant Genetics and Crop Plant Research, Gatersleben, Germany
    ror: 02skbsp27
    index: 2
  - name: Helmholtz Centre for Infection Research, Braunschweig, Germany
    ror: 03d0p2685 
    index: 3
  - name: Bielefeld University, Bielefeld, Germany
    ror: 02hpadn98 
    index: 4
  - name: PowerCo ??
    index: 5

date: XX XXXX 202X
bibliography: paper.bib
---

# Summary
SciWIn-Client (`s4n`) is a command-line tool developed as part of the Scientific Workflow Infrastructure (SciWIn) of the FAIRagro consortium [@Ewert2023Proposal]. It is designed to streamline the creation, execution and management of reproducible computational workflows using the _Common Workflow Language (CWL)_[@Crusoe2022MethodsIncluded]. By wrapping ordinary command-line commands with a thin layer of tooling, SciWIn-Client automatically generates CWL definitions, allowing scientists to write CWL using the well-known commands rather than hand-authoring verbose specifications.
Implemented in Rust for high performance and reliability, SciWIn-Client integrates natively with Git for version control and provenance tracking. It supports both local and remote workflow execution and is interoperable with the Workflow RO-Crate[@??] and Workflow Run RO-Crate[@Leo2024WRRC] standards. Furthermore SciWIn-Client is interoperable with research data management frameworks such as DataPLANT's ARC format [@dataplant2025ARCSpec;@Weil2023PLANTdataHUB].

# Statement of Need
Automated computational workflows are essential for managing complex, multi-step data analysis across various scientific disciplines. Significant effort has been invested into domain-specific languages that formalize and standardize computational scientific processes, thereby enhancing reproducibility, scalability and efficiency. 
To harmonize this wild growth of languages, the Common Workflow Language (CWL) was  introduced as universal standard [@Crusoe2022MethodsIncluded]. Its design emphasizes flexibility and machine readability but its verbose YAML-based syntax poses a substantial barrier to adoption among researchers unfamiliar with structured data formats. 

CWL therefore is predestined to be written by machines rather than humans, which ultimately motivated the conception of SciWIn-Client. 
SciWIn-Client provides an intuitive command-line interface that automates CWL generation and management. It translates typical research computing tasks into structured, version-controlled workflow definitions, effectively allowing scientists to “write CWL by doing science.”

# State of the field
The landscape of scientific workflow management is broad and fragmented. Numerous platforms and languages have emerged to address the need for reproducible, automated data analysis pipeline. Tools such as Nextflow[@di_tommaso_nextflow_2017] and Galaxy[@giardine_galaxy_2005] have achieved significant adoption within the scientific community. Both offer powerful execution environments and rich graphical or scripting environments. Both platforms put significant effort in providing a broad set of scripts especially for the OMICS-community (e.g. nf-core), however lacking in the agro-community where individual scripting plays a key part. 
Bringing individual scripts into the platform in both cases has a hurdle to overcome. For Nextflow researchers need to learn the Groovy-based DSL, for Galaxy a curation process needs to be passed to get tools onto the platform. Workflows authored for Galaxy are typically bound to a specific Galaxy instance, and portability across infrastructures can require substantial re-engineering effort.
CWL was introduced as a vendor-neutral, platform agnostic standard to address fragmentation. CWL workflows are portable by design as they in principle can run on any compliant execution engine. There are even efforts to make Galaxy and Nextflow compliant to this standard [@ref]. One big downside however is the lack of tooling especially in the creation process. CWLs adoption is comparable smaller than Nextflow and Galaxy. Its verbose, YAML-based syntax demands familiarity with structured data formats and workflow abstractions that many domain researchers lack. The result is a paradox: a universal standard that remains inaccessible to a large share of its intended users.
The CWL ecosystem further compounds this problem. While a number of great runner implementations exist (e.g. cwltool, Toil, REANA, ........), the space of authoring tools is sparse. Rabix offered a graphical editor (Rabix Composer) which was made closed-source and moved into the seven-bridges Platform. The open-sourced version has been unmaintained for over 5 years and is significantly outdated. Many generators are outdated as well meaning there is no actively developed open and lightweight CWL generator that integrates naturally into a researchers existing command-line-driven "workflow". (Planemo einbauen??!?)
SciWIn-Client addresses this gap removing the need for researchers to write CWL by hand. Second it works fully offline without dependencies to any platform and is Git-native.


- Fragmented landscape
- Platforms like Nextflow, Galaxy -> significant adoption, 
- CWL vendor-neutral, platform agnostic, portable, runs everywhere
- CWL lacking ecosystem/tooling- lots of runners but only outdated "Generators" like Rabix (now behind vendor lock)
- Alleinstellungsmerkmale SciWIn:
  - CWL, offline, niederschwelliger, unabhängiger, Anbindung an ARC-Ökosystem, unabhängiger was (insbesondere lokale) compute-Instanzen angeht. Vertrauliche Daten. Einfacheres Teilen von Skripten. Universeller durch Git-basierte Repräsentation (Benutzung beliebiger Forges).
  - Arbeitsprozess ist auf **eine** GX-Instanz beschränkt?  
  - Interop mit Galaxy (wrappen um execution engines zu benutzen),  
  - Provenance capture, & versioning via git native


# Software design
SciWIn-Client (short: `s4n`) is implemented in the Rust programming language, chosen for its high performance, strong type safety, and robust error handling — qualities essential in scientific software. Git integration provides built-in version control and interoperability with research data management frameworks such as  DataPLANTs ARC [@dataplant2025ARCSpec][@Weil2023PLANTdataHUB] format which can be viewed as a Git-based implementation of the RO-Crate standard [@SoilandReyes2022ROCrate].

## Managing CWL Files
A central concept of the tool is the automation of CWL generation. When users invoke a command or script using the `s4n create` prefix SciWIn-Client analyzes the command-line inputs and execution to identify `inputs`, `baseCommand` and `requirements` metadata and creates a CWL CommandLineTool. SciWIn-Client uses Git in background  a version-controlled environment for tracking changes and support this process. However most importantly Git serves information of changed files to create the  `outputs`-Section of the CWL CommandLineTool. While the system can automatically infer inputs and outputs, users also have the option to define them explicitly. Users can specify a container image pulled from Docker Hub or provide paths to local Dockerfiles to ensure consistent, reproducible execution environments across different systems.

Once individual CWL CommandLineTools have been created, the next step is to combine them into a CWL Workflow. This is achieved using the `s4n connect` command, which allows the user to specify a source (starting tool or node) and a target (a subsequent tool or node). By linking the output of one tool to the input of another, the user defines the workflow's execution sequence. 

In order to expand the possible sources for connecting complex workflows, there is the option to `install` existing workflows using SciWIn-Client which internally uses Git's submodule feature. 

## Workflow Execution
The simplest way to execute a workflow is to run it directly on the machine where the workflow is defined by using the `s4n execute local` command (or `cwltool` which however does not support Windows).
When performing high demanding calculations, workflows often need to be dispatched to large compute clusters. For the execution on compute clusters SciWIn-Client is able to communicate with the REST-API of Reana instances [@Simko2019Reana]. Reana is a reproducible research data analysis platform provided by CERN. FAIRagro operates their own Reana Installation in de.NBI Cloud. 
Structured execution results in form of RO-crates [@SoilandReyes2022ROCrate] more specifically Workflow Run RO-Crates [@Leo2024WRRC] using the Provenance Run Crate profile can be exported. 

# Research impact statement
- lowering technical barrier
- FAIRagro goals
- By automating CWL generation from everyday research computing tasks, it enables domain scientists — regardless of their software engineering background — to participate in open, collaborative, and reproducible science.
- transparent versioning, FAIR, ARC format, DataPLANT, WRRC
- Future Development: WorkflowHub? DockerGen
The source code is openly available at https://github.com/fairagro/sciwin under a permissive license, and the project welcomes community contributions.

# Acknowledgements 
We gratefully acknowledge the financial support of the German Research Foundation (DFG) – project number 501899475.

# AI usage disclosure
All paper content was written manually and reflects the careful thought and input of the authors. SciWIn is an open source project, and as such contributors are free to use any tools, AI or otherwise, to generate code contained in pull requests or commits. All commits and pull requests are reviewed by the core developers and often iterated on multiple times; therefore, all content in the repository represents the effort and judgment of the authors.

# References