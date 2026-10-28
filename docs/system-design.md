# System design

Status: logical design only. Boxes are responsibilities, not chosen libraries,
processes, programming languages, file layouts, or algorithms. There is no
KeePassXC format-compatibility or identical-internals commitment.

## System context

```mermaid
flowchart LR
    person["Personal user"]
    paper["Physical notebook: recovery secret"]
    subgraph device["User's Mac: initial runtime boundary"]
        app["Codeward: local application"]
        vault["Local encrypted vault file"]
        session["macOS session events and local storage"]
        app <-->|"Read / commit complete states"| vault
        session -->|"Lock / sleep events and storage outcomes"| app
    end
    person <-->|"Create, unlock, manage, generate, lock, recover"| app
    person <-->|"Write, confirm, later transcribe"| paper
    subgraph future["Deferred: no initial runtime connection"]
        online["Optional online recovery"]
        browser["Optional browser integration"]
        backup["Optional backups and file restore"]
    end
```

The paper secret reaches the app through explicit enrollment display/user entry.
The notebook is outside the Mac boundary. Initial runtime has no required service
or network communication. Isolated future boxes describe scope, not approved APIs.

## Logical responsibilities

| Responsibility | Contract |
| --- | --- |
| User interaction | Display lock/save state, collect intent, explain recovery limits |
| Session control | Gate locked access, enforce triggers, handle drafts explicitly |
| Credential operations | Manage records/generation, distinguish drafts from commits |
| Vault access/recovery | Validate input, authenticate, protect contents, manage access metadata |
| Persistence | Commit complete states safely, preserve prior state on failure, report outcomes |

These boundaries support later modular work without choosing Rust/C++, UI,
cryptography, or storage. Security construction/dependencies require ADR review.

## Vault lifecycle

```mermaid
stateDiagram-v2
    [*] --> Closed
    Closed --> Enrollment: Create vault
    Enrollment --> Creating: Master password and paper secret confirmed
    Enrollment --> Closed: Cancel
    Creating --> Locked: Initial commit succeeds
    Creating --> CreationError: Failure or interruption
    CreationError --> Closed: Report without opening partial file
    Closed --> Locked: Open existing vault
    Locked --> Clean: Valid master password and intact vault
    Locked --> Recovery: Forgotten master password
    Locked --> Locked: Invalid credentials leave file unchanged
    Recovery --> AccessChange: Valid paper secret and intact vault
    Recovery --> Locked: Invalid secret or cancel leaves file unchanged
    AccessChange --> RecoveryCommit: Set replacement master password
    RecoveryCommit --> Clean: Access metadata commit succeeds
    RecoveryCommit --> Locked: Failure or crash reopens coherent state
    Locked --> FileError: Missing, unsupported, damaged or tampered file
    Recovery --> FileError: Missing, unsupported, damaged or tampered file
    FileError --> Closed: Report without reconstructing data
    Clean --> Draft: Generate password or edit credential
    Draft --> Clean: Explicit discard
    Draft --> Saving: Explicit save
    Saving --> Clean: Acknowledge after commit succeeds
    Saving --> SaveError: Storage failure or uncertain result
    SaveError --> Saving: Retry after resolving outcome
    SaveError --> Clean: Discard draft after outcome is known
    Clean --> Locked: Lock event
    Draft --> Locked: Lock with draft policy
    SaveError --> Locked: Lock with draft policy
    Locked --> Closed: Close
    Clean --> Closed: Close
    Draft --> Closed: Resolve or discard draft on close
    Saving --> Closed: Crash retains complete old or new commit
    Clean --> Closed: Crash preserves committed state
    Draft --> Closed: Crash may lose draft
    Closed --> [*]
```

`Clean` and `Draft` are unlocked. `AccessChange` and `RecoveryCommit` are restricted
recovery states, not credential browsing. Recovery requires validation of the file;
invalid files never unlock. Announce recovery success only after new access commits.

The diagram summarizes outcomes, not an implementation. A crash in any transient
state leads next launch through closed/locked access. Creation may leave no ready
vault; updates must retain a complete prior or new commit. Normal saves and access
changes share the commit contract. After interrupted acknowledgement, inspect the
actual committed file on reopen.

Lock/close with drafts requires an explicit policy in ADR-0005; these transitions
do not authorize silent autosave or promise draft retention. If locking occurs
during a save/recovery commit, hide credentials while resolving the commit
coherently. Never postpone a security lock indefinitely for an edit or mislabel
an interrupted operation as saved.

## Persistence invariants

1. Save acknowledgement refers to a complete committed state.
2. Failed/interrupted updates cannot replace the last good state with partial data.
3. A crash after commit but before acknowledgement may leave the new complete state.
4. Credential contents and access/recovery metadata remain mutually consistent.
5. Drafts have no durability promise; displaying/copying does not commit them.
6. Conflicting writers must not silently overwrite each other.

Operations, durability, temporary files, writer exclusion, and power-loss
assumptions remain in [ADR-0003](adr/0003-committed-writes.md). Neither a recovery
secret nor a save protocol reconstructs the only missing vault file.
