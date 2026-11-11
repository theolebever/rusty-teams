; Inno Setup script for the Windows installer, built in .github/workflows/build.yml.
; Per-user install (no admin prompt) of the same teams.exe that ships as the portable build.
; iscc /DAppVersion=0.2.0 /DSourceExe=target\release\teams.exe /O<dir> scripts\windows-installer.iss

#ifndef AppVersion
  #define AppVersion "dev"
#endif
#ifndef SourceExe
  #define SourceExe "..\target\release\teams.exe"
#endif

[Setup]
AppId={{6E2B7C1A-5F3D-4B8E-9A21-7C4D0E9F3B52}
AppName=Rusty Teams
AppVersion={#AppVersion}
AppPublisher=Rusty Teams
DefaultDirName={localappdata}\Programs\Rusty Teams
DefaultGroupName=Rusty Teams
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
SetupIconFile=..\crates\app\assets\icon\teams-fast.ico
UninstallDisplayIcon={app}\teams.exe
OutputBaseFilename=rusty-teams-setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern

[Tasks]
Name: desktopicon; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#SourceExe}"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Rusty Teams"; Filename: "{app}\teams.exe"
Name: "{autodesktop}\Rusty Teams"; Filename: "{app}\teams.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\teams.exe"; Description: "{cm:LaunchProgram,Rusty Teams}"; Flags: nowait postinstall skipifsilent
