# ADR-0001: Implementation platform

Status: Placeholder — undecided. Decision: none. Target: Milestone 1.

## Context and questions

Rust and C++ remain candidates; neither is selected. Decide macOS UI toolkit,
application/core boundaries, packaging/build tools, minimum macOS and hardware
support. Do not infer choices from diagrams or folder names.

## Evaluation needed

Compare memory-safety risks, established security dependencies, macOS integration,
accessibility, maintainability, fault testing and developer workflow. Establish
representative vault sizes/performance targets before asserting performance.
Record tradeoffs and primary sources during the decision milestone.

## Acceptance gate

Document alternatives/consequences, demonstrate feasibility where needed, and
obtain owner review. Update NFR-05/06/07 and threat-model platform limits. No
dependencies, build system, or application scaffolding in Milestone 0.
