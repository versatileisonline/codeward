# ADR-0001: Implementation platform

Status: Proposed — language recommendation only. Decision: none; owner choice pending.
Date: 2026-10-28. Target: Milestone 1. UI/platform/tooling questions remain open.

## Context and questions

Codeward is a new personal macOS password manager with no existing app code to
preserve. It handles credentials and untrusted vault input, runs offline, and must
eventually support intact-vault paper recovery and crash-safe committed saves.
Memory handling and maintainability therefore weigh more heavily here than an
unmeasured speed difference (NFR-02/03/05/06/07; threats T-07/09/10).

This comparison assumes no substantial existing C++ implementation or demonstrated
developer-experience advantage in either language. If that assumption is wrong,
revisit delivery cost. No code, benchmark, dependency audit, or UI feasibility
experiment was performed for this comparison.

## Comparison

The implications below are engineering judgments for Codeward, grounded in the
linked primary documentation. They are not measured project outcomes.

| Concern | Rust | C++ | Implication for Codeward |
| --- | --- | --- | --- |
| Memory safety | Ownership/borrowing checks reject many invalid memory accesses before execution. Unsafe code and foreign interfaces still need manual safety review. | RAII and smart pointers support reliable resource ownership, but do not make all references, indexing, and lifetimes safe. Review and analysis remain essential. | Favor Rust for reducing the memory-safety review burden around secrets and vault parsing. [Ownership](https://doc.rust-lang.org/book/ch04-01-what-is-ownership.html), [unsafe Rust](https://doc.rust-lang.org/book/ch20-01-unsafe-rust.html), [C++ resource guidelines](https://isocpp.github.io/CppCoreGuidelines/CppCoreGuidelines#S-resource). |
| Secret handling | Controlled ownership helps track where data lives; freeing a value does not by itself securely erase all copies. | RAII helps schedule cleanup; destruction likewise does not guarantee secure erasure. | Neither removes the need to design secret buffers, copies, diagnostics, and platform exposure. The [secure-memory documentation](https://doc.libsodium.org/memory_management) explains erasure and locking pitfalls; it is evidence, not a library selection. |
| Native macOS integration | macOS compilation targets exist; a native UI/core boundary can use foreign interfaces, with ownership, errors, linking, and secret-copy rules to validate. | Swift documents direct C++ interoperability, subject to supported types and lifetime rules. | C++ has an integration advantage if we choose a Swift-facing native shell. That UI architecture is still undecided. [Rust macOS targets](https://doc.rust-lang.org/rustc/platform-support/apple-darwin.html), [Rust FFI](https://doc.rust-lang.org/nomicon/ffi.html), [Swift/C++](https://www.swift.org/documentation/cxx-interop/). |
| Build and maintenance workflow | Cargo coordinates packages, dependencies, builds, and test conventions. Ownership concepts impose an initial learning cost. | The build/dependency workflow needs its own evaluation; familiarity with existing C++ tools may reduce cost for an experienced maintainer. | Favor Rust's cohesive core workflow under the stated experience assumption; neither language removes macOS UI/signing/packaging work. [Cargo](https://doc.rust-lang.org/cargo/guide/), [ownership learning curve](https://doc.rust-lang.org/book/ch04-01-what-is-ownership.html). |
| Testing and review | Compiler checks supplement tests; unsafe dependencies/boundaries, parsing, and business logic still need targeted validation. | Sanitizers can detect errors such as out-of-bounds access and use-after-free during execution. They supplement disciplined ownership and review. | Both need malformed-input, recovery, and interrupted-save tests. Runtime checks are evidence for exercised paths, not proof of all possible behavior. [Clang AddressSanitizer](https://clang.llvm.org/docs/AddressSanitizer.html), [unsafe Rust](https://doc.rust-lang.org/book/ch20-01-unsafe-rust.html). |
| Cryptography, recovery, durability | No automatic solution from choosing Rust. Dependencies and any foreign code must be evaluated. | No automatic solution from choosing C++. Dependencies and access protocols must be evaluated. | No language winner on correctness of these contracts alone. ADR-0002/0003/0004 still own the security construction, writes, and recovery design. |
| Performance and offline runtime | Candidate for the required local runtime; no Codeward performance data exists. | Candidate for the required local runtime; no Codeward performance data exists. | Do not claim a speed winner or pick by benchmark folklore. Neither requires an online account/service in our architecture; dependency/build downloads are separate from offline runtime. Establish representative workloads later. |

## Recommendation, not a decision

Prefer **Rust for the vault/core logic**. This is a recommendation to the owner,
not an accepted language choice or a complete application stack. A new password
manager benefits from moving more memory-lifetime checks into the compiler before
we accumulate parsing, session, and recovery logic. There is no legacy C++ core
in this repository whose reuse would offset that advantage.

The tradeoff is a learning cost and additional native-integration work to evaluate.
If a Swift-facing UI is later preferred, a narrow Rust/native boundary is a
candidate, not an approved design. UI secret copies, ownership, callbacks, and
error propagation would remain part of the security review. C++'s documented Swift
interoperability makes it a reasonable alternative if that integration cost or
substantial C++ expertise outweighs Rust's benefit.

Choosing C++ would require explicit resource/lifetime conventions and a suitable
analysis/sanitizer strategy. Choosing Rust would require minimizing and reviewing
unsafe/foreign-code boundaries and evaluating dependencies. Neither language makes
bad cryptography, misleading save acknowledgements, incomplete writes, plaintext
logging, or a compromised operating system safe.

## Still open before ADR acceptance

- Owner choice between Rust and C++; this recommendation does not make that choice.
- UI toolkit and app/core interface, including accessibility and secret-copy limits.
- Dependency/security-component assessment in coordination with ADR-0002/0004.
- Build/packaging tools, minimum macOS, hardware targets, and update/distribution scope.
- Representative vault size/performance targets and any necessary integration
  feasibility work, separately scoped before code is added.

## Acceptance gate

Owner selection of a language can be recorded separately while the remaining
platform questions stay open. Do not mark the whole ADR accepted until its relevant
alternatives, consequences, feasibility evidence, and owner review are recorded.
Update NFR-05/06/07 and threat-model platform limits when concrete choices are made.
This comparison adds no dependencies, build system, app scaffolding, or algorithm.
