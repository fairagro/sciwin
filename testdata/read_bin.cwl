#!/usr/bin/env cwl-runner

cwlVersion: v1.2
class: CommandLineTool

requirements:
- class: InitialWorkDirRequirement
  listing:
  - entryname: data.bin
    entry: $(inputs.data)
  - entryname: read_bin.py
    entry:
      $include: read_bin.py

inputs: 
  data:
    type: File
    inputBinding:
      position: 1
    default: 
        class: File
        location: data.bin

outputs: 
  output:
    type: File
    outputBinding:
      glob: output.txt
baseCommand:
- python3
- read_bin.py
