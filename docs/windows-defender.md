# Windows Defender and SmartScreen

KLIF starts other programs, reads GPU and process counters, probes local HTTP ports and can open a network
listener when you ask. An executable that does these things and has no reputation yet can be flagged by
heuristics, or shown the "Windows protected your PC" SmartScreen page. Nobody can promise this will not happen on
your machine, and this project does not claim its builds are "clean" for all time.

What the project does to avoid looking like malware:

- KLIF's programs do not start PowerShell, `cmd.exe` or any script host themselves. They start exactly the program
  in your preset.
- No network port is opened unless `[node] listen` is set. The control channel `klif-cli` uses listens on
  `127.0.0.1` only and needs a per-run token.
- The executables carry full version information and a manifest that asks for no elevation (`asInvoker`).
- UI assets are embedded uncompressed, and release builds strip the builder's local paths.
- The hardware-identification reader that `klif-cli diag` uses for the RAM type is compiled only into
  `klif-cli.exe`, never into `klif.exe`.
- `Build-Release.ps1 -Sign` signs both executables when you provide a certificate or an Azure Trusted Signing
  profile through environment variables (see the script). Signing is optional and not set up in this repository.

What you can do:

- Build from source and compare the SHA-256 the build prints with `Get-FileHash -Algorithm SHA256 <file>`.
- Scan a file without remediation:

  ```powershell
  & "$env:ProgramFiles\Windows Defender\MpCmdRun.exe" -Scan -ScanType 3 -File "C:\path\to\klif.exe" -DisableRemediation
  ```

  One scan reflects one set of definitions at one time. Treat it as a data point, not as a certificate.
- You do not need to turn Defender off or exclude a folder to run KLIF, and you should not.

**If Defender flags a build you made from this source**, report it as a false positive to Microsoft:

1. Note the detection name: Windows Security, **Protection history**, or
   `Get-MpThreatDetection | Select-Object ThreatID, Resources, InitialDetectionTime` and `Get-MpThreat`.
2. Note the definitions version: Windows Security, **Virus & threat protection updates**, or
   `(Get-MpComputerStatus).AntivirusSignatureVersion`.
3. Open the Microsoft Security Intelligence file submission page,
   <https://www.microsoft.com/en-us/wdsi/filesubmission>. Signing in with a Microsoft account is optional, but it
   lets you track the submission.
4. Choose the submitter type **Software developer** and the reason **Incorrectly detected as malware/malicious**
   (not "Malware/malicious").
5. Upload the file, enter the detection name and the definitions version, and say in English what the program is:
   an open-source process manager at <https://github.com/koksny/klif>, the build commit, the SHA-256, and the
   behaviour that likely triggered the detection (it starts child processes and reads GPU counters).
6. Wait for Microsoft's answer. A cleared detection reaches machines with a later definitions update. The form may
   change; if it does, look for "submit a file for malware analysis" on Microsoft Security Intelligence.

A SmartScreen page for a file you built yourself or whose hash you verified can be passed with **More info**, then
**Run anyway**. Do not do that for a file someone else sent you.
