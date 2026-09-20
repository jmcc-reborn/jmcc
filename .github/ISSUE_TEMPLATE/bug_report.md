---
name: Bug Report
about: Create a report to help us fix a bug or compiler crash
title: "[BUG] "
labels: ["bug"]
assignees: ""
---

## Description
A clear and concise description of what the bug is.

## Reproduction
Code snippet or `.jc` fixture that triggers the issue:
```jc
// Paste minimal reproducing JustCode here
```

Command executed:
```bash
jmcc compile <file.jc>
```

## Expected Behavior
A clear and concise description of what you expected to happen.

## Actual Behavior / Error Output
If this is a compiler crash (ICE / `E0001`), please include the generated crash report (`jmcc-crash-*.log`) or terminal output:
```text
// Paste compiler error or crash report here
```

## Environment
- **OS**: [e.g. Linux, Windows, macOS]
- **JMCC version**: [e.g. `jmcc --version` or git commit]
- **Target / Edition**: [e.g. edition 2026, target justmc]
