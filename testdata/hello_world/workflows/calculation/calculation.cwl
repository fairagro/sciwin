#!/usr/bin/env cwl-runner

cwlVersion: v1.2
class: CommandLineTool

requirements:
- class: InitialWorkDirRequirement
  listing:
  - entryname: calculation.py
    entry:
      $include: calculation.py
- class: DockerRequirement
  dockerPull: pandas/pandas:pip-all

inputs:
- id: population
  type: File
  default:
    class: File
    location: '../../data/population.csv'
  inputBinding:
    prefix: '--population'
- id: speakers
  type: File
  default:
    class: File
    location: '../../data/speakers_revised.csv'
  inputBinding:
    prefix: '--speakers'

outputs:
- id: results
  type: File
  outputBinding:
    glob: results.csv

baseCommand:
- python
- calculation.py