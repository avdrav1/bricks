; WIN-5: each app's per-user Windows installer, one template for the four apps. Built by
; scripts/win_bundle.sh with Inno Setup 6:
;
;   iscc /DAppDefs=APP.iss /DSource=BUNDLE /DOutputDir=DIST packaging\windows\setup.iss
;
; APP.iss (scripts/win_app.py iss APP) defines App, Name, Version, AppId, Description, Url,
; and Types, and registers the app's file types for the user ([Registry], HKCU). BUNDLE is
; the app's self-contained folder (bin\<app>.exe and GTK's DLLs, lib, share, etc, fonts,
; licenses, <app>.ico). The result is DIST\<app>-<version>-x86_64-windows-setup.exe.
;
; No admin rights: it installs for the user alone into %LOCALAPPDATA%\Programs\<Name>, with
; a Start menu entry and an entry in Settings > Apps (HKCU's Uninstall key). File types are
; offered through Open with and Default apps; Windows keeps the user's defaults. Silent:
; Setup.exe /VERYSILENT /SUPPRESSMSGBOXES /NORESTART /CURRENTUSER (what install.ps1 runs).
#ifndef AppDefs
  #error Build it with scripts/win_bundle.sh (iscc /DAppDefs=... /DSource=... /DOutputDir=...)
#endif
#include AppDefs

[Setup]
AppId={#AppId}
AppName={#Name}
AppVersion={#Version}
AppVerName={#Name} {#Version}
AppPublisher=Cronkite
AppPublisherURL={#Url}
AppSupportURL={#Url}
AppUpdatesURL={#Url}
AppComments={#Description}
VersionInfoVersion={#Version}.0
VersionInfoProductName={#Name}
VersionInfoProductVersion={#Version}
VersionInfoDescription={#Name} Setup
VersionInfoCompany=Cronkite
PrivilegesRequired=lowest
; {autopf} is %LOCALAPPDATA%\Programs for a per-user install.
DefaultDirName={autopf}\{#Name}
DisableProgramGroupPage=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
; MSYS2's UCRT64 GTK needs Windows 10.
MinVersion=10.0
WizardStyle=modern
SetupIconFile={#Source}\{#App}.ico
UninstallDisplayIcon={app}\{#App}.ico
UninstallDisplayName={#Name}
OutputDir={#OutputDir}
OutputBaseFilename={#App}-{#Version}-x86_64-windows-setup
Compression=lzma2/ultra64
SolidCompression=yes
ChangesAssociations=yes
CloseApplications=yes
RestartApplications=no

[InstallDelete]
; An update replaces the bundle as a whole: no DLL of an older release stays behind.
Type: filesandordirs; Name: "{app}\bin"
Type: filesandordirs; Name: "{app}\lib"
Type: filesandordirs; Name: "{app}\share"
Type: filesandordirs; Name: "{app}\etc"
Type: filesandordirs; Name: "{app}\fonts"
Type: filesandordirs; Name: "{app}\licenses"

[Files]
Source: "{#Source}\*"; DestDir: "{app}"; Flags: recursesubdirs createallsubdirs ignoreversion

[Icons]
Name: "{autoprograms}\{#Name}"; Filename: "{app}\bin\{#App}.exe"; IconFilename: "{app}\{#App}.ico"; Comment: "{#Description}"

[Run]
Filename: "{app}\bin\{#App}.exe"; Description: "{cm:LaunchProgram,{#Name}}"; Flags: nowait postinstall skipifsilent

[Code]
// The user's own files stay (Windows' convention; as the Linux and macOS installers keep
// theirs): settings and recent files in %APPDATA%\Cronkite\<Name>, recovery journals and
// state in %LOCALAPPDATA%\Cronkite\<Name> (WIN-2). Uninstalling by hand asks whether to
// remove them too (No by default); a silent uninstall keeps them. The fontconfig cache the
// bundle keeps there (suite_platform::bundle) always goes.
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Roaming, Local: String;
begin
  if CurUninstallStep = usPostUninstall then
  begin
    Roaming := ExpandConstant('{userappdata}\Cronkite\{#Name}');
    Local := ExpandConstant('{localappdata}\Cronkite\{#Name}');
    DelTree(Local + '\fontconfig', True, True, True);
    // (A line of [Code] mustn't start with "[": Inno would take it for a section.)
    Log(Format('User data: %s (%d), %s (%d); silent: %d', [Roaming, Ord(DirExists(Roaming)),
      Local, Ord(DirExists(Local)), Ord(UninstallSilent)]));
    if (not UninstallSilent) and (DirExists(Roaming) or DirExists(Local)) then
      if MsgBox('Also remove your {#Name} settings, recent files, and recovered work?' + #13#10#13#10 +
                Roaming + #13#10 + Local + #13#10#13#10 +
                'Choose No to keep them for when you install {#Name} again.',
                mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES then
      begin
        Log('Removing the user data');
        DelTree(Roaming, True, True, True);
        DelTree(Local, True, True, True);
      end
      else
        Log('Keeping the user data');
    RemoveDir(Local);
    RemoveDir(Roaming);
    RemoveDir(ExpandConstant('{localappdata}\Cronkite'));
    RemoveDir(ExpandConstant('{userappdata}\Cronkite'));
    // By now the uninstaller's own files are gone too: the app's folder goes, then
    // %LOCALAPPDATA%\Programs, which Setup made if this was the user's first per-user
    // program, when nothing else is in it.
    RemoveDir(ExpandConstant('{app}'));
    RemoveDir(ExpandConstant('{userpf}'));
  end;
end;
