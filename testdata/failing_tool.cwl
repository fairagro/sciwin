#!/usr/bin/env cwl-runner

cwlVersion: v1.2
class: CommandLineTool

requirements:
- class: DockerRequirement
  dockerPull: pandas/pandas:pip-all

inputs: []
outputs: []

baseCommand:
- python
- -c
- raise RuntimeError('intentional failure for find_failures test')
