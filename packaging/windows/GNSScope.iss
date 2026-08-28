#define MyAppName "GNSScope"
#define MyAppPublisher "GNSScope contributors"
#define MyAppExeName "GNSScope.exe"

#ifndef MyAppVersion
  #error MyAppVersion must be provided with /DMyAppVersion=<version>
#endif
#ifndef MySourceExe
  #error MySourceExe must be provided with /DMySourceExe=<path>
#endif
#ifndef MyOutputDir
  #error MyOutputDir must be provided with /DMyOutputDir=<path>
#endif

[Setup]
AppId={{7E55EA6F-7385-4B8F-9A0A-72635B0674EC}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
DefaultDirName={localappdata}\Programs\{#MyAppName}
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
OutputDir={#MyOutputDir}
OutputBaseFilename=GNSScope-v{#MyAppVersion}-windows-x86_64-setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
UninstallDisplayIcon={app}\{#MyAppExeName}

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Additional shortcuts:"; Flags: unchecked

[Files]
Source: "{#MySourceExe}"; DestDir: "{app}"; DestName: "{#MyAppExeName}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "Launch {#MyAppName}"; Flags: nowait postinstall skipifsilent
