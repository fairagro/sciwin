# Unreleased

# v1.2.0
## 🚀 Features
- `create` has option `--run-container` to execute the command for CommandLineTool creation with provided container
- `execute local --apptainer` or `execute local --singularity` to use apptainer/singularity for local execution

## 🐛 Bugfixes
Fixed bugs with `s4n init` and the error reporting
  
# v1.1.0
- `create` now has `--env` option to load in an .env file
- made inputs and outputs prefix optional in `connect`

## 🐛 Bugfixes
- default values are used on input connections
- Fixed some bugs in command parsing (`s4n create`, fixes for python modules)
- Better alignment with CWL spec in some points (e.g. glob)

## 🚜 Refactor
- Major restructuring for better maintainability

# v1.0.0
## 🚀 Features
- Restructured CLI Commands #158 #151
> [!IMPORTANT]
> The commands have changed in v1.0.0 (**Breaking Change**). The mapping is as follows:
> | Old Command               | New Command            |
>  |---------------------------|------------------------|
>  | s4n tool create           | s4n create             |
>  | s4n tool list             | s4n list               |
>  | s4n tool remove           | s4n remove             |
>  | s4n workflow create       | s4n create --name (optional!)|
>  | s4n workflow list         | s4n list               |
>  | s4n workflow remove       | s4n remove             |
>  | s4n workflow status       | s4n list [WORKFLOW_NAME]|
>  | s4n workflow connect      | s4n connect            |
>  | s4n workflow disconnect   | s4n disconnect         |
>  | s4n workflow visualize    | s4n visualize          |
>  | s4n workflow save         | s4n save               |


# v0.8.0
## 🚀 Features
- Support Workflows in git submodules #137
- Added `s4n install` and `s4n uninstall` commands #138
- Runner supports scatter and linkMerge now 
- Support packed CWL Documents in Runner #144
- Use ttl.sh to build Docker images for REANA on the fly (fixes #119)

## 🐛 Bugfixes
- ramping up runner conformance from 204/378 to 223/378
- fixed staging of arrays of files and dirs
- fixed staging of iwdr arrays

## 🚜 Refactor
- Code restructuring
- Consolidation in REANA specific (De)Serialization code #145

# v0.7.0
## 🚀 Features
- Remote Execution using Reana #108 
    - FAIRagro Reana Instance will be provided later, see #42
    - Credentials stored in keyring #118
    - Provenance RO-Crate can be created from remote execution 46808afdd219b68c69562fedce00e164b4fb0081
- Possibility to connect all CWL Documents in `s4n workflow connect` #95
- added `--mount` to `s4n tool create` to mount directories into working dir #103
- added type hints in `s4n tool create`. Inputs prefixed like s:/mnt/ are then enforced to be of type string.

## 🐛 Bugfixes
- Inputs and Outputs can not have same IDs #113

# v0.6.1
## 🚀 Features
- Added command `s4n workflow visualize` which accepts a path and either `mermaid` or `dot` as renderer outputting diagrams

# v0.6.0
## 🚀 Features
- Remote Execution `s4n execute remote` using REANA (testing) #101 + #108
- Added workflow.toml as config format #102
- added NetworkAccess Requirement via `--net/--enable-network` in `s4n tool create` #104
- Support InlineTools in Workflow step #111
- Support Expressions, Files and Directories as `InitialWorkDirRequirement` (in additon to Dirents)
- Support Conditional Execution

## 🐛 Bugfixes
- fixed a bug with Dockerfile path resolution
- handle NetworkAccess Requirment in runner
- inherit parents requirements correclty
- ramping up runner conformance from 160/378 to 204/378

# v0.5.2
## 🐛 Bugfixes
- fixed bugs with the `tool create -i` argument

# v0.5.1
## 🐛 Bugfixes
- fixed serialisation error

# v0.5.0
## 🚀 Features
- Support automatic downloading if files are given with `http://` or `https://` protocol
- Detect URLs as files if given with `http://` or `https://` protocol
- Support Detection of Arrays as inputs in `tool create` #100
- Support ExpressionLibs

## 🐛 Bugfixes
- correctly support `DockerRequirement.DockerOutputDirectory`#96
- made `CommandOutputBinding.glob` Optional #99
- ramping up runner conformance from 131/378 to 160/378

# v0.4.0
## 🚀 Features
- Added `--no-defaults` flag to tool create which can be handy when using passwords as inputs

## 🐛 Bugfixes
- fixed critical error in tool create where `outputEval` was set, even when null
- 🏃CWL Runner
    - Fixed some bugs in CWL Runner ramping up its conformance from 126/378 to 131/378
    - Improved Array support in Runner
    - Support cwl.output.json handling in Runner
    - Support globs in Runner
- Rewrite input ids if "bad words" are found (e.g. sql connection strings)

# v0.3.0
## 🚀 Features
- Added Containerization Support (Docker & Podman) for `s4n execute local`
- Support CWL ExpressionTools

## 🐛 Bugfixes
- Fixed some bugs in CWL Runner ramping up its conformance from 90/378 to 126/378

## 👀 Miscellaneous Tasks
- Removed Nightly Builds CI Workflow

# v0.2.0
## 🚀 Features
- Allowed handling of nullable and array CWLTypes using `File?` or `File[]` notation
- Added `s4n execute make-template ./path/to.cwl` to create job templates #75
- Added support for the direct execution of files #79
- Allow Directories as output

## 🐛 Bugfixes
- Fixed setting correct InitialWorkDirRequirement when `-i` is used in `s4n tool create` #69
- Fixed handling of json-Data #60
- Fixed unreported Bug, where CWL CommandLineTool Output was ignored if not of type File, Directory, stdout, stderr or string. 781d20e
- Fixed Command fail because of invalid git user config - prompts user if missing #78
- Fixed cleanup if init fails #77
- Fixed Files in subfolders can not be created in s4n tool create #88
- Fixed Do not check for uncommited changes if --no-run #89

## 🚜 Refactor
- Moved Runner into separate crate (Refactor)

## 👀 Miscellaneous Tasks
- Added Tests for all Documentation examples #76
- Added CWL Conformance Tests to CI Workflow
- Added more integration tests

# v0.1.0
Initial Release
