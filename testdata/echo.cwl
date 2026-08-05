#!/usr/bin/env cwl-runner

cwlVersion: v1.2
class: CommandLineTool

requirements:
- class: InitialWorkDirRequirement
  listing:
  - entryname: echo.py
    entry:
      $include: echo.py

inputs:
- id: test
  type: File
  default:
    class: File
    location: input.txt
  inputBinding:
    prefix: '--test'

outputs:
- id: results
  type: File
  outputBinding:
    glob: results.txt

baseCommand:
- python3
- echo.py