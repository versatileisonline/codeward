# Offline paper recovery

Codeward's paper code restores access to an intact encrypted vault after a
forgotten master password. It is an independent access credential: anyone with
the matching code and vault can recover access. Store the paper separately from
the computer and its vault file.

The Rust core and headless macOS workflow implement the operations below.
The credential interface will present them during the desktop prototype work.
See [recovery evidence](milestone-3-progress.md) for tested scope and remaining gates.

## Enrollment

1. Start explicit creation at the managed destination. Rust generates a new code;
   creating directories or displaying the code does not create a ready vault.
2. Write down the entire code, including all groups, and transcribe it into the
   confirmation step. Rust validates the checksum and matches the generated secret.
3. Confirm the master password. Only a complete initial commit makes the vault ready.

The code is available only during pending enrollment. Successful preparation,
cancel or lock releases the core's enrollment display material. Already returned
display/input copies must also be cleared by the future UI. Restarting enrollment
generates a fresh code; a previous code cannot confirm the replacement enrollment.
The application must never retain plaintext for later redisplay.

## Recovering access

1. Open the intact managed vault and enter the paper code. Invalid or unmatched
   input cannot change its bytes. Accepted ASCII case, spaces and hyphens support
   transcription without changing the underlying secret.
2. Correct authentication enters restricted recovery. It does not expose credential
   browsing or finish the recovery operation.
3. Enter and confirm a replacement master password. Wait for the complete committed
   outcome before reporting success; visibility or a queued request is insufficient.
4. Unlock with the replacement password. The previous password fails on the updated
   vault; the existing paper code remains valid for another recovery.

Recovery preserves the credential snapshot and recovery slot. Older vault copies
retain their older password slots. Password replacement does not revoke those
copies or rotate an exposed data key.

## Messages for the desktop interface

These are the message contract for the later UI, not a claim that screens exist.
Messages contain no entered secrets, raw paths or native error text.

| Proven state | Message |
| --- | --- |
| Enrollment code displayed | Write down this recovery code and keep it separate from your computer. You will need your intact vault file as well as this code to recover access. |
| Malformed code or transcription checksum failure | Check every group of the recovery code and try again. |
| Valid enrollment transcription does not match | This copy does not match the recovery code shown for this setup. Check your paper copy. |
| Recovery authentication failed | The recovery code could not open this vault. Check the code and make sure you have the matching, intact vault file. |
| Restricted recovery authenticated | Recovery code accepted. Choose a new master password. |
| Replacement confirmation differs | The passwords do not match. Enter the same new master password in both fields. |
| Replacement pending | Saving your new master password… |
| Definite pre-publication replacement failure | Your new master password was not saved. Your previous vault state is unchanged. You can explicitly retry. |
| Outcome unknown | We could not confirm whether your new master password was saved. Keep your paper code and check the vault's saved state before trying again. |
| Reconciliation proves unchanged base | Your previous vault state is intact. The new master password was not saved. |
| Replacement commit confirmed | Your new master password is saved. Your existing recovery code still works. |
| No vault file at the managed destination | No vault file was found. The recovery code cannot recreate it. You need an intact copy of the vault to recover access. |
| Unsupported or structurally damaged vault | This vault could not be opened. The recovery code cannot repair or recreate damaged vault data. |

An uncertain save requires fresh authentication against actual known base/candidate
bytes and the required persistence barriers. A failed recheck retains uncertainty;
it cannot report success, repeat publication or unlock the session. An explicit
retry uses a fresh request only after the prior outcome permits it.

Missing files, malformed authority and leftover stages never authorize silent
creation, repair or stage promotion. This workflow supplies no file restoration,
backup service or recovery from losing both usable unlock routes.
