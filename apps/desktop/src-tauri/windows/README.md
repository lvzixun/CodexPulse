# Windows NSIS template

`installer.nsi` is based on the official Tauri CLI 2.12.1 template, under the accompanying MIT license. The source URL is in its header. Local changes:

- Guard the built-in Run registry deletion with an exact comparison to this installation's quoted `codexpulse.exe --startup` command. A hook alone cannot enforce this: the upstream uninstall section performs its own unconditional deletion after hooks.
- For current-user installs, restore an orphaned registered path only if it is writable; otherwise retain the normal `$LOCALAPPDATA\CodexPulse` default. An installation is orphaned only when both the application and uninstaller are confirmed missing. Access denied is not proof of absence. Explicit `/D` paths and existing installations are never silently redirected.
- Skip orphaned maintenance entries and tolerate a missing NSIS uninstaller when repairing an existing application. Do not delete registry records before successful installation replaces them.
- Probe directory creation and file writing before installation, including silent/passive updates, before an old uninstaller runs, and when leaving the directory page. Unwritable paths show a useful error (suppressed in silent mode) and fail with exit code 2. Probes use unique temporary files and never truncate application files. The check does not guarantee that a running/locked executable can be replaced.

When upgrading Tauri CLI, compare this template with the matching upstream version and reapply these changes; preserve upstream shortcut and user-data behavior. `scripts/test-windows-installer.ps1` compiles the marked production helper functions into a small NSIS harness and executes them against temporary directories, real deny ACLs, and an isolated HKCU test key. The release workflow runs it with the same NSIS toolchain used to build the installer. No real application registration, Run or StartupApproved entries are changed by these tests.
