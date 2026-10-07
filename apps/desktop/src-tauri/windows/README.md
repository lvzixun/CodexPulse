# Windows NSIS template

`installer.nsi` is based on the official Tauri CLI 2.12.1 template, under the accompanying MIT license. The source URL is in its header. The only executable change guards the built-in Run registry deletion with an exact comparison to this installation's quoted `codexpulse.exe --startup` command. A hook alone cannot enforce this: the upstream uninstall section performs its own unconditional deletion after hooks.

When upgrading Tauri CLI, compare this template with the matching upstream version and reapply that ownership guard; preserve upstream install, upgrade, shortcut and user-data behavior. No real Run or StartupApproved entries are changed by the automated startup tests.
