//! Synthetic in-memory demonstration, not a persistent password-manager CLI.
//! Fixed example passwords/ID are deliberately public. No secret values are printed.

use codeward_core::{
    credentials::{Credential, CredentialInput, CredentialSnapshot},
    vault::{Enrollment, begin_recovery, unlock_with_master},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let password = "synthetic demonstration password";
    let mut enrollment = Enrollment::begin()?;
    // A future UI must ask the person to transcribe their paper copy. This
    // synthetic example supplies the generated copy directly to exercise the API.
    let paper = enrollment.recovery_code()?;
    let initial = enrollment.prepare(password, password, &paper)?;
    println!(
        "Prepared initial encrypted candidate: {} bytes.",
        initial.as_bytes().len()
    );

    let vault = unlock_with_master(initial.as_bytes(), password)?;
    let record = Credential::new(CredentialInput {
        id: "00000000000000000000000000000001",
        title: "Synthetic service",
        username: "synthetic user",
        password: "synthetic credential password",
        url: "https://example.invalid/",
        notes: "In-memory example only",
        created_at: 0,
        updated_at: 0,
    })?;
    let snapshot = CredentialSnapshot::new(vec![record])?;
    let content = vault.prepare_contents(vault.revision(), &snapshot)?;
    drop(snapshot);
    vault.lock();

    let reopened = unlock_with_master(content.as_bytes(), password)?;
    println!(
        "Reopened content candidate: revision {}, {} credential.",
        reopened.revision(),
        reopened.snapshot().credentials().len()
    );
    reopened.lock();

    let recovery = begin_recovery(content.as_bytes(), &paper)?;
    let replacement = "replacement synthetic demonstration password";
    let changed =
        recovery.prepare_master_password(recovery.revision(), replacement, replacement)?;
    drop(recovery);
    drop(paper);
    let reopened = unlock_with_master(changed.as_bytes(), replacement)?;
    println!(
        "Reopened recovery candidate: revision {}, {} credential.",
        reopened.revision(),
        reopened.snapshot().credentials().len()
    );
    reopened.lock();
    println!("All vault work stayed in memory; no vault file was saved.");
    Ok(())
}
