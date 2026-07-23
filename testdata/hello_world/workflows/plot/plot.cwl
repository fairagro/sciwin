#!/usr/bin/env cwl-runner

cwlVersion: v1.2
class: CommandLineTool

requirements:
- class: InitialWorkDirRequirement
  listing:
  - entryname: plot.py
    entry:
      $include: plot.py
- class: DockerRequirement
  dockerPull: sciwin/python-datascience
inputs:
- id: results
  type: File
  default:
    class: File
    location: '../../results.csv'
  inputBinding:
    prefix: '--results'

outputs:
- id: o_results
  type: File
  outputBinding:
    glob: results.svg

baseCommand:
- python
- plot.py