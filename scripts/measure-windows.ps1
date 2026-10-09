# Pane's cost while hidden, on Windows (#189): the hidden-idle phase of
# scripts/measure-linux.sh, measured on this computer for a scratch Pane.
#
# What it does: starts the Pane build given (the development build by
# default) on a scratch profile in the temporary folder - PANE_DATA_DIR and
# LOCALAPPDATA (where Pane keeps its cache) point there for that one
# process - so Pane acquires its default extensions there, and nothing
# else, from the artifact source this script serves on 127.0.0.1
# (scripts/artifact_server.py over target/dist/artifacts, which
# `cargo xtask package-windows --dev` assembles; only a development build
# takes its artifact source from PANE_ARTIFACTS). Files' index covers an
# empty folder of the scratch profile (PANE_TEST_FILE_INDEX_HOME, read by
# development builds only), not the home folder. The launcher's window is
# shown once, as every start shows it, and hidden at once with Escape at
# its blank root search, the way a user hides it. Once the defaults are
# recorded and Pane has settled, the script samples Pane's process tree
# for the hidden phase: every process's CPU time and working set, and
# every thread's context switches and CPU time, named by the thread's
# name (SetThreadDescription, which Rust's std gives every named thread).
# The samples have proc_tree.py's shape, and its `summary` writes the same
# summary.json as the Linux workload: phases hidden-setup and hidden-idle,
# each with CPU share and the wake-ups by thread name.
#
# What it never does: it captures nothing of the screen; it sends the one
# Escape to Pane's own window (a posted key message; only when that does
# not hide it, and Pane's window is the foreground window, a typed Escape);
# it never brings Pane forward. It never reads or writes the user's own
# Pane data folder or cache: the scratch Pane's data starts empty, and its
# settings record is written first with the launch-at-login choice the
# startup list already holds, so the scratch Pane leaves the user's own
# Pane's "Pane" value in HKCU\...\Run as it is (the script also checks the
# value afterwards and puts it back if anything changed it). The scratch
# profile, which holds whatever Clipboard History recorded during the run,
# is removed at the end (-KeepScratch keeps it). It quits only the Pane it
# started, and warns when another Pane (the user's own) runs. While it
# runs, the scratch Pane also shows its tray icon and takes the default
# hotkeys if no other Pane holds them. The scratch LOCALAPPDATA also moves
# the ClickOnce store and %LOCALAPPDATA%\Packages, whose watch is then of
# an empty folder: their cost is not in the number (see
# docs/research/resource-measurements.md).
#
# It runs only with the user's consent: the measured machine is the user's
# own. Requires 64-bit PowerShell (5.1 or 7), Python 3 on PATH, the Pane
# build, and target/dist/artifacts.
#
# Usage: scripts/measure-windows.ps1 [-OutDir measure-windows]
#   [-Binary target/debug/pane.exe] [-Artifacts target/dist/artifacts]
#   [-HiddenSeconds 60] [-SettleSeconds 30] [-SampleSeconds 1]
#   [-SetupSeconds 600] [-KeepScratch]
# The record is written to <OutDir>: samples.jsonl, events.jsonl,
# record.json, summary.json and the scratch Pane's stderr.log.
param(
    [string]$OutDir = "measure-windows",
    [string]$Binary = "target/debug/pane.exe",
    [string]$Artifacts = "target/dist/artifacts",
    [int]$HiddenSeconds = 60,
    [int]$SettleSeconds = 30,
    [double]$SampleSeconds = 1,
    [int]$SetupSeconds = 600,
    [switch]$KeepScratch
)
$ErrorActionPreference = "Stop"

# The system's process records are read at their 64-bit layout below.
if ([IntPtr]::Size -ne 8) { throw "run this script in 64-bit PowerShell" }
if (-not (Test-Path -LiteralPath $Binary -PathType Leaf)) { throw "no pane binary at $Binary (cargo build -p pane)" }
if (-not (Test-Path -LiteralPath (Join-Path $Artifacts "pane-defaults.json"))) {
    throw "$Artifacts holds no default extensions (cargo xtask package-windows --dev)"
}
if (-not (Get-Command python -ErrorAction SilentlyContinue)) { throw "python is not on PATH" }
$Binary = (Resolve-Path -LiteralPath $Binary).Path
$Artifacts = (Resolve-Path -LiteralPath $Artifacts).Path
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$OutDir = (Resolve-Path -LiteralPath $OutDir).Path
# Pane's default extensions (#60), the set the workload waits for.
$defaults = @("calculator", "applications", "quicklinks", "files", "clipboard-history")
$utf8 = New-Object System.Text.UTF8Encoding $false   # proc_tree.py reads JSON without a BOM

# The scratch profile: a new folder of the temporary folder, never one that
# exists, and never inside the user's own Pane folder.
$userPane = [System.IO.Path]::GetFullPath((Join-Path $env:LOCALAPPDATA "Pane")).TrimEnd('\') + '\'
$scratch = Join-Path ([System.IO.Path]::GetTempPath()) ("pane-measure-{0}-{1}" -f (Get-Date -Format "yyyyMMdd-HHmmss"), [Guid]::NewGuid().ToString("N").Substring(0, 8))
$scratch = [System.IO.Path]::GetFullPath($scratch)
foreach ($folder in $scratch, $OutDir) {
    if (($folder.TrimEnd('\') + '\').StartsWith($userPane, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "$folder is inside the user's own Pane folder $userPane"
    }
}
$scratchData = Join-Path $scratch "data"
$scratchLocal = Join-Path $scratch "localappdata"
$scratchHome = Join-Path $scratch "home"
New-Item -ItemType Directory -Path $scratch, $scratchData, $scratchLocal, $scratchHome | Out-Null
$registry = Join-Path $scratchData "extensions\installed.json"

$samples = Join-Path $OutDir "samples.jsonl"
$events = Join-Path $OutDir "events.jsonl"
$recordPath = Join-Path $OutDir "record.json"
$summaryPath = Join-Path $OutDir "summary.json"
$stderrLog = Join-Path $OutDir "stderr.log"
foreach ($file in $samples, $events, $recordPath, $summaryPath) {
    [System.IO.File]::WriteAllText($file, "", $utf8)
}

# The sampler, compiled once per session under a name made from a digest
# of its source: Add-Type cannot define a type again, so a run after the
# source changed, in the same PowerShell session, compiles the new one
# under its own name rather than failing.
$sampler = @'
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Globalization;
using System.Runtime.InteropServices;
using System.Text;

// One sample of a process tree, as one JSON line of proc_tree.py's shape:
// the processes under a root (each started after its parent, so a reused
// parent id is not taken for the parent), their CPU time in 100 ns units
// and working set, and every thread's context switches, CPU time and name.
public static class PaneMeasure__DIGEST__ {
    [DllImport("ntdll.dll")]
    static extern int NtQuerySystemInformation(int infoClass, IntPtr info, int length, out int returned);
    [DllImport("kernel32.dll")]
    static extern IntPtr OpenThread(uint access, bool inherit, uint threadId);
    [DllImport("kernel32.dll")]
    static extern bool CloseHandle(IntPtr handle);
    // Windows 10 1607 and later; the name a thread was given with
    // SetThreadDescription, in memory the caller frees with LocalFree.
    [DllImport("kernel32.dll")]
    static extern int GetThreadDescription(IntPtr thread, out IntPtr description);
    [DllImport("kernel32.dll")]
    static extern IntPtr LocalFree(IntPtr memory);
    [DllImport("user32.dll")]
    public static extern bool IsWindowVisible(IntPtr window);
    [DllImport("user32.dll")]
    public static extern bool PostMessage(IntPtr window, uint message, IntPtr wParam, IntPtr lParam);
    [DllImport("user32.dll")]
    public static extern IntPtr GetForegroundWindow();

    const int SystemProcessInformation = 5;
    const int InfoLengthMismatch = unchecked((int)0xC0000004);
    const uint ThreadQueryLimitedInformation = 0x0800;
    // SYSTEM_PROCESS_INFORMATION and SYSTEM_THREAD_INFORMATION on 64-bit
    // Windows: their sizes, and the offsets read below.
    const int ProcessSize = 256;
    const int ThreadSize = 80;

    static IntPtr buffer = IntPtr.Zero;
    static int capacity = 0;
    static readonly Dictionary<string, string> names = new Dictionary<string, string>();

    class ThreadRecord {
        public long Tid, Created, Switches, Cpu;
    }

    class ProcessRecord {
        public long Pid, Ppid, Created, RssKb, Cpu;
        public string Comm;
        public List<ThreadRecord> Threads = new List<ThreadRecord>();
    }

    static List<ProcessRecord> Snapshot() {
        while (true) {
            if (buffer == IntPtr.Zero) {
                capacity = Math.Max(capacity, 8 << 20);
                buffer = Marshal.AllocHGlobal(capacity);
            }
            int returned;
            int status = NtQuerySystemInformation(SystemProcessInformation, buffer, capacity, out returned);
            if (status == InfoLengthMismatch) {
                Marshal.FreeHGlobal(buffer);
                buffer = IntPtr.Zero;
                capacity = Math.Max(capacity * 2, returned + (1 << 20));
                continue;
            }
            if (status < 0) {
                throw new InvalidOperationException("NtQuerySystemInformation answered 0x" + status.ToString("X8"));
            }
            break;
        }
        List<ProcessRecord> all = new List<ProcessRecord>();
        long offset = 0;
        while (true) {
            IntPtr entry = new IntPtr(buffer.ToInt64() + offset);
            uint next = (uint)Marshal.ReadInt32(entry, 0);
            int threads = Marshal.ReadInt32(entry, 4);
            ProcessRecord process = new ProcessRecord();
            process.Created = Marshal.ReadInt64(entry, 32);
            process.Cpu = Marshal.ReadInt64(entry, 40) + Marshal.ReadInt64(entry, 48);   // UserTime + KernelTime
            int nameBytes = Marshal.ReadInt16(entry, 56) & 0xFFFF;                      // ImageName.Length
            IntPtr name = Marshal.ReadIntPtr(entry, 64);                                  // ImageName.Buffer
            process.Comm = name == IntPtr.Zero ? "" : Marshal.PtrToStringUni(name, nameBytes / 2);
            process.Pid = Marshal.ReadIntPtr(entry, 80).ToInt64();                       // UniqueProcessId
            process.Ppid = Marshal.ReadIntPtr(entry, 88).ToInt64();                      // InheritedFromUniqueProcessId
            process.RssKb = Marshal.ReadIntPtr(entry, 144).ToInt64() / 1024;             // WorkingSetSize
            for (int i = 0; i < threads; i++) {
                IntPtr thread = new IntPtr(entry.ToInt64() + ProcessSize + (long)i * ThreadSize);
                ThreadRecord record = new ThreadRecord();
                record.Cpu = Marshal.ReadInt64(thread, 0) + Marshal.ReadInt64(thread, 8);   // KernelTime + UserTime
                record.Created = Marshal.ReadInt64(thread, 16);                             // CreateTime
                record.Tid = Marshal.ReadIntPtr(thread, 48).ToInt64();                      // ClientId.UniqueThread
                record.Switches = (uint)Marshal.ReadInt32(thread, 64);                      // ContextSwitches
                process.Threads.Add(record);
            }
            all.Add(process);
            if (next == 0) {
                break;
            }
            offset += next;
        }
        return all;
    }

    static List<ProcessRecord> Tree(List<ProcessRecord> all, long root) {
        List<ProcessRecord> found = new List<ProcessRecord>();
        foreach (ProcessRecord process in all) {
            if (process.Pid == root) {
                found.Add(process);
                break;
            }
        }
        for (int i = 0; i < found.Count; i++) {
            ProcessRecord parent = found[i];
            foreach (ProcessRecord process in all) {
                if (process.Ppid == parent.Pid && process.Pid != parent.Pid
                    && process.Created >= parent.Created && !found.Contains(process)) {
                    found.Add(process);
                }
            }
        }
        found.Sort(delegate (ProcessRecord a, ProcessRecord b) { return a.Pid.CompareTo(b.Pid); });
        return found;
    }

    // The thread's name, or `unnamed` (the process's own name, as Linux
    // names a thread nobody named) when it has none or cannot be asked.
    // A name is kept per thread id and creation time, so a reused id is
    // asked again.
    static string ThreadName(ThreadRecord thread, string unnamed) {
        string key = thread.Tid.ToString(CultureInfo.InvariantCulture) + ":"
            + thread.Created.ToString(CultureInfo.InvariantCulture);
        string known;
        if (names.TryGetValue(key, out known)) {
            return known;
        }
        IntPtr handle = OpenThread(ThreadQueryLimitedInformation, false, (uint)thread.Tid);
        if (handle == IntPtr.Zero) {
            return unnamed;
        }
        IntPtr text = IntPtr.Zero;
        try {
            if (GetThreadDescription(handle, out text) < 0 || text == IntPtr.Zero) {
                return unnamed;
            }
            string name = Marshal.PtrToStringUni(text);
            if (String.IsNullOrEmpty(name)) {
                return unnamed;
            }
            names[key] = name;
            return name;
        } finally {
            if (text != IntPtr.Zero) {
                LocalFree(text);
            }
            CloseHandle(handle);
        }
    }

    static string Text(string value) {
        StringBuilder text = new StringBuilder("\"");
        foreach (char c in value) {
            if (c == '"' || c == '\\') {
                text.Append('\\').Append(c);
            } else if (c < ' ') {
                text.Append("\\u").Append(((int)c).ToString("x4", CultureInfo.InvariantCulture));
            } else {
                text.Append(c);
            }
        }
        return text.Append('"').ToString();
    }

    static string Number(double value) {
        return value.ToString("0.###", CultureInfo.InvariantCulture);
    }

    static string Whole(long value) {
        return value.ToString(CultureInfo.InvariantCulture);
    }

    // One sample of the tree under `root` in `phase`: empty once it is gone.
    public static string Sample(string phase, long root) {
        double t = Stopwatch.GetTimestamp() / (double)Stopwatch.Frequency;
        double wall = (DateTime.UtcNow - new DateTime(1970, 1, 1, 0, 0, 0, DateTimeKind.Utc)).TotalSeconds;
        List<ProcessRecord> tree = root > 0 ? Tree(Snapshot(), root) : new List<ProcessRecord>();
        long rss = 0, cpu = 0;
        StringBuilder json = new StringBuilder();
        json.Append("{\"t\":").Append(Number(t)).Append(",\"wall\":").Append(Number(wall));
        json.Append(",\"phase\":").Append(Text(phase)).Append(",\"root\":").Append(Whole(root));
        json.Append(",\"tick_hz\":10000000,\"switch_kind\":\"all\",\"processes\":[");
        for (int i = 0; i < tree.Count; i++) {
            ProcessRecord process = tree[i];
            rss += process.RssKb;
            cpu += process.Cpu;
            string unnamed = process.Comm.EndsWith(".exe", StringComparison.OrdinalIgnoreCase)
                ? process.Comm.Substring(0, process.Comm.Length - 4) : process.Comm;
            if (i > 0) {
                json.Append(',');
            }
            json.Append("{\"pid\":").Append(Whole(process.Pid)).Append(",\"ppid\":").Append(Whole(process.Ppid));
            json.Append(",\"comm\":").Append(Text(process.Comm)).Append(",\"rss_kb\":").Append(Whole(process.RssKb));
            json.Append(",\"cpu_ticks\":").Append(Whole(process.Cpu)).Append(",\"threads\":[");
            process.Threads.Sort(delegate (ThreadRecord a, ThreadRecord b) { return a.Tid.CompareTo(b.Tid); });
            for (int j = 0; j < process.Threads.Count; j++) {
                ThreadRecord thread = process.Threads[j];
                if (j > 0) {
                    json.Append(',');
                }
                json.Append("{\"tid\":").Append(Whole(thread.Tid));
                json.Append(",\"name\":").Append(Text(ThreadName(thread, unnamed)));
                json.Append(",\"switches\":").Append(Whole(thread.Switches));
                json.Append(",\"cpu_ticks\":").Append(Whole(thread.Cpu)).Append('}');
            }
            json.Append("]}");
        }
        json.Append("],\"nproc\":").Append(Whole(tree.Count)).Append(",\"rss_kb\":").Append(Whole(rss));
        json.Append(",\"cpu_ticks\":").Append(Whole(cpu)).Append('}');
        return json.ToString();
    }
}
'@
$digest = [System.Security.Cryptography.SHA256]::Create().ComputeHash($utf8.GetBytes($sampler))
$typeName = "PaneMeasure" + (-join ($digest[0..7] | ForEach-Object { $_.ToString("x2") }))
if (-not ($typeName -as [type])) {
    Add-Type -TypeDefinition $sampler.Replace("PaneMeasure__DIGEST__", $typeName)
}
$measure = $typeName -as [type]

function Write-Line($file, $line) { [System.IO.File]::AppendAllText($file, $line + "`n", $utf8) }
function Write-Event($phase, $key, $value) {
    if ($key -eq "window") {
        Write-Line $events ('{{"phase": "{0}", "event": "window", "latency_ms": {1}}}' -f $phase, $value)
    } else {
        Write-Line $events ('{{"phase": "{0}", "{1}": {2}}}' -f $phase, $key, $value)
    }
}
# Whether the scratch Pane recorded every default extension.
function Defaults-Recorded {
    if (-not (Test-Path -LiteralPath $registry)) { return $false }
    try {
        $text = [System.IO.File]::ReadAllText($registry)
    } catch {
        return $false   # replaced at that moment; asked again at the next sample
    }
    foreach ($default in $defaults) {
        if (-not $text.Contains('"default": "' + $default + '"')) { return $false }
    }
    return $true
}
# Samples the scratch Pane's tree as $phase every $SampleSeconds until
# $done answers true ($true then) or $seconds have passed ($false then).
# With -Hidden, the launcher showing itself ends the run.
function Sample-For($phase, $seconds, [scriptblock]$done, [switch]$Hidden) {
    $deadline = [DateTime]::UtcNow.AddSeconds($seconds)
    while ($true) {
        if ($script:pane.HasExited) { throw "Pane exited during the measurement (see stderr.log)" }
        Write-Line $samples ($measure::Sample($phase, $script:pane.Id))
        if ($Hidden -and $measure::IsWindowVisible($script:window)) { throw "the launcher showed itself during the hidden phase" }
        if ($done -and (& $done)) { return $true }
        if ([DateTime]::UtcNow -ge $deadline) { return $false }
        Start-Sleep -Milliseconds ([int]($SampleSeconds * 1000))
    }
}
# Escape, posted to Pane's window alone: a key message no other window
# receives, which Pane handles as the key (Escape at a blank root search
# hides the launcher).
function Post-Escape {
    $escape = [IntPtr]::new(0x1B)
    $measure::PostMessage($script:window, 0x0100, $escape, [IntPtr]::new(0x00010001L)) | Out-Null   # WM_KEYDOWN
    Start-Sleep -Milliseconds 50
    $measure::PostMessage($script:window, 0x0101, $escape, [IntPtr]::new(0xC0010001L)) | Out-Null   # WM_KEYUP
}
function Wait-Hidden($milliseconds) {
    for ($waited = 0; $waited -lt $milliseconds; $waited += 100) {
        if (-not $measure::IsWindowVisible($script:window)) { return $true }
        Start-Sleep -Milliseconds 100
    }
    return (-not $measure::IsWindowVisible($script:window))
}
# Hides the launcher after its one show: the posted Escape, then, only if
# Pane's own window is the foreground window, a typed one. Nothing is ever
# typed into another window.
function Hide-Launcher {
    Post-Escape
    if (Wait-Hidden 3000) { return "posted Escape" }
    if ($measure::GetForegroundWindow() -eq $script:window) {
        Add-Type -AssemblyName System.Windows.Forms
        [System.Windows.Forms.SendKeys]::SendWait("{ESC}")
        if (Wait-Hidden 3000) { return "typed Escape" }
    }
    throw "the launcher could not be hidden; nothing was typed into another window"
}
function Stop-ScratchPane {
    if (-not $script:pane -or $script:pane.HasExited) { return }
    # Closing the launcher's window quits Pane, which removes its tray icon
    # and releases its hotkeys; a Pane that does not quit is stopped.
    if ($script:window -ne [IntPtr]::Zero) {
        $measure::PostMessage($script:window, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null   # WM_CLOSE
    }
    if (-not $script:pane.WaitForExit(10000)) {
        Stop-Process -Id $script:pane.Id -Force
        $script:pane.WaitForExit()
    }
}
# The user's own launch-at-login registration: the "Pane" value of the
# per-user startup list, or $null.
$runKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run"
function Login-Command {
    $value = Get-ItemProperty -Path $runKey -Name Pane -ErrorAction SilentlyContinue
    if ($value) { return $value.Pane }
    return $null
}

$record = [ordered]@{
    format = 1
    script = "measure-windows.ps1"
    date = (Get-Date).ToString("o")
    os = [System.Environment]::OSVersion.VersionString
    windowsBuild = (Get-ItemProperty "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion" -ErrorAction SilentlyContinue | Select-Object DisplayVersion, CurrentBuild, UBR)
    architecture = $env:PROCESSOR_ARCHITECTURE
    processors = [System.Environment]::ProcessorCount
    pane = [ordered]@{
        path = $Binary
        bytes = (Get-Item -LiteralPath $Binary).Length
        sha256 = (Get-FileHash -LiteralPath $Binary -Algorithm SHA256).Hash
        profile = $(if ($Binary -match '\\debug\\') { "debug" } else { "release" })
    }
    commit = ""
    workload = [ordered]@{
        hiddenSeconds = $HiddenSeconds
        settleSeconds = $SettleSeconds
        sampleSeconds = $SampleSeconds
        setupSeconds = $SetupSeconds
        defaultExtensions = $defaults
    }
    counted = "context switches of each thread (SYSTEM_THREAD_INFORMATION.ContextSwitches); CPU in 100 ns; rss_kb is the working set"
    otherPaneProcesses = @(Get-Process -Name pane -ErrorAction SilentlyContinue).Count
    loginRegistered = $false
    loginRegistrationRestored = $false
    hiddenBy = $null
    scratchKept = [bool]$KeepScratch
}
# Another Pane (the user's own) is never touched, but it shares the tray,
# the hotkeys and the clipboard with the scratch Pane: said loudly.
if ($record.otherPaneProcesses -gt 0) {
    Write-Warning ("{0} other pane process(es) run, the user's own Pane perhaps. This script never touches them, but they share the tray, the hotkeys and the clipboard with the scratch Pane: quit them for a run worth recording." -f $record.otherPaneProcesses)
}
try {
    $answer = & git rev-parse HEAD
    if ($LASTEXITCODE -eq 0 -and $answer) { $record.commit = "$answer".Trim() }
} catch { }   # git may be absent; what ran is recorded without it

$loginBefore = Login-Command
$record.loginRegistered = $null -ne $loginBefore
# The scratch Pane's settings record holds the choice the startup list
# already reflects, so the scratch Pane, reconciling the two at start,
# finds nothing to repair and leaves the user's registration alone.
$settings = '{{"version": 1, "launchAtLogin": {0}}}' -f $(if ($record.loginRegistered) { "true" } else { "false" })
[System.IO.File]::WriteAllText((Join-Path $scratchData "settings.json"), $settings, $utf8)

$script:pane = $null
$script:window = [IntPtr]::Zero
$server = $null
$failure = $null
try {
    $portFile = Join-Path $OutDir "artifact-server.port"
    if (Test-Path -LiteralPath $portFile) { Remove-Item -LiteralPath $portFile }
    $server = Start-Process python -PassThru -NoNewWindow `
        -ArgumentList @("`"$PSScriptRoot/artifact_server.py`"", "`"$Artifacts`"", "`"$portFile`"") `
        -RedirectStandardError (Join-Path $OutDir "artifact-server.log")
    for ($i = 0; $i -lt 600 -and -not (Test-Path -LiteralPath $portFile) -and -not $server.HasExited; $i++) { Start-Sleep -Milliseconds 100 }
    if (-not (Test-Path -LiteralPath $portFile)) { throw "the local artifact source did not start (see artifact-server.log)" }

    # The scratch Pane's environment, for its start only: every PANE_
    # variable of this shell is left out, the scratch ones are set, and
    # this shell's own are put back as soon as Pane has started.
    $saved = @{}
    foreach ($variable in Get-ChildItem Env: | Where-Object { $_.Name -like "PANE_*" -or $_.Name -eq "LOCALAPPDATA" }) {
        $saved[$variable.Name] = $variable.Value
    }
    try {
        foreach ($name in @($saved.Keys)) { Remove-Item "Env:$name" }
        $env:PANE_DATA_DIR = $scratchData
        $env:LOCALAPPDATA = $scratchLocal
        $env:PANE_TEST_FILE_INDEX_HOME = $scratchHome
        $env:PANE_ARTIFACTS = "http://127.0.0.1:$((Get-Content -LiteralPath $portFile).Trim())/"
        $started = [System.Diagnostics.Stopwatch]::StartNew()
        $script:pane = Start-Process -FilePath $Binary -PassThru -RedirectStandardError $stderrLog
    } finally {
        foreach ($name in "PANE_DATA_DIR", "LOCALAPPDATA", "PANE_TEST_FILE_INDEX_HOME", "PANE_ARTIFACTS") {
            Remove-Item "Env:$name" -ErrorAction SilentlyContinue
        }
        foreach ($name in $saved.Keys) { Set-Item "Env:$name" $saved[$name] }
    }

    # The one show: the window every start opens.
    for ($i = 0; $i -lt 150 -and $script:pane.MainWindowHandle -eq [IntPtr]::Zero; $i++) {
        if ($script:pane.HasExited) { throw "Pane exited before its window appeared (see stderr.log)" }
        Start-Sleep -Milliseconds 200
        $script:pane.Refresh()
    }
    if ($script:pane.MainWindowHandle -eq [IntPtr]::Zero) { throw "the Pane window did not appear" }
    $script:window = $script:pane.MainWindowHandle
    Write-Event "hidden-setup" "window" $started.ElapsedMilliseconds
    Start-Sleep -Seconds 2   # the launcher draws its first frames, as the smoke waits
    $record.hiddenBy = Hide-Launcher

    # The setup: the defaults acquired and recorded, then the settling.
    $recorded = Sample-For "hidden-setup" $SetupSeconds { Defaults-Recorded } -Hidden
    if (-not $recorded) { throw "the default extensions were not recorded within $SetupSeconds s (see stderr.log)" }
    Sample-For "hidden-setup" $SettleSeconds $null -Hidden | Out-Null
    Write-Event "hidden-idle" "default_extensions" $defaults.Count
    Sample-For "hidden-idle" $HiddenSeconds $null -Hidden | Out-Null
} catch {
    $failure = $_
} finally {
    Stop-ScratchPane
    if ($server -and -not $server.HasExited) { Stop-Process -Id $server.Id -ErrorAction SilentlyContinue }
    $loginAfter = Login-Command
    if ($loginAfter -ne $loginBefore) {
        # Not expected (the settings record above prevents it); the user's
        # registration is put back as it was.
        if ($null -eq $loginBefore) {
            Remove-ItemProperty -Path $runKey -Name Pane -ErrorAction SilentlyContinue
        } else {
            Set-ItemProperty -Path $runKey -Name Pane -Value $loginBefore
        }
        $record.loginRegistrationRestored = $true
        Write-Warning "the startup list's Pane value changed during the run and was put back"
    }
    if (-not $KeepScratch) {
        # Only the folder this run made: it holds what Clipboard History
        # recorded while Pane ran.
        $leaf = Split-Path -Leaf $scratch
        if ($leaf -like "pane-measure-*" -and (Test-Path -LiteralPath $scratch)) {
            try { Remove-Item -LiteralPath $scratch -Recurse -Force } catch { Write-Warning "the scratch profile $scratch could not be removed: $($_.Exception.Message)" }
        }
    } else {
        $record.scratch = $scratch
    }
    [System.IO.File]::WriteAllText($recordPath, ($record | ConvertTo-Json -Depth 5), $utf8)
}
if ($failure) { throw $failure }

python "$PSScriptRoot/proc_tree.py" summary $samples $events $summaryPath $recordPath
if ($LASTEXITCODE -ne 0) { throw "proc_tree.py could not summarize the samples" }
# The phase at a glance; summary.json holds every thread.
$hidden = (Get-Content -Raw -Encoding UTF8 -LiteralPath $summaryPath | ConvertFrom-Json).phases."hidden-idle"
"hidden-idle: CPU share {0}, {1} wake-ups a second, working set max {2} KiB" -f $hidden.cpu_share, $hidden.wakeups.per_second, $hidden.rss_kb.max
$hidden.wakeups.by_thread.PSObject.Properties |
    Sort-Object { $_.Value.count } -Descending |
    Select-Object -First 10 |
    ForEach-Object { "  {0,-28} {1,4} threads {2,9} a second" -f $_.Name, $_.Value.threads, $_.Value.per_second }
"record in $OutDir"
