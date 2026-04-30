#!/usr/bin/env cwl-runner

cwlVersion: v1.2
class: Workflow

inputs:
- id: dirname
  type: string

outputs:
- id: out
  type: Directory
  outputSource: mkdir/out

steps:
- id: mkdir
  in:
    dirname: dirname
  run: mkdir.cwl
  out:
  - out