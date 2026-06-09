; WB-300 — Global Edition installer (perMachine, requires admin).
;
; Built by .github/workflows/windows-installers.yml after release.yml finishes
; publishing the GitHub Release. Companion to inno/corporate.iss (perUser
; sibling).
;
; Builds with Inno Setup 6 (`iscc` from JRSoftware) on a Windows runner. CI
; passes the version via `iscc /DMyAppVersion=1.0.0` so the same script
; rebuilds at every release without editing.
;
; The MSI sibling lives at wix/main.wxs. Both installers target the SAME
; install path (C:\Program Files\wb300\bin\wb300.exe) and write the SAME
; registry marker (HKCU\Software\WB300) — only the InstallSource value
; differs (msi-global vs exe-global). README documents "pick one format per
; edition" since coexistence creates duplicate Add/Remove Programs entries.
;
; If install path changes here, update src/update.rs::detect_install_origin()
; in lockstep — that function matches on the install path to choose which
; installer to fetch during `wb300 update`. The AppId GUID is PERMANENT.

#ifndef MyAppVersion
  #define MyAppVersion "0.0.0-dev"
#endif

#define MyAppName "wb300"
#define MyAppPublisher "Emmett S"
#define MyAppURL "https://github.com/QubeTX/qube-workbranch-view"
#define MyAppExeName "wb300.exe"

[Setup]
; AppId is the immutable identity of the Global EXE installer.
; Different from the MSI Global's UpgradeCode (Windows treats MSI products and
; Inno Setup products as separate even when they target the same install path)
; and different from the Corporate EXE's AppId.
AppId={{DDA836A5-116A-4D4F-8FDF-5A6366CFE883}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppVerName={#MyAppName} {#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}
AppUpdatesURL={#MyAppURL}/releases
; perMachine install: %ProgramFiles%\wb300 — same path as MSI Global by design.
DefaultDirName={commonpf}\{#MyAppName}
DefaultGroupName={#MyAppName}
; CLI tool — no start menu group, no desktop shortcut.
DisableProgramGroupPage=yes
DisableDirPage=auto
; Require admin (perMachine scope). Triggers UAC prompt at install start.
PrivilegesRequired=admin
PrivilegesRequiredOverridesAllowed=
ArchitecturesAllowed=x64
ArchitecturesInstallIn64BitMode=x64
OutputBaseFilename=wb300-x86_64-pc-windows-msvc-setup
OutputDir=Output
Compression=lzma
SolidCompression=yes
WizardStyle=modern
; Tell Windows we touched env vars so File Explorer broadcasts WM_SETTINGCHANGE.
; New cmd / PowerShell sessions then pick up the PATH addition without reboot.
ChangesEnvironment=yes
; ARP display name. Matches the MSI Global's Product Name so users see the
; same label regardless of which installer they used.
UninstallDisplayName={#MyAppName}
; Embed the LICENSE file so the installer wizard shows it (PolyForm
; Noncommercial 1.0.0).
LicenseFile=..\LICENSE
SetupLogging=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Files]
; Bundles wb300.exe from target/release/. The CI workflow runs cargo build
; --release before invoking iscc so this path is populated.
Source: "..\target\release\{#MyAppExeName}"; DestDir: "{app}\bin"; Flags: ignoreversion

[Registry]
; Install-source marker. `wb300 update` reads HKCU\Software\WB300\InstallSource
; and picks the matching installer to download for in-place upgrades. Value
; must match the `exe-global` arm in src/update.rs.
Root: HKCU; Subkey: "Software\WB300"; ValueType: string; ValueName: "InstallSource"; ValueData: "exe-global"; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\WB300"; Flags: uninsdeletekeyifempty

[Code]
{
  PATH management — system PATH (HKLM) for the Global perMachine edition.
  Inno Setup's [Registry] section can't safely append-without-duplicates +
  reliably remove-on-uninstall, so we do it explicitly in [Code].
  The canonical pattern, adapted from the Inno Setup community knowledge base.
}
const
  EnvironmentKey = 'SYSTEM\CurrentControlSet\Control\Session Manager\Environment';

procedure EnvAddPath(Path: string);
var
  Paths: string;
begin
  if not RegQueryStringValue(HKEY_LOCAL_MACHINE, EnvironmentKey, 'Path', Paths) then
    Paths := '';

  // Skip if already in PATH (case-insensitive substring match with
  // ;-padding so we don't match a prefix of a different directory).
  if Pos(';' + Uppercase(Path) + ';', ';' + Uppercase(Paths) + ';') > 0 then exit;

  if Length(Paths) > 0 then
    Paths := Paths + ';' + Path
  else
    Paths := Path;

  RegWriteExpandStringValue(HKEY_LOCAL_MACHINE, EnvironmentKey, 'Path', Paths);
end;

procedure EnvRemovePath(Path: string);
var
  Paths: string;
  P: Integer;
begin
  if not RegQueryStringValue(HKEY_LOCAL_MACHINE, EnvironmentKey, 'Path', Paths) then
    exit;

  P := Pos(';' + Uppercase(Path) + ';', ';' + Uppercase(Paths) + ';');
  if P = 0 then exit;

  if P = 1 then
    // First-entry case. Delete(Paths, P - 1, ...) would be Delete(Paths, 0, ...)
    // which is undefined behavior in Pascal Script (treated as a no-op), so the
    // entry would be stranded in PATH after uninstall. Most likely on fresh
    // corporate workstations where SYSTEM Path is empty before install, so
    // wb300's bin lands at index 1. With this branch:
    //   Paths = "X;Y"  -> "Y"    (eats "X;")
    //   Paths = "X;"   -> ""     (eats "X;")
    //   Paths = "X"    -> ""     (eats "X", count clamps to remaining)
    Delete(Paths, 1, Length(Path) + 1)
  else
    // Middle/end entry: consume the leading `;` plus the path.
    //   Paths = "A;X;B" -> "A;B" (eats ";X")
    //   Paths = "A;X"   -> "A"   (eats ";X")
    Delete(Paths, P - 1, Length(Path) + 1);

  RegWriteExpandStringValue(HKEY_LOCAL_MACHINE, EnvironmentKey, 'Path', Paths);
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
    EnvAddPath(ExpandConstant('{app}') + '\bin');
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then
    EnvRemovePath(ExpandConstant('{app}') + '\bin');
end;
