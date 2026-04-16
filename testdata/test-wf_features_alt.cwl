#!/usr/bin/env cwl-runner

cwlVersion: v1.2
class: Workflow

inputs:
  pop:
    type: File

outputs:
  out:
    type: File
    outputSource: s_default/out

steps:
  echo:
    in:
      test: pop
    run: echo.cwl
    out:
    - results
  s_default:
    in: 
      file1:
        source: echo/results
    run: default.cwl
    out: [out]
