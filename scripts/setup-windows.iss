; Pane's Windows setup (#463): Pane-Setup-<version>.exe, built by Inno
; Setup's compiler from the files `cargo xtask package-windows` staged for
; the zip package (xtask/src/package.rs runs it as
;   ISCC /DAppVersion=<version> /DStageDir=<stage> /O<release folder>
; and writes the setup's .sha256 beside it).
;
; It installs Pane for one user, with no administrator rights, into
; %LOCALAPPDATA%\Pane: the folder install.ps1 has always installed to, so a
; setup run over an install the script made replaces it in place (the same
; pane.exe, the same Start menu shortcut, the same launch-at-login command).
; Pane keeps its data in %LOCALAPPDATA%\Pane\data, its caches and log beside
; it (pane's `data_dir`, `cache_dir`, `logs_dir`), whichever folder the
; program is installed in; the setup never writes there, and its uninstall
; removes them only if the user says so.
;
; The Apps entry it writes (HKCU's Uninstall key `<AppId>_is1`) names the
; version installed. Pane's own in-app update replaces pane.exe without the
; setup, and then gives that entry the new version itself (pane-core's
; `application_update`, which names the same AppId).

#ifndef AppVersion
  #error Name the version: ISCC /DAppVersion=<version>
#endif
#ifndef StageDir
  #error Name the staged package: ISCC /DStageDir=<folder holding pane.exe>
#endif

[Setup]
; The setup's identity, the same for every version of Pane forever: Windows
; and the setup find an earlier install by it, and pane-core's
; application_update names the Apps entry by it. Never change it.
AppId={{0737054E-4FDB-4FAD-8AD1-20C980B0C16E}
AppName=Pane
AppVersion={#AppVersion}
AppVerName=Pane {#AppVersion}
AppPublisher=pane-app
AppPublisherURL=https://github.com/pane-app/pane
AppSupportURL=https://github.com/pane-app/pane/issues
AppUpdatesURL=https://github.com/pane-app/pane/releases
AppCopyright=Copyright (C) The Pane contributors. GPL-3.0-or-later.
VersionInfoVersion={#AppVersion}
VersionInfoProductName=Pane
VersionInfoDescription=Pane setup
; One user's install: no elevation is asked for, and every folder and key
; written is the user's own.
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
DefaultDirName={localappdata}\Pane
; The folder holds Pane's data already when Pane ran before: that is
; expected, not a reason to warn.
DirExistsWarning=no
DisableProgramGroupPage=yes
; The Apps entry is named Pane, and its version column says which.
UninstallDisplayName=Pane
UninstallDisplayIcon={app}\pane.exe
SetupIconFile=..\crates\pane\assets\app\pane.ico
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes
OutputBaseFilename=Pane-Setup-{#AppVersion}
; A Pane running from the install folder is closed before its program is
; replaced (Windows' Restart Manager asks it to end its session, which
; Pane answers by quitting cleanly; one that does not is ended). The
; program an in-app update renamed away, pane.exe.old, is still the one
; running until Pane restarts, so it is checked too. Pane is started again
; by the [Run] entries below, not by the Restart Manager.
CloseApplications=force
CloseApplicationsFilter=*.exe,*.exe.old
RestartApplications=no
; The sign-in task is never inherited from an earlier install: after the
; install, the choice is Pane's own setting (Settings > General), which the
; user may have changed since.
UsePreviousTasks=no

[Tasks]
Name: startatsignin; Description: "Start Pane when I sign in"; Flags: unchecked

[Files]
Source: "{#StageDir}\pane.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#StageDir}\LICENSE.txt"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#StageDir}\THIRD-PARTY-NOTICES.txt"; DestDir: "{app}"; Flags: ignoreversion

[InstallDelete]
; What an in-app update leaves until Pane's next start: the program it
; renamed away and a staging folder a stopped update left.
Type: files; Name: "{app}\pane.exe.old"
Type: filesandordirs; Name: "{app}\update"

[UninstallDelete]
Type: files; Name: "{app}\pane.exe.old"
Type: filesandordirs; Name: "{app}\update"

[Icons]
; The same shortcut install.ps1 makes, so an install made by the script
; keeps one Pane entry in the Start menu.
Name: "{userprograms}\Pane"; Filename: "{app}\pane.exe"; WorkingDir: "{app}"; Comment: "Pane, a desktop launcher"

[Run]
; "Start Pane when I sign in" is Pane's own launch-at-login setting: Pane
; records the choice and registers itself (the Run key's Pane value), so
; the setting and the registration cannot disagree. It starts Pane.
Filename: "{app}\pane.exe"; Parameters: "--launch-at-login"; Flags: nowait; Tasks: startatsignin
Filename: "{app}\pane.exe"; Description: "Start Pane"; Flags: nowait postinstall skipifsilent; Tasks: not startatsignin

[Code]
// The folder Pane keeps its data, caches and log in (pane's `data_dir`,
// `cache_dir` and `logs_dir`), whichever folder the program is in.
function PaneFolder(): String;
begin
  Result := ExpandConstant('{localappdata}\Pane');
end;

// Whether a process of this install's program is running: pane.exe, or
// the pane.exe.old an in-app update renamed the running program to.
function IsThisInstall(Path: String): Boolean;
begin
  Result := (CompareText(Path, ExpandConstant('{app}\pane.exe')) = 0) or
    (CompareText(Path, ExpandConstant('{app}\pane.exe.old')) = 0);
end;

// The ids of the running processes of this install's program.
function RunningPanes(var Ids: TArrayOfString): Integer;
var
  Locator, Service, Processes, Process: Variant;
  I: Integer;
begin
  Result := 0;
  SetArrayLength(Ids, 0);
  try
    Locator := CreateOleObject('WbemScripting.SWbemLocator');
    Service := Locator.ConnectServer('.', 'root\CIMV2');
    Processes := Service.ExecQuery(
      'SELECT ProcessId, ExecutablePath FROM Win32_Process ' +
      'WHERE Name = ''pane.exe'' OR Name = ''pane.exe.old''');
    for I := 0 to Processes.Count - 1 do
    begin
      Process := Processes.ItemIndex(I);
      if not VarIsNull(Process.ExecutablePath) and IsThisInstall(Process.ExecutablePath) then
      begin
        SetArrayLength(Ids, Result + 1);
        Ids[Result] := IntToStr(Process.ProcessId);
        Result := Result + 1;
      end;
    end;
  except
    Log('The running Panes could not be listed: ' + GetExceptionMessage);
  end;
end;

// Closes the Panes running from this install before it is removed: each
// is asked to close (taskkill without /F closes its windows, and closing
// the launcher's window quits Pane cleanly), and one still running after
// ten seconds is ended.
procedure ClosePanes();
var
  Ids: TArrayOfString;
  I, Waited, Code: Integer;
begin
  if RunningPanes(Ids) = 0 then
    exit;
  for I := 0 to GetArrayLength(Ids) - 1 do
    Exec(ExpandConstant('{sys}\taskkill.exe'), '/PID ' + Ids[I], '', SW_HIDE,
      ewWaitUntilTerminated, Code);
  // What is still running is listed again, so an id is only ever one of
  // this install's Panes.
  Waited := 0;
  while (RunningPanes(Ids) > 0) and (Waited < 10000) do
  begin
    Sleep(250);
    Waited := Waited + 250;
  end;
  for I := 0 to GetArrayLength(Ids) - 1 do
    Exec(ExpandConstant('{sys}\taskkill.exe'), '/F /PID ' + Ids[I], '', SW_HIDE,
      ewWaitUntilTerminated, Code);
end;

// Removes Pane's launch-at-login registration when it starts this
// install's program; one naming another Pane is left alone.
procedure RemoveSignIn();
var
  Command: String;
begin
  if RegQueryStringValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run', 'Pane', Command) and
    (CompareText(Command, '"' + ExpandConstant('{app}\pane.exe') + '"') = 0) then
    RegDeleteValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run', 'Pane');
end;

// Asks whether Pane's data goes too, and removes it if so. The default,
// and a silent uninstall's answer, is to keep it, for a later install.
procedure RemoveDataIfAsked();
var
  Folder: String;
begin
  Folder := PaneFolder();
  if not (DirExists(Folder + '\data') or DirExists(Folder + '\cache') or DirExists(Folder + '\logs')) then
    exit;
  if SuppressibleMsgBox('Remove Pane''s data too?' + #13#10#13#10 +
    'Your installed extensions, their settings and Pane''s own settings are in ' + Folder +
    '. Choose No to keep them for when you install Pane again.',
    mbConfirmation, MB_YESNO or MB_DEFBUTTON2, IDNO) <> IDYES then
    exit;
  DelTree(Folder + '\data', True, True, True);
  DelTree(Folder + '\cache', True, True, True);
  DelTree(Folder + '\logs', True, True, True);
  RemoveDir(Folder);
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  case CurUninstallStep of
    usUninstall:
      begin
        ClosePanes();
        RemoveSignIn();
      end;
    usPostUninstall:
      RemoveDataIfAsked();
  end;
end;
