# Execute the production NSIS helpers, without touching real application keys.
param([string]$MakeNsis = "$env:LOCALAPPDATA\tauri\NSIS\makensis.exe")
$ErrorActionPreference = 'Stop'
if (!$IsWindows) { throw 'These tests require Windows and real Windows ACLs' }

$root = Join-Path ([IO.Path]::GetTempPath()) ("CodexPulseInstallerTests-" + [guid]::NewGuid().ToString('N'))
$key = 'Software\CodexPulseInstallerTests\' + [guid]::NewGuid().ToString('N')
$registryPath = 'HKCU:\' + $key
$deniedPaths = [Collections.Generic.List[object]]::new()
$sid = [Security.Principal.WindowsIdentity]::GetCurrent().User
$passed = 0
New-Item -ItemType Directory -Path $root | Out-Null

function Assert-Equal($Actual, $Expected, [string]$Message) {
  if ($Actual -cne $Expected) { throw "$Message`: expected [$Expected], got [$Actual]" }
}

function Deny-Access([string]$Path, [Security.AccessControl.FileSystemRights]$Rights) {
  $rule = [Security.AccessControl.FileSystemAccessRule]::new(
    $sid, $Rights, 'ContainerInherit,ObjectInherit', 'None', 'Deny')
  $acl = Get-Acl -LiteralPath $Path
  $acl.AddAccessRule($rule)
  Set-Acl -LiteralPath $Path -AclObject $acl
  $deniedPaths.Add(@{ Path = $Path; Rule = $rule })
}

function Invoke-Case(
  [string]$Name, [string]$Previous, [string]$Selected, [string]$Expected,
  [int]$Missing, [int]$Uninstaller, [string]$Operation = 'restore', [int]$ExitCode = 0
) {
  Remove-Item -LiteralPath $registryPath -Recurse -Force -ErrorAction SilentlyContinue
  if ($Previous) {
    New-Item -Path $registryPath -Force | Out-Null
    Set-Item -LiteralPath $registryPath -Value $Previous
  }
  $inputLines = @('[case]', "selected=$Selected", "operation=$Operation")
  [IO.File]::WriteAllLines((Join-Path $root 'case.ini'), $inputLines, [Text.UnicodeEncoding]::new($false, $true))
  Remove-Item -LiteralPath (Join-Path $root 'result.ini') -ErrorAction SilentlyContinue
  $process = Start-Process -FilePath (Join-Path $root 'harness.exe') -ArgumentList '/S' -PassThru
  if (!$process.WaitForExit(30000)) {
    $process.Kill()
    throw "Timed out: $Name"
  }
  Assert-Equal $process.ExitCode $ExitCode "$Name exit code"
  if ($ExitCode -eq 0) {
    $result = @{}
    Get-Content -LiteralPath (Join-Path $root 'result.ini') | ForEach-Object {
      if ($_ -match '^([^=]+)=(.*)$') { $result[$Matches[1]] = $Matches[2] }
    }
    Assert-Equal $result.path $Expected "$Name path"
    Assert-Equal $result.missing ([string]$Missing) "$Name orphan detection"
    Assert-Equal $result.uninstaller ([string]$Uninstaller) "$Name uninstaller detection"
    Assert-Equal $result.registers 'preserved' "$Name NSIS register preservation"
  } else {
    Assert-Equal (Test-Path -LiteralPath (Join-Path $root 'result.ini')) $false "$Name must abort before completing"
  }
  if ($Previous) {
    Assert-Equal ((Get-Item -LiteralPath $registryPath).GetValue('')) $Previous "$Name registration must remain unchanged"
  } else {
    Assert-Equal (Test-Path -LiteralPath $registryPath) $false "$Name must not create registration"
  }
  $script:passed++
  Write-Output "PASS $Name"
}

try {
  if (!(Test-Path -LiteralPath $MakeNsis)) {
    # Same version and pinned digest as Tauri CLI 2.12.1's NSIS toolchain.
    # Bootstrap here so permission regressions fail before the full app build.
    $archive = Join-Path $root 'nsis.zip'
    Invoke-WebRequest 'https://github.com/tauri-apps/binary-releases/releases/download/nsis-3.11/nsis-3.11.zip' -OutFile $archive
    Assert-Equal (Get-FileHash -LiteralPath $archive -Algorithm SHA1).Hash 'EF7FF767E5CBD9EDD22ADD3A32C9B8F4500BB10D' 'NSIS toolchain digest'
    Expand-Archive -LiteralPath $archive -DestinationPath $root
    $MakeNsis = Join-Path $root 'nsis-3.11/makensis.exe'
  }
  $templatePath = Join-Path $PSScriptRoot '../apps/desktop/src-tauri/windows/installer.nsi'
  $template = Get-Content -Raw -LiteralPath $templatePath
  $helpers = [regex]::Match($template, '(?s); BEGIN CODEXPULSE INSTALL PATH HELPERS[^\r\n]*\r?\n(.*?); END CODEXPULSE INSTALL PATH HELPERS')
  if (!$helpers.Success) { throw 'Production helper block not found' }
  $harness = @'
Unicode true
RequestExecutionLevel user
SilentInstall silent
!include LogicLib.nsh
!include FileFunc.nsh
!define MANUPRODUCTKEY "__TEST_KEY__"
!define MAINBINARYNAME "codexpulse"
!define PRODUCTNAME "CodexPulse"
!define INSTALLMODE "currentUser"
Var InstallDirWritable
Var PreviousInstallMissing
Var PreviousUninstallerExists
Var Operation
Name "CodexPulse installer regression harness"
OutFile "harness.exe"
Function .onInit
  SetShellVarContext current
  ReadINIStr $INSTDIR "$EXEDIR\case.ini" "case" "selected"
  ReadINIStr $Operation "$EXEDIR\case.ini" "case" "operation"
FunctionEnd
__PRODUCTION_HELPERS__
Section
  StrCpy $0 "register0"
  StrCpy $1 "register1"
  StrCpy $2 "register2"
  StrCpy $4 "register4"
  ${If} $Operation == "restore"
    Call RestorePreviousInstallLocation
  ${Else}
    Call EnsureInstallDirWritable
  ${EndIf}
  Call IsPreviousInstallMissing
  Call CheckPreviousUninstaller
  ${If} $0 != "register0"
  ${OrIf} $1 != "register1"
  ${OrIf} $2 != "register2"
  ${OrIf} $4 != "register4"
    SetErrorLevel 99
    Abort
  ${EndIf}
  ; WritePrivateProfileString otherwise creates an ANSI file and loses Unicode paths.
  FileOpen $3 "$EXEDIR\result.ini" w
  FileWriteUTF16LE /BOM $3 "[result]$\r$\n"
  FileClose $3
  WriteINIStr "$EXEDIR\result.ini" "result" "path" "$INSTDIR"
  WriteINIStr "$EXEDIR\result.ini" "result" "missing" "$PreviousInstallMissing"
  WriteINIStr "$EXEDIR\result.ini" "result" "uninstaller" "$PreviousUninstallerExists"
  WriteINIStr "$EXEDIR\result.ini" "result" "registers" "preserved"
  SetErrorLevel 0
SectionEnd
'@
  $harness = $harness.Replace('__TEST_KEY__', $key).Replace('__PRODUCTION_HELPERS__', $helpers.Groups[1].Value)
  $source = Join-Path $root 'harness.nsi'
  [IO.File]::WriteAllText($source, $harness, [Text.UTF8Encoding]::new($false))
  Push-Location $root
  try {
    & $MakeNsis -INPUTCHARSET UTF8 -V2 $source
    if ($LASTEXITCODE -ne 0) { throw 'NSIS harness compilation failed' }
  } finally { Pop-Location }

  $default = Join-Path $root 'default'
  Invoke-Case 'fresh install keeps default' '' $default $default 0 0

  $writable = Join-Path $root '旧安装路径 with spaces'
  Invoke-Case 'missing but writable old path is retained' $writable $default $writable 1 0
  Assert-Equal (Test-Path -LiteralPath $writable) $false 'write probe removes its empty directory'

  $blocked = Join-Path $root 'blocked-parent'
  New-Item -ItemType Directory -Path $blocked | Out-Null
  Deny-Access $blocked ([Security.AccessControl.FileSystemRights]::Write)
  $stale = Join-Path $blocked 'missing-app'
  Invoke-Case 'missing old path under protected parent falls back' $stale $default $default 1 0
  Assert-Equal (Test-Path -LiteralPath $stale) $false 'blocked old path was not created'
  Invoke-Case 'explicit protected path fails silently' '' $stale '' 0 0 'preflight' 2

  $live = Join-Path $root 'live-install'
  New-Item -ItemType Directory -Path $live | Out-Null
  Set-Content -LiteralPath (Join-Path $live 'codexpulse.exe') -Value 'application sentinel' -NoNewline
  Set-Content -LiteralPath (Join-Path $live 'uninstall.exe') -Value 'uninstaller sentinel' -NoNewline
  Deny-Access $live ([Security.AccessControl.FileSystemRights]::Write)
  Invoke-Case 'existing protected installation is never redirected' $live $default $live 0 1
  Invoke-Case 'silent update of protected installation fails' $live $live '' 0 1 'preflight' 2
  Assert-Equal (Get-Content -Raw -LiteralPath (Join-Path $live 'codexpulse.exe')) 'application sentinel' 'application is intact'
  Assert-Equal (Get-Content -Raw -LiteralPath (Join-Path $live 'uninstall.exe')) 'uninstaller sentinel' 'uninstaller is intact'
  Assert-Equal @(Get-ChildItem -LiteralPath $live -Force).Count 2 'no probe files remain'

  $repair = Join-Path $root 'repair-install'
  New-Item -ItemType Directory -Path $repair | Out-Null
  Set-Content -LiteralPath (Join-Path $repair 'codexpulse.exe') -Value 'application sentinel' -NoNewline
  Invoke-Case 'application without uninstaller can be repaired in place' $repair $default $repair 0 0
  Invoke-Case 'writable existing installation passes preflight' $repair $repair $repair 0 0 'preflight'
  Assert-Equal @(Get-ChildItem -LiteralPath $repair -Force).Count 1 'successful probe leaves no files'

  $uninstallerOnly = Join-Path $root 'uninstaller-only'
  New-Item -ItemType Directory -Path $uninstallerOnly | Out-Null
  Set-Content -LiteralPath (Join-Path $uninstallerOnly 'uninstall.exe') -Value 'uninstaller sentinel' -NoNewline
  Invoke-Case 'remaining uninstaller is not an orphan' $uninstallerOnly $default $uninstallerOnly 0 1

  $inaccessible = Join-Path $root 'inaccessible-install'
  New-Item -ItemType Directory -Path $inaccessible | Out-Null
  Set-Content -LiteralPath (Join-Path $inaccessible 'codexpulse.exe') -Value 'application sentinel'
  Deny-Access $inaccessible ([Security.AccessControl.FileSystemRights]::ReadAndExecute)
  Invoke-Case 'access denied is not mistaken for missing files' $inaccessible $default $inaccessible 0 0

  $newPath = Join-Path $root '新安装目录 with spaces'
  Invoke-Case 'new Unicode directory passes preflight' '' $newPath $newPath 0 0 'preflight'
  Assert-Equal (Test-Path -LiteralPath $newPath) $false 'new directory probe is cleaned up'
  Write-Output "Passed $passed Windows installer regression cases"
} finally {
  foreach ($entry in $deniedPaths) {
    $acl = Get-Acl -LiteralPath $entry.Path
    $acl.RemoveAccessRuleSpecific($entry.Rule)
    Set-Acl -LiteralPath $entry.Path -AclObject $acl
  }
  Remove-Item -LiteralPath $registryPath -Recurse -Force -ErrorAction SilentlyContinue
  Remove-Item -LiteralPath $root -Recurse -Force
}
