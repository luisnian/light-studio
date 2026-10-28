; Inno Setup script. Built by scripts/make-installer.ps1 (pass /DAppVersion=x.y.z).
#ifndef AppVersion
  #define AppVersion "0.1.0"
#endif

[Setup]
AppId={{B6E2D0A4-6C3F-4C7B-9D0E-5B1A7E3F2A10}
AppName=Light Studio
AppVersion={#AppVersion}
AppPublisher=Luis Nieto
DefaultDirName={autopf}\Light Studio
DefaultGroupName=Light Studio
DisableProgramGroupPage=yes
OutputDir=..\dist
OutputBaseFilename=Light-Studio-{#AppVersion}-setup
SetupIconFile=..\assets\icon.ico
UninstallDisplayIcon={app}\light-studio.exe
Compression=lzma2
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
WizardStyle=modern
LicenseFile=..\LICENSE

[Languages]
Name: "spanish"; MessagesFile: "compiler:Languages\Spanish.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "..\target\release\light-studio.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\Light Studio"; Filename: "{app}\light-studio.exe"
Name: "{autodesktop}\Light Studio"; Filename: "{app}\light-studio.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\light-studio.exe"; Description: "{cm:LaunchProgram,Light Studio}"; Flags: nowait postinstall skipifsilent
