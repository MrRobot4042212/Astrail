// CPU temperature sidecar for Meteor's metrics overlay.
//
// LibreHardwareMonitor reads the CPU package temperature (Ryzen Tctl/Tdie, Intel
// core) via a kernel driver it loads at runtime — so this must run elevated; if
// the driver can't load, no temperature sensor appears and we print nothing, and
// the Rust side simply omits CPU temp (best-effort, like PresentMon).
//
// Protocol: one integer (°C) per line on stdout, ~once per second. That's all the
// Rust controller (cputemp.rs) parses.
//
// Shutdown protocol: the parent closes our stdin, we see EOF, call computer.Close()
// and exit. This matters more than it looks — Open() makes LibreHardwareMonitor
// install and start a kernel driver via the SCM, and only Close() unloads it. Being
// TerminateProcess'd (which is what a kill or the parent's kill-on-close Job Object
// does) skips .NET finalizers, so the driver would stay loaded and registered for
// the rest of the boot. That residue is what vulnerable-driver blocklists and kernel
// anti-cheats look for, so it must not depend on the happy path alone.

using System.Globalization;
using LibreHardwareMonitor.Hardware;

// `cputemp.exe --self-test`: checks the sensor ranking without opening the driver,
// so CI (no admin, no kernel driver) can exercise the selection logic.
if (args.Length > 0 && args[0] == "--self-test")
    return SensorRank.SelfTest();

var computer = new Computer { IsCpuEnabled = true };
try
{
    computer.Open();
}
catch (Exception e)
{
    Console.Error.WriteLine("cputemp: open failed: " + e.Message);
    return 1;
}

// Unload the driver exactly once, whichever path we leave by.
var closed = 0;
void Shutdown()
{
    if (Interlocked.Exchange(ref closed, 1) != 0) return;
    try { computer.Close(); }
    catch (Exception e) { Console.Error.WriteLine("cputemp: close failed: " + e.Message); }
}

// Covers a normal return and an unhandled exception; TerminateProcess still cannot
// be intercepted, which is why the parent asks over stdin instead of killing.
AppDomain.CurrentDomain.ProcessExit += (_, _) => Shutdown();

using var stop = new ManualResetEventSlim(false);

Console.CancelKeyPress += (_, e) =>
{
    // Handle it ourselves so the sampling loop can unwind through Shutdown().
    e.Cancel = true;
    stop.Set();
};

// EOF on stdin = the parent dropped its write handle and wants us gone.
new Thread(() =>
{
    try { Console.In.ReadToEnd(); }
    catch { /* closed underneath us; treat as a stop request */ }
    stop.Set();
})
{ IsBackground = true, Name = "stdin-watch" }.Start();

var visitor = new UpdateVisitor();

while (!stop.IsSet)
{
    computer.Accept(visitor);

    float? temp = null;
    foreach (IHardware hw in computer.Hardware)
    {
        if (hw.HardwareType != HardwareType.Cpu) continue;

        var readings = new List<(string Name, float Value)>();
        foreach (ISensor s in hw.Sensors)
        {
            if (s.SensorType != SensorType.Temperature || s.Value is not float v) continue;
            readings.Add((s.Name ?? string.Empty, v));
        }
        temp = SensorRank.Pick(readings);
        if (temp is not null) break;
    }

    if (temp is float t)
    {
        Console.WriteLine(((int)Math.Round(t)).ToString(CultureInfo.InvariantCulture));
        Console.Out.Flush();
    }

    // Wait, but wake immediately when the parent asks us to stop, so a shutdown
    // never has to sit through the rest of a sampling second.
    if (stop.Wait(1000)) break;
}

Shutdown();
return 0;

// Picks the CPU temperature from one CPU's temperature sensors.
//
// Ordered preference, first hit per rank. The previous loop let the *last* sensor
// whose name contained Package/Tctl/Tdie win, so on multi-CCD Ryzen a per-CCD
// sensor could displace the package reading depending on enumeration order.
// Sensor names follow LibreHardwareMonitor's naming (reported, not verified against
// every LHM version): Intel "CPU Package" / "Core Max" / "CPU Core #n"; AMD
// "Core (Tctl/Tdie)", "Core (Tdie)", "Core (Tctl)", "CCDs Max (Tdie)", "CCD1 (Tdie)".
static class SensorRank
{
    // Lower is better; null = never use (averages, TjMax distances).
    public static int? Rank(string name)
    {
        if (name.Contains("Distance") || name.Contains("Average")) return null;
        if (name.Contains("Package")) return 0;
        bool ccd = name.Contains("CCD");
        if (!ccd && name.Contains("Tdie") && !name.Contains("Tctl")) return 1;
        if (!ccd && name.Contains("Tctl")) return 2;
        if (name.Contains("CCDs Max")) return 3;
        if (name.Contains("Core Max")) return 4;
        if (ccd || name.Contains("Core")) return 5;
        return null;
    }

    public static float? Pick(IEnumerable<(string Name, float Value)> readings)
    {
        int bestRank = int.MaxValue;
        float? best = null;
        foreach (var (name, value) in readings)
        {
            if (Rank(name) is not int r || r > bestRank) continue;
            if (r < bestRank) { bestRank = r; best = value; }
            // Individual cores/CCDs: report the hottest; better ranks keep the first hit.
            else if (r == 5 && best is float b && value > b) best = value;
        }
        return best;
    }

    public static int SelfTest()
    {
        var failures = 0;
        void Check(string label, float? got, float? want)
        {
            if (got == want) return;
            Console.Error.WriteLine($"FAIL {label}: got {got?.ToString(CultureInfo.InvariantCulture) ?? "null"}, want {want?.ToString(CultureInfo.InvariantCulture) ?? "null"}");
            failures++;
        }

        Check("package beats a later CCD", Pick(new[] { ("CPU Package", 60f), ("CCD1 (Tdie)", 70f) }), 60f);
        Check("package beats an earlier CCD", Pick(new[] { ("CCD2 (Tdie)", 72f), ("Core (Tctl/Tdie)", 65f), ("CCD1 (Tdie)", 70f) }), 65f);
        Check("Tdie beats Tctl", Pick(new[] { ("Core (Tctl)", 75f), ("Core (Tdie)", 65f) }), 65f);
        Check("CCDs Max beats one CCD", Pick(new[] { ("CCD1 (Tdie)", 61f), ("CCDs Max (Tdie)", 68f), ("CCDs Average (Tdie)", 64f) }), 68f);
        Check("hottest individual core", Pick(new[] { ("CPU Core #1", 50f), ("CPU Core #2", 58f), ("CPU Core #3", 55f) }), 58f);
        Check("TjMax distance is not a temperature", Pick(new[] { ("CPU Core #1 Distance to TjMax", 45f), ("CPU Core #1", 55f) }), 55f);
        Check("averages are ignored", Pick(new[] { ("Core Average", 50f) }), null);
        Check("no sensors", Pick(Array.Empty<(string, float)>()), null);

        Console.WriteLine(failures == 0 ? "cputemp self-test: ok" : $"cputemp self-test: {failures} failure(s)");
        return failures == 0 ? 0 : 1;
    }
}

// Walks the hardware tree and calls Update() so sensor values refresh.
sealed class UpdateVisitor : IVisitor
{
    public void VisitComputer(IComputer computer) => computer.Traverse(this);
    public void VisitHardware(IHardware hardware)
    {
        hardware.Update();
        foreach (IHardware sub in hardware.SubHardware) sub.Accept(this);
    }
    public void VisitSensor(ISensor sensor) { }
    public void VisitParameter(IParameter parameter) { }
}
